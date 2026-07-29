mod body;
mod connect;
mod credentials;
mod decision_log;
mod dns;
mod enforcement;
mod engine;
mod error;
mod fairness;
mod fanout;
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
mod terminal;
mod tls_authority;
mod worker;

#[cfg(test)]
mod tests;

pub use credentials::{
    CredentialSecretProvider, CredentialSecretProviderRef, CredentialSecretStore,
};
pub use decision_log::{
    AppendOnlyDecisionLogSink, DecisionLogSinkContext, DecisionLogger, DecisionRecordKind,
    DurableDecisionSink, EgressDecisionLog,
};
pub use dns::{DnsCacheConfig, DnsResolution};
pub use engine::{
    EgressEngine, RegisteredLifecyclePhase, RegistrationCommitFailure, RegistrationDecision,
    RegistrationSlot, RetainedFailedRegistration, StopHandle,
};
pub use error::{EgressProxyError, Result};
pub use fairness::{
    FairnessRegistry, TaskTimeSpan, TenantFairness, TenantLease, TenantTaskTimeAccounting,
};
pub use fanout::{fan_out_decision_loggers, tenant_decision_counter_sink};
pub use phase::{EgressProxyRequestPhase, REQUEST_PHASE_ORDER};
pub use policy_state::{
    PolicyGeneration, PolicyReloadAttempt, PolicyReloadObservation, PolicyReloadReceipt,
    WorkloadPepPolicyEvidence, WorkloadPepReadiness,
};
pub use pool::{
    EgressProxyCredentialDlpMode, EgressProxyPoolIdentity, EgressProxyPoolKey,
    EgressProxySubstrate, TlsVerificationMode,
};
pub use redaction::redact_egress_decision_log_value;
pub use substrate::ProxySubstrate;
pub use tls_authority::WorkloadPepTlsAuthority;
pub use worker::{PreparedWorkloadPep, WorkloadPep, WorkloadPepConfig};

pub(crate) const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
pub(crate) const DEFAULT_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
pub(crate) const DEFAULT_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
pub(crate) const DEFAULT_MAX_CONNECTIONS: usize = 128;
