use std::fmt::{Display, Formatter};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use nimbus_core::is_valid_dns_hostname;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressProtocol {
    Tcp,
    Http,
    Https,
    /// Observable cleartext WebSocket request. Runtime gateways can identify
    /// this protocol before transport. A supervisor proxy cannot distinguish
    /// it from HTTP without parsing an upgrade and rejects rules of this kind.
    Ws,
    /// Observable TLS WebSocket request. Runtime gateways can identify this
    /// protocol before transport. A supervisor proxy sees only an opaque HTTPS
    /// CONNECT authority and rejects rules of this kind.
    Wss,
}

/// Default-deny egress allowlist: the PDP authorizes a request only when some
/// rule in `allow` matches it. An empty policy denies everything; there is no
/// implicit allow and no deny list. This is the policy-decision point — it is
/// pure (`nimbus-core` only, zero I/O) and never performs the egress itself.
/// The enforcing PEP (nimbus-proxy / netns firewall / isolate gateway) consumes
/// the [`EgressAuthorization`] this returns.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EgressPolicy {
    /// Allowlist of rules; a request is authorized by the first compiled rule
    /// that matches it. Empty means deny-all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<EgressRule>,
}

impl EgressPolicy {
    pub fn deny_all() -> Self {
        Self::default()
    }

    pub fn new(rules: impl IntoIterator<Item = EgressRule>) -> Self {
        Self {
            allow: rules.into_iter().collect(),
        }
    }

    pub fn with_rule(mut self, rule: EgressRule) -> Self {
        self.allow.push(rule);
        self
    }

    pub fn rules(&self) -> &[EgressRule] {
        &self.allow
    }

    pub fn is_deny_all(&self) -> bool {
        self.allow.is_empty()
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        self.compile().map(|_| ())
    }

    pub fn compile(&self) -> std::result::Result<CompiledEgressPolicy, String> {
        let mut names = std::collections::BTreeSet::new();
        let mut rules = Vec::with_capacity(self.allow.len());
        for rule in &self.allow {
            rule.validate()?;
            if !names.insert(rule.name.as_str()) {
                return Err(format!(
                    "sandbox egress rule {:?} is declared more than once",
                    rule.name
                ));
            }
            rules.push(rule.canonicalized());
        }
        rules.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(CompiledEgressPolicy {
            policy: Self { allow: rules },
        })
    }

    /// Compile a policy for the host-side supervisor proxy.
    ///
    /// The proxy can enforce TCP, cleartext HTTP, and HTTPS authority grants.
    /// It cannot authenticate `ws` inside HTTP or `wss` inside an opaque TLS
    /// CONNECT tunnel. Those protocol values are reserved for runtime gateways
    /// that observe `new WebSocket()` before transport.
    pub fn compile_for_supervisor_proxy(
        &self,
    ) -> std::result::Result<CompiledEgressPolicy, String> {
        let compiled = self.compile()?;
        compiled.validate_for_supervisor_proxy()?;
        Ok(compiled)
    }

    pub fn authorize(&self, request: &EgressRequest) -> EgressAuthorization {
        match self.compile() {
            Ok(compiled) => compiled.authorize(request),
            Err(message) => {
                EgressAuthorization::deny(format!("sandbox egress policy invalid: {message}"))
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct CompiledEgressPolicy {
    policy: EgressPolicy,
}

impl CompiledEgressPolicy {
    pub fn deny_all() -> Self {
        Self {
            policy: EgressPolicy::deny_all(),
        }
    }

    pub fn policy(&self) -> &EgressPolicy {
        &self.policy
    }

    pub fn into_policy(self) -> EgressPolicy {
        self.policy
    }

    /// Validate that every rule can be enforced by the supervisor proxy.
    pub fn validate_for_supervisor_proxy(&self) -> std::result::Result<(), String> {
        if let Some(rule) = self
            .policy
            .allow
            .iter()
            .find(|rule| matches!(rule.protocol, EgressProtocol::Ws | EgressProtocol::Wss))
        {
            return Err(format!(
                "sandbox egress rule `{}` uses protocol `{}`, which is supported only by an observable runtime gateway; the supervisor proxy cannot distinguish WebSocket traffic inside HTTP or opaque TLS",
                rule.name,
                egress_protocol_label(rule.protocol)
            ));
        }
        Ok(())
    }

    pub fn authorize(&self, request: &EgressRequest) -> EgressAuthorization {
        self.authorize_with_l7(request, true)
    }

    /// Authorize the authority phase of an HTTPS CONNECT request. This checks
    /// protocol, host, port, and the internal-address guard, but deliberately
    /// defers HTTP method/path constraints until the decrypted inner request.
    pub fn authorize_connect(&self, request: &EgressRequest) -> EgressAuthorization {
        self.authorize_with_l7(request, false)
    }

    fn authorize_with_l7(&self, request: &EgressRequest, enforce_l7: bool) -> EgressAuthorization {
        let canonical_request_host = match request.canonical_host_result() {
            Ok(host) => host,
            Err(error) => {
                return EgressAuthorization::deny(format!(
                    "sandbox egress request host authority rejected: {error}"
                ));
            }
        };
        let mut matched_but_denied = None;
        let mut allowed_fallback = None;
        for rule in &self.policy.allow {
            if !rule.matches_l4(request, &canonical_request_host) {
                continue;
            }
            if request.targets_internal_address() && !rule.allow_internal_ips {
                matched_but_denied.get_or_insert_with(|| {
                    format!(
                        "sandbox egress rule `{}` matched {}, but internal/non-global targets require allow_internal_ips=true",
                        rule.name,
                        request.target_summary()
                    )
                });
                continue;
            }
            if enforce_l7 && !rule.matches_l7(request) {
                matched_but_denied.get_or_insert_with(|| {
                    format!(
                        "sandbox egress rule `{}` matched {}, but HTTP method/path policy denied the request",
                        rule.name,
                        request.target_summary()
                    )
                });
                continue;
            }
            let allowed =
                EgressAuthorization::allow(rule.name.clone(), rule.requires_proxy_enforcement());
            if enforce_l7 || rule.requires_connect_interception() {
                return allowed;
            }
            allowed_fallback.get_or_insert(allowed);
        }

        if let Some(allowed) = allowed_fallback {
            return allowed;
        }

        if let Some(reason) = matched_but_denied {
            return EgressAuthorization::deny(reason);
        }

        EgressAuthorization::deny(format!(
            "sandbox egress default deny: no rule matched {}",
            request.target_summary()
        ))
    }

    /// Hostname-only pre-DNS authorization used by the PEP to decide whether a
    /// caller-controlled host is even eligible for resolution. This deliberately
    /// clears `resolved_ip`: it is not the SSRF/internal-IP gate. The PEP must
    /// call [`Self::authorize`] again with the exact selected resolved address
    /// before dialing.
    pub fn authorize_hostname_without_resolved_ip(
        &self,
        request: &EgressRequest,
    ) -> EgressAuthorization {
        let mut request = request.clone();
        request.resolved_ip = None;
        self.authorize(&request)
    }

    /// Hostname-only form of [`Self::authorize_connect`]. The caller must
    /// authorize the selected resolved IP again before dialing.
    pub fn authorize_connect_hostname_without_resolved_ip(
        &self,
        request: &EgressRequest,
    ) -> EgressAuthorization {
        let mut request = request.clone();
        request.resolved_ip = None;
        self.authorize_connect(&request)
    }

    /// Whether any authority-matching rule requires the decrypted HTTPS
    /// request to be inspected for method/path, credentials, or DLP.
    pub fn connect_requires_interception(&self, request: &EgressRequest) -> bool {
        let Ok(canonical_request_host) = request.canonical_host_result() else {
            return false;
        };
        self.policy.allow.iter().any(|rule| {
            rule.matches_l4(request, &canonical_request_host)
                && (!request.targets_internal_address() || rule.allow_internal_ips)
                && rule.requires_connect_interception()
        })
    }
}

/// A single allow rule. A request matches only when protocol, canonical host,
/// and port all match (L4) and — for HTTP(S) or WebSocket — the method and path-prefix
/// constraints are satisfied (L7). Unless `allow_internal_ips` is set, a rule
/// will not authorize a request whose host or resolved IP is internal/
/// non-global, which is the SSRF / metadata-endpoint guard. An optional
/// `credential` is injected by the PEP, and `dlp` rules are inspected by the
/// PEP; both make the rule proxy-enforced (see
/// [`EgressAuthorization::requires_proxy_enforcement`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EgressRule {
    /// Unique, concrete rule name (no whitespace, not `*`); used in diagnostics.
    pub name: String,
    /// Protocol the rule authorizes (`tcp`, `http`, `https`, `ws`, or `wss`).
    pub protocol: EgressProtocol,
    /// Concrete destination host: a bare DNS name or IP literal, no wildcards.
    pub host: String,
    /// Destination port; must be non-zero.
    pub port: u16,
    /// Allowed HTTP methods (uppercase tokens). Empty means any method. Invalid
    /// for `tcp` rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<String>,
    /// Allowed request path prefixes, matched per path segment (a prefix `/v1`
    /// admits `/v1` and `/v1/x` but not `/v1beta`). Empty means any path.
    /// Invalid for `tcp` rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_prefixes: Vec<String>,
    /// Opt in to targeting internal/non-global addresses. Off by default so the
    /// SSRF / metadata-endpoint gate stays fail-closed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_internal_ips: bool,
    /// Optional managed credential the PEP injects on matching requests; its
    /// presence makes the rule proxy-enforced. Runtime-only `ws` and `wss`
    /// rules cannot use this field because their observable gateway is not a
    /// credential-injecting proxy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<EgressCredentialInjection>,
    /// Optional DLP inspection rules the PEP applies to the request body; any
    /// rule makes the rule proxy-enforced. Runtime-only `ws` and `wss` rules
    /// cannot use this field because their observable gateway does not inspect
    /// WebSocket frames.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dlp: Vec<EgressDlpRule>,
}

impl EgressRule {
    pub fn new(
        name: impl Into<String>,
        protocol: EgressProtocol,
        host: impl Into<String>,
        port: u16,
    ) -> Self {
        Self {
            name: name.into(),
            protocol,
            host: host.into(),
            port,
            methods: Vec::new(),
            path_prefixes: Vec::new(),
            allow_internal_ips: false,
            credential: None,
            dlp: Vec::new(),
        }
    }

    pub fn with_methods(mut self, methods: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.methods = methods.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_path_prefixes(
        mut self,
        path_prefixes: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.path_prefixes = path_prefixes.into_iter().map(Into::into).collect();
        self
    }

    pub fn allow_internal_ips(mut self, allow_internal_ips: bool) -> Self {
        self.allow_internal_ips = allow_internal_ips;
        self
    }

    pub fn with_credential_injection(mut self, credential: EgressCredentialInjection) -> Self {
        self.credential = Some(credential);
        self
    }

    pub fn with_dlp_rules(mut self, dlp: impl IntoIterator<Item = EgressDlpRule>) -> Self {
        self.dlp = dlp.into_iter().collect();
        self
    }

    fn validate(&self) -> std::result::Result<(), String> {
        validate_rule_name(&self.name)?;
        validate_egress_host(&self.host, self.allow_internal_ips)?;
        if self.port == 0 {
            return Err(format!(
                "sandbox egress rule `{}` port must not be 0",
                self.name
            ));
        }
        if matches!(self.protocol, EgressProtocol::Tcp)
            && (!self.methods.is_empty() || !self.path_prefixes.is_empty())
        {
            return Err(format!(
                "sandbox egress rule `{}` uses tcp and must not set HTTP methods or path_prefixes",
                self.name
            ));
        }
        if matches!(self.protocol, EgressProtocol::Ws | EgressProtocol::Wss)
            && self.requires_proxy_enforcement()
        {
            return Err(format!(
                "sandbox egress rule `{}` uses protocol `{}` with credential injection or DLP, but the observable runtime gateway cannot apply proxy-mediated enforcement",
                self.name,
                egress_protocol_label(self.protocol)
            ));
        }
        for method in &self.methods {
            validate_http_method(method, &self.name)?;
        }
        for path_prefix in &self.path_prefixes {
            validate_path_prefix(path_prefix, &self.name)?;
        }
        if let Some(credential) = &self.credential {
            credential.validate(&self.name)?;
        }
        for rule in &self.dlp {
            rule.validate(&self.name)?;
        }
        Ok(())
    }

    fn canonicalized(&self) -> Self {
        let mut methods = self.methods.clone();
        methods.sort();
        methods.dedup();
        let mut path_prefixes = self.path_prefixes.clone();
        path_prefixes.sort();
        path_prefixes.dedup();
        Self {
            name: self.name.clone(),
            protocol: self.protocol,
            host: canonicalize_authority_host(&self.host)
                .expect("egress rule host was validated before canonicalization"),
            port: self.port,
            methods,
            path_prefixes,
            allow_internal_ips: self.allow_internal_ips,
            credential: self.credential.clone(),
            dlp: self.dlp.clone(),
        }
    }

    /// True when this rule's enforcement depends on the nimbus-proxy PEP —
    /// credential injection or L7 DLP. Substrates that do not route through the
    /// PEP (the isolate `fetch` gateway) must fail closed for such rules rather
    /// than egress without those controls. (audit H4.)
    fn requires_proxy_enforcement(&self) -> bool {
        self.credential.is_some() || !self.dlp.is_empty()
    }

    fn requires_connect_interception(&self) -> bool {
        !self.methods.is_empty()
            || !self.path_prefixes.is_empty()
            || self.requires_proxy_enforcement()
    }

    fn matches_l4(&self, request: &EgressRequest, canonical_request_host: &str) -> bool {
        self.protocol == request.protocol
            && self.port == request.port
            && self.host == canonical_request_host
    }

    fn matches_l7(&self, request: &EgressRequest) -> bool {
        if matches!(self.protocol, EgressProtocol::Tcp) {
            return true;
        }
        if !self.methods.is_empty() {
            let Some(method) = request.method.as_deref() else {
                return false;
            };
            if !self
                .methods
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(method))
            {
                return false;
            }
        }
        if !self.path_prefixes.is_empty() {
            let Some(path) = request.path.as_deref() else {
                return false;
            };
            if !self
                .path_prefixes
                .iter()
                .any(|prefix| path_matches_prefix(path, prefix))
            {
                return false;
            }
        }
        true
    }
}

/// Instruction for the PEP to inject a managed credential into matching
/// requests. This policy struct carries no secret material: `credential_ref` is
/// an opaque *handle* the PEP resolves against the secret store at egress time,
/// so the policy can be serialized, logged, and diffed without leaking secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EgressCredentialInjection {
    /// Opaque handle the PEP resolves to the real secret — never the secret
    /// itself. Must be a concrete, whitespace-free, non-empty value.
    pub credential_ref: String,
    /// HTTP header the resolved credential is written to. Must be a valid HTTP
    /// token and not a proxy-controlled header (e.g. `Host`, `Connection`).
    pub header_name: String,
    /// Optional literal prefix prepended to the resolved value (e.g. `Bearer `).
    /// Must not contain CR or LF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_prefix: Option<String>,
    /// When false (default) the PEP strips any caller-supplied value of
    /// `header_name` before injecting, so the caller cannot smuggle its own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_caller_header: bool,
}

impl EgressCredentialInjection {
    pub fn new(credential_ref: impl Into<String>, header_name: impl Into<String>) -> Self {
        Self {
            credential_ref: credential_ref.into(),
            header_name: header_name.into(),
            value_prefix: None,
            allow_caller_header: false,
        }
    }

    pub fn with_value_prefix(mut self, value_prefix: impl Into<String>) -> Self {
        self.value_prefix = Some(value_prefix.into());
        self
    }

    pub fn allow_caller_header(mut self, allow_caller_header: bool) -> Self {
        self.allow_caller_header = allow_caller_header;
        self
    }

    fn validate(&self, rule_name: &str) -> std::result::Result<(), String> {
        if self.credential_ref.trim().is_empty()
            || self.credential_ref != self.credential_ref.trim()
            || self.credential_ref.contains(char::is_whitespace)
        {
            return Err(format!(
                "sandbox egress rule `{rule_name}` credential_ref must be a concrete non-empty handle"
            ));
        }
        validate_http_header_name(&self.header_name, rule_name)?;
        if let Some(prefix) = &self.value_prefix
            && (prefix.contains('\r') || prefix.contains('\n'))
        {
            return Err(format!(
                "sandbox egress rule `{rule_name}` credential value_prefix must not contain CR/LF"
            ));
        }
        Ok(())
    }
}

/// A data-loss-prevention rule the PEP applies to a matching request body.
/// Keeping the PDP pure means there is no regex engine here: `pattern` is
/// matched by the PEP as a **literal substring**, not a regular expression, so
/// this crate carries no regex dependency. (audit L9.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EgressDlpRule {
    /// Unique, concrete rule name (no whitespace, not `*`); used in diagnostics.
    pub name: String,
    /// Non-empty literal substring the PEP scans the body for — not a regex.
    pub pattern: String,
    /// Upper bound on how many leading bytes of the body the PEP inspects; must
    /// be greater than 0.
    #[serde(default = "default_dlp_max_inspection_bytes")]
    pub max_inspection_bytes: usize,
}

impl EgressDlpRule {
    pub fn new(name: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pattern: pattern.into(),
            max_inspection_bytes: default_dlp_max_inspection_bytes(),
        }
    }

    pub fn with_max_inspection_bytes(mut self, max_inspection_bytes: usize) -> Self {
        self.max_inspection_bytes = max_inspection_bytes;
        self
    }

    fn validate(&self, rule_name: &str) -> std::result::Result<(), String> {
        validate_rule_name(&self.name)?;
        if self.pattern.is_empty() {
            return Err(format!(
                "sandbox egress rule `{rule_name}` DLP rule `{}` pattern must not be empty",
                self.name
            ));
        }
        if self.max_inspection_bytes == 0 {
            return Err(format!(
                "sandbox egress rule `{rule_name}` DLP rule `{}` max_inspection_bytes must be greater than 0",
                self.name
            ));
        }
        if self.max_inspection_bytes > MAX_DLP_INSPECTION_BYTES {
            return Err(format!(
                "sandbox egress rule `{rule_name}` DLP rule `{}` max_inspection_bytes {} exceeds the PEP inspection cap {MAX_DLP_INSPECTION_BYTES}",
                self.name, self.max_inspection_bytes
            ));
        }
        Ok(())
    }
}

/// Hard upper bound on `max_inspection_bytes`, enforced at policy validation
/// and again by the PEP at request time. DLP inspection is the only place the
/// proxy whole-body-buffers, and policies are tenant-supplied, so the buffer
/// budget must be capped by the platform, not by policy content alone.
pub const MAX_DLP_INSPECTION_BYTES: usize = 1024 * 1024;

fn default_dlp_max_inspection_bytes() -> usize {
    64 * 1024
}

/// A single egress attempt presented to the PDP for an authorization decision.
///
/// The internal-IP / SSRF gate inspects both the literal `host` and the
/// `resolved_ip`. When the rule targets a DNS name, the host string alone does
/// not reveal that the name resolves to an internal address, so **the PEP MUST
/// populate `resolved_ip` with the address it is about to connect to before
/// calling [`EgressPolicy::authorize`]**. Leaving it `None` for a DNS-name rule
/// silently skips the internal-IP gate — a fail-open shape that lets a
/// DNS-rebinding / metadata-endpoint target through. This struct is not
/// `Deserialize`: it is constructed by the PEP from the live connection, never
/// from untrusted policy input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressRequest {
    /// Protocol of the attempt. An observable runtime gateway keeps WebSocket
    /// requests distinct from ordinary HTTP. Supervisor-proxy requests use
    /// `http` or `https` authority semantics because an opaque CONNECT tunnel
    /// does not reveal its application protocol.
    pub protocol: EgressProtocol,
    /// Destination host as requested by the caller (DNS name or IP literal).
    pub host: String,
    /// Destination port.
    pub port: u16,
    /// HTTP method, when known; required to satisfy a rule's `methods`.
    pub method: Option<String>,
    /// Request path, when known; required to satisfy a rule's `path_prefixes`.
    pub path: Option<String>,
    /// The address the PEP is about to connect to. MUST be set for DNS-name
    /// targets or the internal-IP gate is skipped (fail-open).
    pub resolved_ip: Option<IpAddr>,
}

impl EgressRequest {
    pub fn new(protocol: EgressProtocol, host: impl Into<String>, port: u16) -> Self {
        Self {
            protocol,
            host: host.into(),
            port,
            method: None,
            path: None,
            resolved_ip: None,
        }
    }

    pub fn with_http(mut self, method: impl Into<String>, path: impl Into<String>) -> Self {
        self.method = Some(method.into());
        self.path = Some(path.into());
        self
    }

    pub fn with_resolved_ip(mut self, resolved_ip: IpAddr) -> Self {
        self.resolved_ip = Some(resolved_ip);
        self
    }

    fn targets_internal_address(&self) -> bool {
        let host = self
            .canonical_host_result()
            .unwrap_or_else(|_| self.host.to_ascii_lowercase());
        host.parse::<IpAddr>()
            .is_ok_and(is_non_global_or_internal_ip)
            || self.resolved_ip.is_some_and(is_non_global_or_internal_ip)
            || is_internal_hostname(&host)
    }

    fn canonical_host_result(&self) -> std::result::Result<String, HostAuthorityError> {
        canonicalize_authority_host(&self.host)
    }

    fn target_summary(&self) -> String {
        let protocol = egress_protocol_label(self.protocol);
        format!("{protocol}://{}:{}", self.host, self.port)
    }
}

const fn egress_protocol_label(protocol: EgressProtocol) -> &'static str {
    match protocol {
        EgressProtocol::Tcp => "tcp",
        EgressProtocol::Http => "http",
        EgressProtocol::Https => "https",
        EgressProtocol::Ws => "ws",
        EgressProtocol::Wss => "wss",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressAuthorization {
    allowed: bool,
    matched_rule: Option<String>,
    requires_proxy_enforcement: bool,
    reason: String,
}

impl EgressAuthorization {
    fn allow(rule_name: String, requires_proxy_enforcement: bool) -> Self {
        Self {
            allowed: true,
            reason: format!("sandbox egress allowed by rule `{rule_name}`"),
            matched_rule: Some(rule_name),
            requires_proxy_enforcement,
        }
    }

    pub(crate) fn deny(reason: String) -> Self {
        Self {
            allowed: false,
            matched_rule: None,
            requires_proxy_enforcement: false,
            reason,
        }
    }

    pub fn is_allowed(&self) -> bool {
        self.allowed
    }

    pub fn matched_rule(&self) -> Option<&str> {
        self.matched_rule.as_deref()
    }

    /// True when the matched rule's enforcement depends on the nimbus-proxy PEP
    /// (credential injection or L7 DLP); always false for a deny. A substrate
    /// that does not route through the PEP must fail closed when this is set.
    pub fn requires_proxy_enforcement(&self) -> bool {
        self.requires_proxy_enforcement
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }
}

fn validate_rule_name(name: &str) -> std::result::Result<(), String> {
    if name.trim().is_empty() || name != name.trim() || name == "*" {
        return Err("sandbox egress rule names must be concrete non-empty values".to_owned());
    }
    if name.contains(char::is_whitespace) {
        return Err(format!(
            "sandbox egress rule name `{name}` must not contain whitespace"
        ));
    }
    Ok(())
}

fn validate_egress_host(host: &str, allow_internal_ips: bool) -> std::result::Result<(), String> {
    if host == "*" || host.contains('*') {
        return Err(format!(
            "sandbox egress host `{host}` must be a concrete host without wildcards"
        ));
    }
    let canonical = canonicalize_authority_host(host).map_err(|error| {
        format!(
            "sandbox egress host `{host}` must be a strict bare DNS name or IP literal: {error}"
        )
    })?;
    if let Ok(ip) = canonical.parse::<IpAddr>() {
        if is_non_global_or_internal_ip(ip) && !allow_internal_ips {
            return Err(format!(
                "sandbox egress host `{host}` is internal/non-global and requires allow_internal_ips=true"
            ));
        }
        return Ok(());
    }
    if is_internal_hostname(&canonical) && !allow_internal_ips {
        return Err(format!(
            "sandbox egress host `{host}` is internal/non-global and requires allow_internal_ips=true"
        ));
    }
    Ok(())
}

fn validate_http_method(method: &str, rule_name: &str) -> std::result::Result<(), String> {
    if method.is_empty()
        || !method.bytes().all(|byte| {
            byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err(format!(
            "sandbox egress rule `{rule_name}` HTTP method `{method}` must be an uppercase token"
        ));
    }
    Ok(())
}

fn validate_http_header_name(
    header_name: &str,
    rule_name: &str,
) -> std::result::Result<(), String> {
    if header_name.is_empty()
        || !header_name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
    {
        return Err(format!(
            "sandbox egress rule `{rule_name}` credential header_name `{header_name}` must be an HTTP token"
        ));
    }
    if matches!(
        header_name.to_ascii_lowercase().as_str(),
        "host" | "content-length" | "connection" | "proxy-connection" | "transfer-encoding"
    ) {
        return Err(format!(
            "sandbox egress rule `{rule_name}` credential header_name `{header_name}` is controlled by the proxy"
        ));
    }
    Ok(())
}

/// Segment-aware path-prefix match: a request path matches a rule prefix only
/// when the prefix is either an exact match or a true path-segment ancestor.
/// A bare `starts_with` over-authorizes — prefix `/v1` would wrongly admit
/// `/v1beta/secret`. The match succeeds only when the remainder after the
/// prefix is empty, the prefix already ends in `/`, or the remainder starts a
/// new segment with `/`. (audit M4.)
fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    if !path.starts_with(prefix) {
        return false;
    }
    let rest = &path[prefix.len()..];
    rest.is_empty() || prefix.ends_with('/') || rest.starts_with('/')
}

fn validate_path_prefix(path_prefix: &str, rule_name: &str) -> std::result::Result<(), String> {
    if !path_prefix.starts_with('/') || path_prefix.contains(char::is_whitespace) {
        return Err(format!(
            "sandbox egress rule `{rule_name}` path prefix `{path_prefix}` must start with / and contain no whitespace"
        ));
    }
    Ok(())
}

fn is_internal_hostname(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    matches!(host.as_str(), "localhost" | "localhost.localdomain")
        || host.ends_with(".localhost")
        || host.ends_with(".localhost.localdomain")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAuthorityError {
    Empty,
    ControlOrWhitespace,
    EncodedOrAmbiguousDelimiter,
    BracketOrPort,
    NonCanonicalNumericIp,
    InvalidDnsName,
}

impl Display for HostAuthorityError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("host authority is empty"),
            Self::ControlOrWhitespace => {
                f.write_str("host authority contains null/control or whitespace characters")
            }
            Self::EncodedOrAmbiguousDelimiter => f.write_str(
                "host authority contains userinfo, percent-encoding, or path delimiters",
            ),
            Self::BracketOrPort => f.write_str("host authority must not include brackets or ports"),
            Self::NonCanonicalNumericIp => {
                f.write_str("host authority is a non-canonical numeric IP form")
            }
            Self::InvalidDnsName => f.write_str("host authority is not a valid DNS hostname"),
        }
    }
}

impl std::error::Error for HostAuthorityError {}

pub fn canonicalize_authority_host(host: &str) -> std::result::Result<String, HostAuthorityError> {
    let trimmed = host.trim();
    if trimmed.is_empty() || trimmed == "." {
        return Err(HostAuthorityError::Empty);
    }
    if trimmed != host || trimmed.chars().any(char::is_control) {
        return Err(HostAuthorityError::ControlOrWhitespace);
    }
    if trimmed.contains('%')
        || trimmed.contains('@')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("://")
    {
        return Err(HostAuthorityError::EncodedOrAmbiguousDelimiter);
    }
    if let Ok(ip) = trimmed.parse::<IpAddr>() {
        return Ok(ip.to_string());
    }
    if trimmed.starts_with('[') || trimmed.ends_with(']') || trimmed.contains(':') {
        return Err(HostAuthorityError::BracketOrPort);
    }
    if looks_like_noncanonical_ipv4(trimmed) {
        return Err(HostAuthorityError::NonCanonicalNumericIp);
    }
    let without_trailing_dot = trimmed.strip_suffix('.').unwrap_or(trimmed);
    if without_trailing_dot.ends_with('.') {
        return Err(HostAuthorityError::InvalidDnsName);
    }
    let canonical = without_trailing_dot.to_ascii_lowercase();
    if !is_valid_dns_hostname(&canonical) {
        return Err(HostAuthorityError::InvalidDnsName);
    }
    Ok(canonical)
}

fn looks_like_noncanonical_ipv4(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    if lower.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if let Some(rest) = lower.strip_prefix("0x")
        && !rest.is_empty()
        && rest.chars().all(|c| c.is_ascii_hexdigit())
    {
        return true;
    }
    if !lower.contains('.') {
        return false;
    }
    lower.split('.').all(|label| {
        !label.is_empty()
            && (label.chars().all(|c| c.is_ascii_digit())
                || label.strip_prefix("0x").is_some_and(|rest| {
                    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit())
                }))
    })
}

fn is_non_global_or_internal_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_non_global_or_internal_ipv4(ip),
        IpAddr::V6(ip) => is_non_global_or_internal_ipv6(ip),
    }
}

fn is_non_global_or_internal_ipv4(ip: Ipv4Addr) -> bool {
    let [first, second, third, _] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || first == 0
        || first == 100 && (64..=127).contains(&second)
        || first == 169 && second == 254
        || first == 192 && second == 0 && third == 0
        || first == 192 && second == 0 && third == 2
        || first == 198 && matches!(second, 18 | 19)
        || first == 198 && second == 51 && third == 100
        || first == 203 && second == 0 && third == 113
        || first >= 240
}

fn is_non_global_or_internal_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(embedded_ipv4) = embedded_ipv4_for_ipv6(ip) {
        return is_non_global_or_internal_ipv4(embedded_ipv4);
    }
    let segments = ip.segments();
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
        || segments[0] == 0x2001 && segments[1] == 0x0db8
}

fn embedded_ipv4_for_ipv6(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(ipv4_mapped) = ip.to_ipv4_mapped() {
        return Some(ipv4_mapped);
    }

    let segments = ip.segments();
    if segments[..6] == [0x0064, 0xff9b, 0, 0, 0, 0] || segments[..6] == [0, 0, 0, 0, 0, 0] {
        return Some(ipv4_from_ipv6_tail(segments[6], segments[7]));
    }
    // 6to4 (2002::/16): the embedded IPv4 lives in segments 1 and 2, e.g.
    // `2002:a9fe:a9fe::` carries 169.254.169.254. Without decoding it the
    // classifier would treat a 6to4-wrapped internal target as global and
    // skip the internal-IP gate. (audit L8/M19.)
    if segments[0] == 0x2002 {
        return Some(ipv4_from_ipv6_tail(segments[1], segments[2]));
    }
    None
}

fn ipv4_from_ipv6_tail(high: u16, low: u16) -> Ipv4Addr {
    Ipv4Addr::new((high >> 8) as u8, high as u8, (low >> 8) as u8, low as u8)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::{embedded_ipv4_for_ipv6, is_non_global_or_internal_ip};
    use crate::*;

    #[test]
    fn sandbox_egress_policy_denies_by_default() {
        let policy = EgressPolicy::deny_all();
        let request = EgressRequest::new(EgressProtocol::Https, "api.stripe.com", 443);

        let authorization = policy.authorize(&request);

        assert!(!authorization.is_allowed());
        assert!(
            authorization.reason().contains("default deny"),
            "default-deny reason should be explicit: {:?}",
            authorization
        );
    }

    /// Regression pin for the July-21 review HIGH finding (fixed in #231): a
    /// CONNECT authority request carries no method or path, so a rule with
    /// L7 predicates must (a) allow the CONNECT gate with those predicates
    /// deferred, (b) force TLS interception so the deferred predicates are
    /// enforced on the decrypted inner request, and (c) still enforce them
    /// there. Without (b), relaxing the gate would splice an opaque tunnel
    /// and never enforce L7 at all.
    #[test]
    fn connect_gate_defers_method_path_and_forces_interception() {
        let policy = EgressPolicy::new([EgressRule::new(
            "stripe",
            EgressProtocol::Https,
            "api.stripe.com",
            443,
        )
        .with_methods(["POST"])
        .with_path_prefixes(["/v1/"])]);
        let compiled = policy.compile().expect("policy should compile");
        // The proxy's CONNECT parse produces an authority-only request: no
        // method, no path (`crates/nimbus-proxy/src/request.rs`).
        let connect = EgressRequest::new(EgressProtocol::Https, "api.stripe.com", 443);

        let full = compiled.authorize(&connect);
        assert!(
            !full.is_allowed(),
            "full L7 authorization must not pass without method/path: {full:?}"
        );

        let gate = compiled.authorize_connect(&connect);
        assert!(
            gate.is_allowed(),
            "CONNECT gate must defer method/path instead of denying: {gate:?}"
        );
        assert_eq!(gate.matched_rule(), Some("stripe"));

        assert!(
            compiled.connect_requires_interception(&connect),
            "a rule carrying deferred L7 predicates must force interception"
        );

        let inner_allowed = compiled.authorize(
            &EgressRequest::new(EgressProtocol::Https, "api.stripe.com", 443)
                .with_http("POST", "/v1/charges"),
        );
        assert!(inner_allowed.is_allowed(), "{inner_allowed:?}");

        let inner_denied = compiled.authorize(
            &EgressRequest::new(EgressProtocol::Https, "api.stripe.com", 443)
                .with_http("GET", "/other"),
        );
        assert!(
            !inner_denied.is_allowed(),
            "deferred predicates must still bind the decrypted inner request"
        );
    }

    /// A rule with no L7 predicates, credential, or DLP needs no interception:
    /// the CONNECT gate is the complete check and the tunnel may splice.
    #[test]
    fn connect_gate_without_l7_predicates_splices() {
        let policy = EgressPolicy::new([EgressRule::new(
            "plain",
            EgressProtocol::Https,
            "api.example.com",
            443,
        )]);
        let compiled = policy.compile().expect("policy should compile");
        let connect = EgressRequest::new(EgressProtocol::Https, "api.example.com", 443);

        assert!(compiled.authorize_connect(&connect).is_allowed());
        assert!(
            !compiled.connect_requires_interception(&connect),
            "a predicate-free rule must not force interception"
        );
    }

    #[test]
    fn sandbox_egress_policy_allows_matching_http_method_and_path() {
        let policy = EgressPolicy::new([EgressRule::new(
            "stripe",
            EgressProtocol::Https,
            "API.Stripe.COM",
            443,
        )
        .with_methods(["POST", "POST"])
        .with_path_prefixes(["/v1/", "/v1/"])]);
        let compiled = policy.compile().expect("policy should compile");
        assert_eq!(compiled.policy().rules()[0].host, "api.stripe.com");
        assert_eq!(compiled.policy().rules()[0].methods, vec!["POST"]);

        let allowed = compiled.authorize(
            &EgressRequest::new(EgressProtocol::Https, "api.stripe.com", 443)
                .with_http("POST", "/v1/charges"),
        );
        assert!(allowed.is_allowed(), "{allowed:?}");
        assert_eq!(allowed.matched_rule(), Some("stripe"));

        let denied = compiled.authorize(
            &EgressRequest::new(EgressProtocol::Https, "api.stripe.com", 443)
                .with_http("GET", "/v1/charges"),
        );
        assert!(!denied.is_allowed());
        assert!(
            denied.reason().contains("method/path"),
            "L7 denial should be named: {denied:?}"
        );
    }

    #[test]
    fn websocket_egress_requires_an_explicit_websocket_rule() {
        let http_policy = EgressPolicy::new([EgressRule::new(
            "api-http",
            EgressProtocol::Https,
            "api.example.com",
            443,
        )]);
        let websocket_request = EgressRequest::new(EgressProtocol::Wss, "api.example.com", 443)
            .with_http("GET", "/events");

        let denied = http_policy.authorize(&websocket_request);
        assert!(!denied.is_allowed());
        assert!(
            denied.reason().contains("wss://api.example.com:443"),
            "audit reason must identify the WebSocket protocol: {denied:?}"
        );

        let websocket_policy = EgressPolicy::new([EgressRule::new(
            "api-websocket",
            EgressProtocol::Wss,
            "api.example.com",
            443,
        )
        .with_methods(["GET"])
        .with_path_prefixes(["/events"])]);
        let allowed = websocket_policy.authorize(&websocket_request);
        assert!(allowed.is_allowed(), "{allowed:?}");
        assert_eq!(allowed.matched_rule(), Some("api-websocket"));

        for denied_request in [
            EgressRequest::new(EgressProtocol::Wss, "api.example.com", 443)
                .with_http("POST", "/events"),
            EgressRequest::new(EgressProtocol::Wss, "api.example.com", 443)
                .with_http("GET", "/admin"),
        ] {
            let denied = websocket_policy.authorize(&denied_request);
            assert!(
                !denied.is_allowed(),
                "WebSocket method and path constraints must both bind: {denied_request:?}"
            );
            assert!(denied.reason().contains("method/path"), "{denied:?}");
        }

        let ordinary_http = EgressRequest::new(EgressProtocol::Https, "api.example.com", 443)
            .with_http("GET", "/events");
        assert!(
            !websocket_policy.authorize(&ordinary_http).is_allowed(),
            "a WebSocket-only rule must not authorize ordinary HTTP"
        );
    }

    #[test]
    fn supervisor_proxy_rejects_runtime_only_websocket_protocol_rules() {
        for protocol in [EgressProtocol::Ws, EgressProtocol::Wss] {
            let protocol_label = if matches!(protocol, EgressProtocol::Ws) {
                "ws"
            } else {
                "wss"
            };
            let policy = EgressPolicy::new([EgressRule::new(
                "runtime-websocket",
                protocol,
                "events.example.com",
                if matches!(protocol, EgressProtocol::Ws) {
                    80
                } else {
                    443
                },
            )]);

            let error = policy
                .compile_for_supervisor_proxy()
                .expect_err("the proxy must reject an application protocol it cannot observe");
            assert!(error.contains("observable runtime gateway"), "{error}");
            assert!(error.contains(protocol_label), "{error}");
        }
    }

    #[test]
    fn sandbox_egress_policy_and_request_authority_canonicalization_agree() {
        let policy = EgressPolicy::new([EgressRule::new(
            "api",
            EgressProtocol::Https,
            "API.Example.COM.",
            443,
        )]);
        let compiled = policy.compile().expect("policy should compile");
        assert_eq!(compiled.policy().rules()[0].host, "api.example.com");

        for host in ["api.example.com", "API.EXAMPLE.COM", "api.example.com."] {
            let allowed = compiled.authorize(
                &EgressRequest::new(EgressProtocol::Https, host, 443).with_http("POST", "/v1"),
            );
            assert!(
                allowed.is_allowed(),
                "case and one trailing-dot normalization should be explicit for {host:?}: {allowed:?}"
            );
        }
    }

    #[test]
    fn sandbox_egress_policy_denies_malformed_request_authority() {
        let compiled = EgressPolicy::new([EgressRule::new(
            "api",
            EgressProtocol::Http,
            "api.example.com",
            80,
        )])
        .compile()
        .expect("policy should compile");

        for host in [
            "api.example.com%2e.evil",
            "api.example.com\0.evil",
            "api.example.com\r.evil",
            "user@api.example.com",
            "2130706433",
            "0x7f000001",
            "0177.0.0.1",
        ] {
            let denied = compiled.authorize(&EgressRequest::new(EgressProtocol::Http, host, 80));
            assert!(
                !denied.is_allowed() && denied.reason().contains("host authority rejected"),
                "malformed request host authority {host:?} must deny in the PDP before any allow: {denied:?}"
            );
        }
    }

    #[test]
    fn sandbox_egress_enforcement_plan_fails_closed_for_invalid_raw_policy() {
        let plan = EgressEnforcementPlan {
            schema_version: EGRESS_ENFORCEMENT_SCHEMA_VERSION,
            mode: EgressEnforcementMode::SupervisorProxy,
            reload_policy: EgressReloadPolicy::RecreateRequired,
            policy: EgressPolicy::new([EgressRule::new(
                "wildcard",
                EgressProtocol::Https,
                "*",
                443,
            )]),
        };

        let error = plan
            .validate()
            .expect_err("invalid policy should be rejected");
        assert!(
            error.contains("wildcards"),
            "invalid egress contract should expose the policy error: {error}"
        );
    }

    #[test]
    fn sandbox_egress_enforcement_plan_rejects_removed_launch_metadata_mode() {
        let error = serde_json::from_value::<EgressEnforcementPlan>(serde_json::json!({
            "schema_version": EGRESS_ENFORCEMENT_SCHEMA_VERSION,
            "mode": "launch_metadata",
            "reload_policy": "recreate_required",
            "policy": { "rules": [] },
        }))
        .expect_err("removed launch-metadata mode must not deserialize");
        assert!(
            error
                .to_string()
                .contains("unknown variant `launch_metadata`")
        );
    }

    #[test]
    fn sandbox_egress_enforcement_plan_models_future_supervisor_live_reload() {
        let plan = EgressEnforcementPlan::supervisor_proxy(
            &EgressPolicy::new([EgressRule::new(
                "github",
                EgressProtocol::Https,
                "api.github.com",
                443,
            )])
            .compile()
            .expect("allow policy should compile"),
            EgressReloadPolicy::LiveReload,
        );

        assert_eq!(plan.mode, EgressEnforcementMode::SupervisorProxy);
        assert_eq!(plan.reload_policy, EgressReloadPolicy::LiveReload);
        assert_eq!(plan.policy().rules()[0].name, "github");
        plan.validate()
            .expect("supervisor proxy contract should validate");
    }

    #[test]
    fn sandbox_egress_enforcement_plan_allows_supervisor_recreate_lifecycle() {
        let plan = EgressEnforcementPlan::supervisor_proxy(
            &EgressPolicy::deny_all()
                .compile()
                .expect("deny-all should compile"),
            EgressReloadPolicy::RecreateRequired,
        );

        assert_eq!(plan.mode, EgressEnforcementMode::SupervisorProxy);
        assert_eq!(plan.reload_policy, EgressReloadPolicy::RecreateRequired);
        plan.validate()
            .expect("supervisor proxy can start before live reload exists");
    }

    #[test]
    fn sandbox_egress_policy_checks_all_l4_matching_rules_before_denying() {
        let policy = EgressPolicy::new([
            EgressRule::new("stripe-read", EgressProtocol::Https, "api.stripe.com", 443)
                .with_methods(["GET"])
                .with_path_prefixes(["/v1/customers"]),
            EgressRule::new("stripe-write", EgressProtocol::Https, "api.stripe.com", 443)
                .with_methods(["POST"])
                .with_path_prefixes(["/v1/charges"]),
        ]);
        policy.validate().expect("policy should validate");

        let authorization = policy.authorize(
            &EgressRequest::new(EgressProtocol::Https, "api.stripe.com", 443)
                .with_http("POST", "/v1/charges"),
        );

        assert!(authorization.is_allowed(), "{authorization:?}");
        assert_eq!(authorization.matched_rule(), Some("stripe-write"));
    }

    #[test]
    fn sandbox_egress_policy_denies_resolved_internal_ip_without_explicit_allow() {
        let policy = EgressPolicy::new([EgressRule::new(
            "metadata-lookalike",
            EgressProtocol::Http,
            "metadata.example.com",
            80,
        )]);
        policy.validate().expect("policy should validate");

        let denied = policy.authorize(
            &EgressRequest::new(EgressProtocol::Http, "metadata.example.com", 80)
                .with_resolved_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))),
        );
        assert!(!denied.is_allowed());
        assert!(
            denied.reason().contains("internal/non-global"),
            "SSRF/internal denial should be named: {denied:?}"
        );

        let allowed_policy = EgressPolicy::new([EgressRule::new(
            "metadata",
            EgressProtocol::Http,
            "169.254.169.254",
            80,
        )
        .allow_internal_ips(true)]);
        allowed_policy
            .validate()
            .expect("explicit internal allowlist should validate");
        let allowed = allowed_policy.authorize(&EgressRequest::new(
            EgressProtocol::Http,
            "169.254.169.254",
            80,
        ));
        assert!(allowed.is_allowed(), "{allowed:?}");
    }

    #[test]
    fn sandbox_egress_hostname_precheck_ignores_resolved_ip_but_not_host_policy() {
        let compiled = EgressPolicy::new([EgressRule::new(
            "public-api",
            EgressProtocol::Http,
            "api.example.com",
            80,
        )])
        .compile()
        .expect("policy should compile");
        let request = EgressRequest::new(EgressProtocol::Http, "API.Example.COM.", 80)
            .with_resolved_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));

        let pre_dns = compiled.authorize_hostname_without_resolved_ip(&request);
        assert!(
            pre_dns.is_allowed(),
            "pre-DNS host intent should pass before the resolved-IP SSRF gate: {pre_dns:?}"
        );

        let post_dns = compiled.authorize(&request);
        assert!(
            !post_dns.is_allowed() && post_dns.reason().contains("internal/non-global"),
            "post-DNS authorization must still deny the selected internal resolved IP: {post_dns:?}"
        );

        let denied = compiled.authorize_hostname_without_resolved_ip(&EgressRequest::new(
            EgressProtocol::Http,
            "evil.example.com",
            80,
        ));
        assert!(
            !denied.is_allowed() && denied.reason().contains("default deny"),
            "hostnames absent from policy must deny before DNS: {denied:?}"
        );
    }

    #[test]
    fn sandbox_egress_policy_treats_reserved_and_mapped_addresses_as_internal() {
        let policy = EgressPolicy::new([EgressRule::new(
            "reserved-lookalike",
            EgressProtocol::Http,
            "reserved.example.com",
            80,
        )]);
        policy.validate().expect("policy should validate");

        for resolved_ip in [
            IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1)),
            IpAddr::V4(Ipv4Addr::new(198, 18, 0, 1)),
            IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0xa9fe, 0xa9fe)),
            IpAddr::V6(
                "64:ff9b::169.254.169.254"
                    .parse()
                    .expect("NAT64 IPv6 address should parse"),
            ),
            IpAddr::V6(
                "::169.254.169.254"
                    .parse()
                    .expect("IPv4-compatible IPv6 address should parse"),
            ),
        ] {
            let denied = policy.authorize(
                &EgressRequest::new(EgressProtocol::Http, "reserved.example.com", 80)
                    .with_resolved_ip(resolved_ip),
            );
            assert!(
                !denied.is_allowed(),
                "reserved/internal address {resolved_ip} should be denied: {denied:?}"
            );
        }

        let allowed = policy.authorize(
            &EgressRequest::new(EgressProtocol::Http, "reserved.example.com", 80).with_resolved_ip(
                IpAddr::V6(
                    "64:ff9b::93.184.216.34"
                        .parse()
                        .expect("global NAT64 IPv6 address should parse"),
                ),
            ),
        );
        assert!(
            allowed.is_allowed(),
            "global NAT64 address should remain allowed when the rule matches: {allowed:?}"
        );
    }

    #[test]
    fn sandbox_egress_policy_rejects_wildcards_and_tcp_l7_fields() {
        let wildcard = EgressPolicy::new([EgressRule::new(
            "all",
            EgressProtocol::Https,
            "*.example.com",
            443,
        )]);
        let error = wildcard
            .validate()
            .expect_err("wildcard host should be rejected");
        assert!(
            error.contains("wildcards"),
            "wildcard error should be explicit: {error}"
        );

        let tcp_with_method = EgressPolicy::new([EgressRule::new(
            "postgres",
            EgressProtocol::Tcp,
            "db.example.com",
            5432,
        )
        .with_methods(["GET"])]);
        let error = tcp_with_method
            .validate()
            .expect_err("tcp L7 policy should be rejected");
        assert!(
            error.contains("tcp") && error.contains("HTTP methods"),
            "tcp L7 error should be explicit: {error}"
        );
    }

    #[test]
    fn sandbox_egress_policy_rejects_malformed_host_shapes() {
        for host in [
            "https://api.stripe.com",
            "api.stripe.com/v1",
            "api.stripe.com:443",
            "api stripe com",
            "api.stripe.com%2f.evil",
            "api.stripe.com\0.evil",
            "api.stripe.com\r.evil",
            "user@api.stripe.com",
            "api.stripe.com..",
            "2130706433",
            "0x7f000001",
            "0177.0.0.1",
            "[::1]",
            "-bad.example.com",
        ] {
            let policy = EgressPolicy::new([EgressRule::new(
                "bad-host",
                EgressProtocol::Https,
                host,
                443,
            )]);
            let error = policy
                .validate()
                .expect_err("malformed egress host should be rejected");
            assert!(
                error.contains("host"),
                "host validation error should name host shape for {host:?}: {error}"
            );
        }
    }

    #[test]
    fn sandbox_egress_path_prefix_matches_on_segment_boundaries() {
        let policy = EgressPolicy::new([EgressRule::new(
            "versioned",
            EgressProtocol::Https,
            "api.example.com",
            443,
        )
        .with_methods(["GET"])
        .with_path_prefixes(["/v1"])]);
        let compiled = policy.compile().expect("policy should compile");

        // Exact match and a true sub-segment are authorized.
        for path in ["/v1", "/v1/charges"] {
            let allowed = compiled.authorize(
                &EgressRequest::new(EgressProtocol::Https, "api.example.com", 443)
                    .with_http("GET", path),
            );
            assert!(
                allowed.is_allowed(),
                "path {path:?} should match /v1: {allowed:?}"
            );
        }

        // A sibling path that merely shares the textual prefix must be denied;
        // deleting the segment-boundary check in `path_matches_prefix` would
        // wrongly allow this via `starts_with`.
        let denied = compiled.authorize(
            &EgressRequest::new(EgressProtocol::Https, "api.example.com", 443)
                .with_http("GET", "/v1beta/secret"),
        );
        assert!(
            !denied.is_allowed(),
            "/v1beta must not be admitted by prefix /v1: {denied:?}"
        );
        assert!(
            denied.reason().contains("method/path"),
            "L7 path denial should be named: {denied:?}"
        );
    }

    #[test]
    fn sandbox_egress_policy_denies_internal_ipv6_resolved_forms() {
        let policy = EgressPolicy::new([EgressRule::new(
            "ipv6-lookalike",
            EgressProtocol::Http,
            "ipv6.example.com",
            80,
        )]);
        policy.validate().expect("policy should validate");

        for raw in [
            "::1",              // loopback
            "fe80::1",          // link-local
            "fc00::1",          // unique-local
            "2001:db8::1",      // documentation
            "2002:a9fe:a9fe::", // 6to4-wrapped 169.254.169.254 (audit L8/M19)
        ] {
            let resolved_ip: IpAddr = raw.parse().expect("IPv6 literal should parse");
            let denied = policy.authorize(
                &EgressRequest::new(EgressProtocol::Http, "ipv6.example.com", 80)
                    .with_resolved_ip(resolved_ip),
            );
            assert!(
                !denied.is_allowed(),
                "internal IPv6 {raw} must be denied without allow_internal_ips: {denied:?}"
            );
            assert!(
                denied.reason().contains("internal/non-global"),
                "IPv6 SSRF denial should be named for {raw}: {denied:?}"
            );
        }
    }

    #[test]
    fn sandbox_egress_6to4_decodes_embedded_ipv4_directly() {
        // Guard the 6to4 decode at the classifier level so removing the
        // `segments[0] == 0x2002` branch fails here, not only end-to-end.
        let embedded = embedded_ipv4_for_ipv6(
            "2002:a9fe:a9fe::"
                .parse::<Ipv6Addr>()
                .expect("6to4 literal should parse"),
        );
        assert_eq!(embedded, Some(Ipv4Addr::new(169, 254, 169, 254)));
        assert!(is_non_global_or_internal_ip(IpAddr::V6(
            "2002:a9fe:a9fe::"
                .parse()
                .expect("6to4 literal should parse")
        )));
    }

    #[test]
    fn credential_injection_validate_rejects_empty_or_whitespace_ref() {
        for bad_ref in ["", "   ", "vault has space"] {
            let policy = EgressPolicy::new([EgressRule::new(
                "cred",
                EgressProtocol::Https,
                "api.example.com",
                443,
            )
            .with_credential_injection(EgressCredentialInjection::new(bad_ref, "Authorization"))]);
            let error = policy
                .validate()
                .expect_err("non-concrete credential_ref must be rejected");
            assert!(
                error.contains("credential_ref must be a concrete non-empty handle"),
                "credential_ref error should be named for {bad_ref:?}: {error}"
            );
        }
    }

    #[test]
    fn credential_injection_validate_rejects_proxy_controlled_header() {
        let policy = EgressPolicy::new([EgressRule::new(
            "cred",
            EgressProtocol::Https,
            "api.example.com",
            443,
        )
        .with_credential_injection(EgressCredentialInjection::new("vault://token", "Host"))]);
        let error = policy
            .validate()
            .expect_err("proxy-controlled header must be rejected");
        assert!(
            error.contains("is controlled by the proxy"),
            "blocklisted header error should be named: {error}"
        );
    }

    #[test]
    fn credential_injection_validate_rejects_crlf_value_prefix() {
        for bad_prefix in ["Bearer \r", "Bearer \n", "Bearer \r\nX-Inject: 1"] {
            let policy = EgressPolicy::new([EgressRule::new(
                "cred",
                EgressProtocol::Https,
                "api.example.com",
                443,
            )
            .with_credential_injection(
                EgressCredentialInjection::new("vault://token", "Authorization")
                    .with_value_prefix(bad_prefix),
            )]);
            let error = policy
                .validate()
                .expect_err("CR/LF in value_prefix must be rejected");
            assert!(
                error.contains("value_prefix must not contain CR/LF"),
                "header-injection guard should be named for {bad_prefix:?}: {error}"
            );
        }
    }

    #[test]
    fn websocket_rules_reject_proxy_only_credential_and_dlp_enforcement() {
        let policies = [
            EgressPolicy::new([EgressRule::new(
                "websocket-credential",
                EgressProtocol::Wss,
                "events.example.com",
                443,
            )
            .with_credential_injection(EgressCredentialInjection::new(
                "vault://token",
                "Authorization",
            ))]),
            EgressPolicy::new([EgressRule::new(
                "websocket-dlp",
                EgressProtocol::Ws,
                "events.example.com",
                80,
            )
            .with_dlp_rules([EgressDlpRule::new("secret", "sensitive")])]),
        ];

        for policy in policies {
            let error = policy
                .validate()
                .expect_err("runtime-only WebSocket rules cannot require proxy enforcement");
            assert!(
                error
                    .contains("observable runtime gateway cannot apply proxy-mediated enforcement"),
                "validation should identify the unavailable enforcement seam: {error}"
            );
        }
    }

    #[test]
    fn dlp_validate_rejects_empty_pattern() {
        let policy = EgressPolicy::new([EgressRule::new(
            "dlp-host",
            EgressProtocol::Https,
            "api.example.com",
            443,
        )
        .with_dlp_rules([EgressDlpRule::new("ssn", "")])]);
        let error = policy
            .validate()
            .expect_err("empty DLP pattern must be rejected");
        assert!(
            error.contains("pattern must not be empty"),
            "empty DLP pattern error should be named: {error}"
        );
    }

    #[test]
    fn dlp_validate_rejects_zero_inspection_bytes() {
        let policy = EgressPolicy::new([EgressRule::new(
            "dlp-host",
            EgressProtocol::Https,
            "api.example.com",
            443,
        )
        .with_dlp_rules([EgressDlpRule::new("ssn", "secret").with_max_inspection_bytes(0)])]);
        let error = policy
            .validate()
            .expect_err("zero max_inspection_bytes must be rejected");
        assert!(
            error.contains("max_inspection_bytes must be greater than 0"),
            "zero max_inspection_bytes error should be named: {error}"
        );
    }

    #[test]
    fn dlp_validate_rejects_inspection_bytes_above_pep_cap() {
        let policy = EgressPolicy::new([EgressRule::new(
            "dlp-host",
            EgressProtocol::Https,
            "api.example.com",
            443,
        )
        .with_dlp_rules([EgressDlpRule::new("ssn", "secret")
            .with_max_inspection_bytes(MAX_DLP_INSPECTION_BYTES + 1)])]);
        let error = policy
            .validate()
            .expect_err("an inspection budget above the PEP cap must be rejected");
        assert!(
            error.contains("exceeds the PEP inspection cap"),
            "over-cap DLP budget error should be named: {error}"
        );
    }
}
