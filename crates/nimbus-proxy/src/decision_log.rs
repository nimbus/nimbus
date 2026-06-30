use std::sync::Arc;

use nimbus_egress::EgressProtocol;

use crate::redaction::redact_query_values;
use crate::request::{ParsedProxyRequest, ProxyRequestMode};

pub type DecisionLogger = Arc<dyn Fn(EgressDecisionLog) + Send + Sync + 'static>;

pub(crate) fn noop_decision_logger() -> DecisionLogger {
    Arc::new(|_| {})
}

/// Audit record emitted for every terminal egress decision. Both allow and
/// deny verdicts are recorded so blocked or exfiltration-attempt requests are
/// never an audit blind spot. The record carries only a redacted destination,
/// the policy verdict, the human-readable reason, the matched rule name, and an
/// optional credential *reference* (never the secret material itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressDecisionLog {
    destination: String,
    credential_identity: Option<String>,
    allowed: bool,
    reason: String,
    matched_rule: Option<String>,
}

impl EgressDecisionLog {
    pub(crate) fn allowed(
        parsed: &ParsedProxyRequest,
        credential_identity: Option<String>,
        reason: String,
        matched_rule: Option<String>,
    ) -> Self {
        Self {
            destination: redacted_destination(parsed),
            credential_identity,
            allowed: true,
            reason,
            matched_rule,
        }
    }

    pub(crate) fn denied(
        parsed: &ParsedProxyRequest,
        reason: String,
        matched_rule: Option<String>,
    ) -> Self {
        Self {
            destination: redacted_destination(parsed),
            credential_identity: None,
            allowed: false,
            reason,
            matched_rule,
        }
    }

    pub fn destination(&self) -> &str {
        &self.destination
    }

    pub fn credential_identity(&self) -> Option<&str> {
        self.credential_identity.as_deref()
    }

    /// Whether the request was permitted to reach upstream. `false` records a
    /// terminal deny (no active policy, DNS overflow, PDP deny, DLP block, or
    /// credential deny).
    pub fn is_allowed(&self) -> bool {
        self.allowed
    }

    /// Human-readable reason for the verdict. For denies this mirrors the
    /// fail-closed message returned to the caller; it never contains secret
    /// material.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The name of the policy rule that produced the verdict, when one matched.
    pub fn matched_rule(&self) -> Option<&str> {
        self.matched_rule.as_deref()
    }
}

fn redacted_destination(parsed: &ParsedProxyRequest) -> String {
    redact_query_values(&format!(
        "{}://{}:{}{}",
        match parsed.egress_request.protocol {
            EgressProtocol::Http => "http",
            EgressProtocol::Https => "https",
            EgressProtocol::Tcp => "tcp",
        },
        parsed.upstream_host,
        parsed.upstream_port,
        match &parsed.mode {
            ProxyRequestMode::ForwardHttp { origin_form } => origin_form.as_str(),
            ProxyRequestMode::ConnectTunnel => "",
        }
    ))
}
