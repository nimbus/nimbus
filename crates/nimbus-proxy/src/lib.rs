mod credentials;
mod decision_log;
mod dns;
mod enforcement;
mod error;
mod phase;
mod policy_state;
mod pool;
mod redaction;
mod request;
mod response;
mod worker;

#[cfg(test)]
mod tests;

pub use credentials::CredentialSecretStore;
pub use decision_log::{DecisionLogger, EgressDecisionLog};
pub use dns::{DnsCacheConfig, DnsResolution};
pub use error::{EgressProxyError, Result};
pub use phase::{EgressProxyRequestPhase, REQUEST_PHASE_ORDER};
pub use policy_state::{EgressProxyReadiness, PolicyGeneration};
pub use pool::{EgressProxyPoolKey, EgressProxySubstrate, TlsVerificationMode};
pub use redaction::redact_egress_decision_log_value;
pub use worker::{EgressProxy, EgressProxyConfig};

pub(crate) const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
pub(crate) const DEFAULT_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
pub(crate) const DEFAULT_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
pub(crate) const DEFAULT_MAX_CONNECTIONS: usize = 128;
