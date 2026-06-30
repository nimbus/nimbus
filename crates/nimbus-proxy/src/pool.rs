//! Connection-pool isolation key for PLANNED egress connection pooling.
//!
//! [`EgressProxyPoolKey`] (with [`EgressProxySubstrate`] and
//! [`TlsVerificationMode`]) is the seam for the connection broker / pooling work
//! owned by `docs/private/plans/connection-broker-plan.md`; the NEG plan treats
//! "pool-key completeness" as a testable PEP invariant. It is NOT yet wired:
//! today the worker dials a fresh upstream connection per request (see
//! `worker.rs::handle_client`), so nothing is pooled or reused. The key's
//! presence documents the intended isolation contract — that two requests
//! differing in any security-relevant dimension (tenant, policy generation,
//! injected credential identity, destination authority, resolved peer, SNI, TLS
//! verification mode, client-certificate identity, ALPN, or upstream-proxy
//! settings) can never share a pooled connection once pooling lands. It is the
//! intended contract, not an enforced guarantee.

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
