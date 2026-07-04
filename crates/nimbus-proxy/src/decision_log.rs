use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use nimbus_egress::EgressProtocol;

use crate::policy_state::PolicyGeneration;
use crate::redaction::redact_egress_decision_log_value;
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
    protocol: EgressProtocol,
    canonical_host: String,
    port: u16,
    credential_identity: Option<String>,
    allowed: bool,
    reason: String,
    matched_rule: Option<String>,
    policy_generation: Option<PolicyGeneration>,
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
            protocol: parsed.egress_request.protocol,
            canonical_host: parsed.upstream_host.clone(),
            port: parsed.upstream_port,
            credential_identity,
            allowed: true,
            reason,
            matched_rule,
            policy_generation: None,
        }
    }

    /// Test-only synthetic event (fan-out and counter tests need only the
    /// allowed/denied disposition, not a parsed request).
    #[cfg(test)]
    pub(crate) fn synthetic_for_test(allowed: bool) -> Self {
        Self {
            destination: "https://example.test:443".to_owned(),
            protocol: nimbus_egress::EgressProtocol::Https,
            canonical_host: "example.test".to_owned(),
            port: 443,
            credential_identity: None,
            allowed,
            reason: if allowed {
                "allowed by rule `test`".to_owned()
            } else {
                "no rule matched".to_owned()
            },
            matched_rule: allowed.then(|| "test".to_owned()),
            policy_generation: None,
        }
    }

    pub(crate) fn denied(
        parsed: &ParsedProxyRequest,
        reason: String,
        matched_rule: Option<String>,
    ) -> Self {
        Self {
            destination: redacted_destination(parsed),
            protocol: parsed.egress_request.protocol,
            canonical_host: parsed.upstream_host.clone(),
            port: parsed.upstream_port,
            credential_identity: None,
            allowed: false,
            reason,
            matched_rule,
            policy_generation: None,
        }
    }

    /// Terminal deny for a request the strict parser rejected before any
    /// canonical authority existed (bare CR/LF smuggling, Transfer-Encoding,
    /// parser-differential authorities, oversized headers). Blocked smuggling
    /// attempts are exactly what an auditor wants to see, so they must not be
    /// an audit blind spot just because no destination could be parsed.
    pub(crate) fn malformed(reason: String) -> Self {
        Self {
            destination: "<unparsed>".to_owned(),
            protocol: EgressProtocol::Tcp,
            canonical_host: String::new(),
            port: 0,
            credential_identity: None,
            allowed: false,
            reason,
            matched_rule: None,
            policy_generation: None,
        }
    }

    pub(crate) fn with_policy_generation(mut self, generation: PolicyGeneration) -> Self {
        self.policy_generation = Some(generation);
        self
    }

    pub fn destination(&self) -> &str {
        &self.destination
    }

    pub fn protocol(&self) -> EgressProtocol {
        self.protocol
    }

    pub fn canonical_host(&self) -> &str {
        &self.canonical_host
    }

    pub fn port(&self) -> u16 {
        self.port
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

    pub fn reason_class(&self) -> &'static str {
        if self.allowed {
            return "allowed";
        }
        if self.canonical_host.is_empty() {
            return "malformed";
        }
        let reason = self.reason.to_ascii_lowercase();
        if reason.contains("dlp") {
            "dlp"
        } else if reason.contains("credential") {
            "credential"
        } else if reason.contains("dns") {
            "dns"
        } else if reason.contains("internal/non-global") {
            "internal_target"
        } else if reason.contains("default deny") || reason.contains("no active policy") {
            "default_deny"
        } else if reason.contains("request bodies require content-length")
            || reason.contains("request line")
            || reason.contains("bad request")
        {
            "bad_request"
        } else if reason.contains("upstream") || reason.contains("resolution failed") {
            "upstream_error"
        } else {
            "deny"
        }
    }

    /// The name of the policy rule that produced the verdict, when one matched.
    pub fn matched_rule(&self) -> Option<&str> {
        self.matched_rule.as_deref()
    }

    pub fn policy_generation(&self) -> Option<PolicyGeneration> {
        self.policy_generation
    }
}

/// Static correlation attached to every append-only egress decision event for a
/// live sandbox PEP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionLogSinkContext {
    tenant_id: String,
    workload_id: String,
}

impl DecisionLogSinkContext {
    pub fn new(tenant_id: impl Into<String>, workload_id: impl Into<String>) -> Self {
        Self {
            tenant_id: tenant_id.into(),
            workload_id: workload_id.into(),
        }
    }
}

/// Append-only JSONL file sink for terminal egress decisions. Construction is
/// intentionally fallible: the OCI launch path opens the directory and file and
/// dry-runs serialization before a workload is admitted, so an unlogged PEP
/// fails closed instead of silently falling back to the noop logger.
pub struct AppendOnlyDecisionLogSink {
    context: DecisionLogSinkContext,
    file: Arc<Mutex<File>>,
}

impl AppendOnlyDecisionLogSink {
    pub fn open(path: impl AsRef<Path>, context: DecisionLogSinkContext) -> io::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let sink = Self {
            context,
            file: Arc::new(Mutex::new(file)),
        };
        let probe = EgressDecisionLog {
            destination: "http://audit-probe.invalid:80/".to_owned(),
            protocol: EgressProtocol::Http,
            canonical_host: "audit-probe.invalid".to_owned(),
            port: 80,
            credential_identity: None,
            allowed: false,
            reason: "egress decision log serialization probe".to_owned(),
            matched_rule: None,
            policy_generation: None,
        };
        sink.serialize(&probe).map_err(|error| {
            io::Error::other(format!(
                "failed to serialize egress decision log readiness probe: {error}"
            ))
        })?;
        Ok(sink)
    }

    pub fn logger(self) -> DecisionLogger {
        let sink = Arc::new(self);
        Arc::new(move |log| {
            if let Err(error) = sink.append(&log) {
                eprintln!("failed to append egress decision log: {error}");
            }
        })
    }

    pub fn append(&self, log: &EgressDecisionLog) -> io::Result<()> {
        let line = self.serialize(log).map_err(|error| {
            io::Error::other(format!("failed to serialize egress decision log: {error}"))
        })?;
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("egress decision log file lock is poisoned"))?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()
    }

    fn serialize(&self, log: &EgressDecisionLog) -> serde_json::Result<String> {
        serde_json::to_string(&serde_json::json!({
            "tenant_id": &self.context.tenant_id,
            "workload_id": &self.context.workload_id,
            "policy_generation": log.policy_generation().map(PolicyGeneration::get),
            "rule_id": log.matched_rule(),
            "protocol": protocol_label(log.protocol()),
            "canonical_host": log.canonical_host(),
            "port": log.port(),
            "decision": if log.is_allowed() { "allow" } else { "deny" },
            "allowed": log.is_allowed(),
            "reason_class": log.reason_class(),
            "reason": redact_egress_decision_log_value("egress_decision_reason", log.reason()),
            "destination": log.destination(),
            "credential_identity": log
                .credential_identity()
                .map(|identity| redact_egress_decision_log_value("credential_identity", identity)),
        }))
    }
}

fn redacted_destination(parsed: &ParsedProxyRequest) -> String {
    redact_query_values(&format!(
        "{}://{}:{}{}",
        protocol_label(parsed.egress_request.protocol),
        parsed.upstream_host,
        parsed.upstream_port,
        match &parsed.mode {
            ProxyRequestMode::ForwardHttp { origin_form } => origin_form.as_str(),
            ProxyRequestMode::ConnectTunnel => "",
        }
    ))
}

fn protocol_label(protocol: EgressProtocol) -> &'static str {
    match protocol {
        EgressProtocol::Http => "http",
        EgressProtocol::Https => "https",
        EgressProtocol::Tcp => "tcp",
    }
}
