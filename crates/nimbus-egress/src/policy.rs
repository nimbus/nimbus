use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EgressProtocol {
    Tcp,
    Http,
    Https,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EgressPolicy {
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

    pub fn authorize(&self, request: &EgressRequest) -> EgressAuthorization {
        let mut matched_but_denied = None;
        for rule in &self.policy.allow {
            if !rule.matches_l4(request) {
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
            if !rule.matches_l7(request) {
                matched_but_denied.get_or_insert_with(|| {
                    format!(
                        "sandbox egress rule `{}` matched {}, but HTTP method/path policy denied the request",
                        rule.name,
                        request.target_summary()
                    )
                });
                continue;
            }
            return EgressAuthorization::allow(rule.name.clone());
        }

        if let Some(reason) = matched_but_denied {
            return EgressAuthorization::deny(reason);
        }

        EgressAuthorization::deny(format!(
            "sandbox egress default deny: no rule matched {}",
            request.target_summary()
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EgressRule {
    pub name: String,
    pub protocol: EgressProtocol,
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_prefixes: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_internal_ips: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<EgressCredentialInjection>,
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
            host: canonical_host(&self.host),
            port: self.port,
            methods,
            path_prefixes,
            allow_internal_ips: self.allow_internal_ips,
            credential: self.credential.clone(),
            dlp: self.dlp.clone(),
        }
    }

    fn matches_l4(&self, request: &EgressRequest) -> bool {
        self.protocol == request.protocol
            && self.port == request.port
            && self.host == canonical_host(&request.host)
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
                .any(|prefix| path.starts_with(prefix))
            {
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EgressCredentialInjection {
    pub credential_ref: String,
    pub header_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_prefix: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EgressDlpRule {
    pub name: String,
    pub pattern: String,
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
        Ok(())
    }
}

fn default_dlp_max_inspection_bytes() -> usize {
    64 * 1024
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressRequest {
    pub protocol: EgressProtocol,
    pub host: String,
    pub port: u16,
    pub method: Option<String>,
    pub path: Option<String>,
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
        self.host
            .parse::<IpAddr>()
            .is_ok_and(is_non_global_or_internal_ip)
            || self.resolved_ip.is_some_and(is_non_global_or_internal_ip)
            || is_internal_hostname(&self.host)
    }

    fn target_summary(&self) -> String {
        let protocol = match self.protocol {
            EgressProtocol::Tcp => "tcp",
            EgressProtocol::Http => "http",
            EgressProtocol::Https => "https",
        };
        format!("{protocol}://{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressAuthorization {
    allowed: bool,
    matched_rule: Option<String>,
    reason: String,
}

impl EgressAuthorization {
    fn allow(rule_name: String) -> Self {
        Self {
            allowed: true,
            reason: format!("sandbox egress allowed by rule `{rule_name}`"),
            matched_rule: Some(rule_name),
        }
    }

    fn deny(reason: String) -> Self {
        Self {
            allowed: false,
            matched_rule: None,
            reason,
        }
    }

    pub fn is_allowed(&self) -> bool {
        self.allowed
    }

    pub fn matched_rule(&self) -> Option<&str> {
        self.matched_rule.as_deref()
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
    if host.trim().is_empty() || host != host.trim() || host == "*" || host.contains('*') {
        return Err(format!(
            "sandbox egress host `{host}` must be a concrete host without wildcards"
        ));
    }
    if host.contains(char::is_whitespace) {
        return Err(format!(
            "sandbox egress host `{host}` must not contain whitespace"
        ));
    }
    if host.contains("://") || host.contains('/') || host.contains('\\') || host.contains('@') {
        return Err(format!(
            "sandbox egress host `{host}` must be a bare DNS name or IP literal, not a URL or authority"
        ));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_non_global_or_internal_ip(ip) && !allow_internal_ips {
            return Err(format!(
                "sandbox egress host `{host}` is internal/non-global and requires allow_internal_ips=true"
            ));
        }
        return Ok(());
    }
    if host.starts_with('[') || host.ends_with(']') || host.contains(':') {
        return Err(format!(
            "sandbox egress host `{host}` must not include brackets, schemes, or ports"
        ));
    }
    if !is_valid_dns_hostname(host) {
        return Err(format!(
            "sandbox egress host `{host}` must be a valid DNS hostname"
        ));
    }
    if is_internal_hostname(host) && !allow_internal_ips {
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

fn is_valid_dns_hostname(host: &str) -> bool {
    if host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && label
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && label
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
    })
}

fn canonical_host(host: &str) -> String {
    host.parse::<IpAddr>()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|_| host.to_ascii_lowercase())
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
    None
}

fn ipv4_from_ipv6_tail(high: u16, low: u16) -> Ipv4Addr {
    Ipv4Addr::new((high >> 8) as u8, high as u8, (low >> 8) as u8, low as u8)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

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
    fn sandbox_egress_enforcement_plan_defaults_to_launch_metadata_recreate_required() {
        let plan = EgressEnforcementPlan::launch_metadata(
            &EgressPolicy::deny_all()
                .compile()
                .expect("deny-all should compile"),
        );

        assert_eq!(plan.schema_version, EGRESS_ENFORCEMENT_SCHEMA_VERSION);
        assert_eq!(plan.mode, EgressEnforcementMode::LaunchMetadata);
        assert_eq!(plan.reload_policy, EgressReloadPolicy::RecreateRequired);
        assert!(plan.policy().is_deny_all());
        let compiled = plan.validate().expect("launch metadata should validate");
        let denied = compiled.authorize(&EgressRequest::new(
            EgressProtocol::Https,
            "api.stripe.com",
            443,
        ));
        assert!(!denied.is_allowed());
    }

    #[test]
    fn sandbox_egress_enforcement_plan_materializes_canonical_allow_rules() {
        let policy = EgressPolicy::new([EgressRule::new(
            "stripe",
            EgressProtocol::Https,
            "API.Stripe.COM",
            443,
        )
        .with_methods(["POST", "POST"])
        .with_path_prefixes(["/v1/", "/v1/"])]);

        let plan = EgressEnforcementPlan::from_launch_policy(&policy)
            .expect("allow policy should compile into launch metadata");

        assert_eq!(plan.mode, EgressEnforcementMode::LaunchMetadata);
        assert_eq!(plan.reload_policy, EgressReloadPolicy::RecreateRequired);
        assert_eq!(plan.policy().rules()[0].host, "api.stripe.com");
        assert_eq!(plan.policy().rules()[0].methods, vec!["POST"]);
        assert_eq!(plan.policy().rules()[0].path_prefixes, vec!["/v1/"]);
    }

    #[test]
    fn sandbox_egress_launch_enforcement_selects_supervisor_proxy_for_process_launches() {
        let policy = EgressPolicy::new([EgressRule::new(
            "github",
            EgressProtocol::Https,
            "API.GitHub.COM",
            443,
        )]);

        let plan = EgressLaunchEnforcement::ProcessSupervisorProxy
            .materialize(&policy)
            .expect("process launch egress policy should materialize");

        assert_eq!(plan.mode, EgressEnforcementMode::SupervisorProxy);
        assert_eq!(plan.reload_policy, EgressReloadPolicy::RecreateRequired);
        assert_eq!(plan.policy().rules()[0].host, "api.github.com");
        plan.validate()
            .expect("process launch supervisor contract should validate");
    }

    #[test]
    fn sandbox_egress_launch_enforcement_fails_closed_for_invalid_raw_policy() {
        let policy =
            EgressPolicy::new([EgressRule::new("wildcard", EgressProtocol::Https, "*", 443)]);

        let error = EgressLaunchEnforcement::ProcessSupervisorProxy
            .materialize(&policy)
            .expect_err("invalid process launch egress policy should fail closed");

        assert!(
            error.contains("wildcards"),
            "invalid egress policy should expose the policy error: {error}"
        );
    }

    #[test]
    fn sandbox_egress_enforcement_plan_fails_closed_for_invalid_raw_policy() {
        let plan = EgressEnforcementPlan {
            schema_version: EGRESS_ENFORCEMENT_SCHEMA_VERSION,
            mode: EgressEnforcementMode::LaunchMetadata,
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
    fn sandbox_egress_enforcement_plan_rejects_false_live_reload_claims() {
        let plan = EgressEnforcementPlan {
            schema_version: EGRESS_ENFORCEMENT_SCHEMA_VERSION,
            mode: EgressEnforcementMode::LaunchMetadata,
            reload_policy: EgressReloadPolicy::LiveReload,
            policy: EgressPolicy::deny_all(),
        };

        let error = plan
            .validate()
            .expect_err("launch metadata must not claim live reload");
        assert!(
            error.contains("cannot claim live reload"),
            "reload lifecycle mismatch should fail closed: {error}"
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
}
