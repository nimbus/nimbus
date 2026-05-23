use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use serde::{Deserialize, Serialize};

use crate::endpoint::PublishedEndpointProtocol;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxEgressPolicy {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<SandboxEgressRule>,
}

impl SandboxEgressPolicy {
    pub fn deny_all() -> Self {
        Self::default()
    }

    pub fn new(rules: impl IntoIterator<Item = SandboxEgressRule>) -> Self {
        Self {
            allow: rules.into_iter().collect(),
        }
    }

    pub fn with_rule(mut self, rule: SandboxEgressRule) -> Self {
        self.allow.push(rule);
        self
    }

    pub fn rules(&self) -> &[SandboxEgressRule] {
        &self.allow
    }

    pub fn is_deny_all(&self) -> bool {
        self.allow.is_empty()
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        let mut names = std::collections::BTreeSet::new();
        for rule in &self.allow {
            rule.validate()?;
            if !names.insert(rule.name.as_str()) {
                return Err(format!(
                    "sandbox egress rule {:?} is declared more than once",
                    rule.name
                ));
            }
        }
        Ok(())
    }

    pub fn authorize(&self, request: &SandboxEgressRequest) -> SandboxEgressAuthorization {
        if let Err(message) = self.validate() {
            return SandboxEgressAuthorization::deny(format!(
                "sandbox egress policy invalid: {message}"
            ));
        }
        let mut matched_but_denied = None;
        for rule in &self.allow {
            if !rule.matches_l4(request) {
                continue;
            }
            if request.targets_internal_address() && !rule.allow_internal_ips {
                matched_but_denied.get_or_insert_with(|| {
                    format!(
                    "sandbox egress rule `{}` matched {}, but internal/loopback targets require allow_internal_ips=true",
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
            return SandboxEgressAuthorization::allow(rule.name.clone());
        }

        if let Some(reason) = matched_but_denied {
            return SandboxEgressAuthorization::deny(reason);
        }

        SandboxEgressAuthorization::deny(format!(
            "sandbox egress default deny: no rule matched {}",
            request.target_summary()
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxEgressRule {
    pub name: String,
    pub protocol: PublishedEndpointProtocol,
    pub host: String,
    pub port: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path_prefixes: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_internal_ips: bool,
}

impl SandboxEgressRule {
    pub fn new(
        name: impl Into<String>,
        protocol: PublishedEndpointProtocol,
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

    fn validate(&self) -> std::result::Result<(), String> {
        validate_rule_name(&self.name)?;
        validate_egress_host(&self.host, self.allow_internal_ips)?;
        if self.port == 0 {
            return Err(format!(
                "sandbox egress rule `{}` port must not be 0",
                self.name
            ));
        }
        if matches!(self.protocol, PublishedEndpointProtocol::Tcp)
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
        Ok(())
    }

    fn matches_l4(&self, request: &SandboxEgressRequest) -> bool {
        self.protocol == request.protocol
            && self.port == request.port
            && self.host.eq_ignore_ascii_case(request.host.as_str())
    }

    fn matches_l7(&self, request: &SandboxEgressRequest) -> bool {
        if matches!(self.protocol, PublishedEndpointProtocol::Tcp) {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxEgressRequest {
    pub protocol: PublishedEndpointProtocol,
    pub host: String,
    pub port: u16,
    pub method: Option<String>,
    pub path: Option<String>,
    pub resolved_ip: Option<IpAddr>,
}

impl SandboxEgressRequest {
    pub fn new(protocol: PublishedEndpointProtocol, host: impl Into<String>, port: u16) -> Self {
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
            .is_ok_and(is_internal_or_loopback_ip)
            || self.resolved_ip.is_some_and(is_internal_or_loopback_ip)
            || is_internal_hostname(&self.host)
    }

    fn target_summary(&self) -> String {
        let protocol = match self.protocol {
            PublishedEndpointProtocol::Tcp => "tcp",
            PublishedEndpointProtocol::Http => "http",
            PublishedEndpointProtocol::Https => "https",
        };
        format!("{protocol}://{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxEgressAuthorization {
    allowed: bool,
    matched_rule: Option<String>,
    reason: String,
}

impl SandboxEgressAuthorization {
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
    if is_internal_hostname(host) && !allow_internal_ips {
        return Err(format!(
            "sandbox egress host `{host}` is internal/loopback and requires allow_internal_ips=true"
        ));
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && is_internal_or_loopback_ip(ip)
        && !allow_internal_ips
    {
        return Err(format!(
            "sandbox egress host `{host}` is internal/loopback and requires allow_internal_ips=true"
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

fn validate_path_prefix(path_prefix: &str, rule_name: &str) -> std::result::Result<(), String> {
    if !path_prefix.starts_with('/') || path_prefix.contains(char::is_whitespace) {
        return Err(format!(
            "sandbox egress rule `{rule_name}` path prefix `{path_prefix}` must start with / and contain no whitespace"
        ));
    }
    Ok(())
}

fn is_internal_hostname(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "localhost.localdomain"
    )
}

fn is_internal_or_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_internal_or_loopback_ipv4(ip),
        IpAddr::V6(ip) => is_internal_or_loopback_ipv6(ip),
    }
}

fn is_internal_or_loopback_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.octets()[0] == 0
        || ip.octets()[0] == 169 && ip.octets()[1] == 254
}

fn is_internal_or_loopback_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || ip.is_multicast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_egress_policy_denies_by_default() {
        let policy = SandboxEgressPolicy::deny_all();
        let request =
            SandboxEgressRequest::new(PublishedEndpointProtocol::Https, "api.stripe.com", 443);

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
        let policy = SandboxEgressPolicy::new([SandboxEgressRule::new(
            "stripe",
            PublishedEndpointProtocol::Https,
            "api.stripe.com",
            443,
        )
        .with_methods(["POST"])
        .with_path_prefixes(["/v1/"])]);
        policy.validate().expect("policy should validate");

        let allowed = policy.authorize(
            &SandboxEgressRequest::new(PublishedEndpointProtocol::Https, "api.stripe.com", 443)
                .with_http("POST", "/v1/charges"),
        );
        assert!(allowed.is_allowed(), "{allowed:?}");
        assert_eq!(allowed.matched_rule(), Some("stripe"));

        let denied = policy.authorize(
            &SandboxEgressRequest::new(PublishedEndpointProtocol::Https, "api.stripe.com", 443)
                .with_http("GET", "/v1/charges"),
        );
        assert!(!denied.is_allowed());
        assert!(
            denied.reason().contains("method/path"),
            "L7 denial should be named: {denied:?}"
        );
    }

    #[test]
    fn sandbox_egress_policy_checks_all_l4_matching_rules_before_denying() {
        let policy = SandboxEgressPolicy::new([
            SandboxEgressRule::new(
                "stripe-read",
                PublishedEndpointProtocol::Https,
                "api.stripe.com",
                443,
            )
            .with_methods(["GET"])
            .with_path_prefixes(["/v1/customers"]),
            SandboxEgressRule::new(
                "stripe-write",
                PublishedEndpointProtocol::Https,
                "api.stripe.com",
                443,
            )
            .with_methods(["POST"])
            .with_path_prefixes(["/v1/charges"]),
        ]);
        policy.validate().expect("policy should validate");

        let authorization = policy.authorize(
            &SandboxEgressRequest::new(PublishedEndpointProtocol::Https, "api.stripe.com", 443)
                .with_http("POST", "/v1/charges"),
        );

        assert!(authorization.is_allowed(), "{authorization:?}");
        assert_eq!(authorization.matched_rule(), Some("stripe-write"));
    }

    #[test]
    fn sandbox_egress_policy_denies_resolved_internal_ip_without_explicit_allow() {
        let policy = SandboxEgressPolicy::new([SandboxEgressRule::new(
            "metadata-lookalike",
            PublishedEndpointProtocol::Http,
            "metadata.example.com",
            80,
        )]);
        policy.validate().expect("policy should validate");

        let denied = policy.authorize(
            &SandboxEgressRequest::new(PublishedEndpointProtocol::Http, "metadata.example.com", 80)
                .with_resolved_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254))),
        );
        assert!(!denied.is_allowed());
        assert!(
            denied.reason().contains("internal/loopback"),
            "SSRF/internal denial should be named: {denied:?}"
        );

        let allowed_policy = SandboxEgressPolicy::new([SandboxEgressRule::new(
            "metadata",
            PublishedEndpointProtocol::Http,
            "169.254.169.254",
            80,
        )
        .allow_internal_ips(true)]);
        allowed_policy
            .validate()
            .expect("explicit internal allowlist should validate");
        let allowed = allowed_policy.authorize(&SandboxEgressRequest::new(
            PublishedEndpointProtocol::Http,
            "169.254.169.254",
            80,
        ));
        assert!(allowed.is_allowed(), "{allowed:?}");
    }

    #[test]
    fn sandbox_egress_policy_rejects_wildcards_and_tcp_l7_fields() {
        let wildcard = SandboxEgressPolicy::new([SandboxEgressRule::new(
            "all",
            PublishedEndpointProtocol::Https,
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

        let tcp_with_method = SandboxEgressPolicy::new([SandboxEgressRule::new(
            "postgres",
            PublishedEndpointProtocol::Tcp,
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
}
