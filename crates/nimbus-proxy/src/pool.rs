use std::net::SocketAddr;

use nimbus_core::TenantId;

use crate::policy_state::PolicyGeneration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EgressProxySubstrate {
    Container,
    Isolate,
    Wasm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TlsVerificationMode {
    WebPki,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EgressProxyPoolKey {
    pub tenant_id: TenantId,
    pub substrate: EgressProxySubstrate,
    pub policy_generation: PolicyGeneration,
    pub credential_identity: Option<String>,
    pub destination: String,
    pub resolved_peer: SocketAddr,
    pub sni: Option<String>,
    pub tls_verification: TlsVerificationMode,
    pub client_cert_identity: Option<String>,
    pub alpn: Vec<String>,
    pub proxy_settings: Option<String>,
}
