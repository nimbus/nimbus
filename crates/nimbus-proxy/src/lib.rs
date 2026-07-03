mod body;
mod connect;
mod credentials;
mod decision_log;
mod dns;
mod enforcement;
mod error;
mod https_intercept;
mod phase;
mod pingora_app;
mod pingora_identity;
mod pingora_io;
mod policy_state;
mod pool;
mod redaction;
mod request;
mod response;
mod substrate;
mod tls_authority;
mod worker;

#[cfg(test)]
mod tests;

pub use credentials::{
    CredentialSecretProvider, CredentialSecretProviderRef, CredentialSecretStore,
};
pub use decision_log::{
    AppendOnlyDecisionLogSink, DecisionLogSinkContext, DecisionLogger, EgressDecisionLog,
};
pub use dns::{DnsCacheConfig, DnsResolution};
pub use error::{EgressProxyError, Result};
pub use phase::{EgressProxyRequestPhase, REQUEST_PHASE_ORDER};
pub use policy_state::{EgressProxyReadiness, PolicyGeneration};
pub use pool::{
    EgressProxyCredentialDlpMode, EgressProxyPoolIdentity, EgressProxyPoolKey,
    EgressProxySubstrate, TlsVerificationMode,
};
pub use redaction::redact_egress_decision_log_value;
pub use substrate::ProxySubstrate;
pub use tls_authority::EgressProxyTlsAuthority;
pub use worker::{EgressProxy, EgressProxyConfig};

pub(crate) const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
pub(crate) const DEFAULT_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
pub(crate) const DEFAULT_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
pub(crate) const DEFAULT_MAX_CONNECTIONS: usize = 128;
