use std::sync::Arc;

use nimbus_egress::EgressProtocol;

use crate::redaction::redact_query_values;
use crate::request::{ParsedProxyRequest, ProxyRequestMode};

pub type DecisionLogger = Arc<dyn Fn(EgressDecisionLog) + Send + Sync + 'static>;

pub(crate) fn noop_decision_logger() -> DecisionLogger {
    Arc::new(|_| {})
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressDecisionLog {
    destination: String,
    credential_identity: Option<String>,
}

impl EgressDecisionLog {
    pub(crate) fn for_request(
        parsed: &ParsedProxyRequest,
        credential_identity: Option<String>,
    ) -> Self {
        Self {
            destination: redact_query_values(&format!(
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
            )),
            credential_identity,
        }
    }

    pub fn destination(&self) -> &str {
        &self.destination
    }

    pub fn credential_identity(&self) -> Option<&str> {
        self.credential_identity.as_deref()
    }
}
