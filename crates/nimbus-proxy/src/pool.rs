//! Pool identity for Pingora peer construction and planned connection reuse.
//!
//! [`EgressProxyPoolKey`] (with [`EgressProxySubstrate`] and
//! [`TlsVerificationMode`]) is the seam for the connection broker / pooling work
//! owned by the connection-broker plan; the NEG plan treats "pool-key
//! completeness" as a testable PEP invariant. Today the key is derived for
//! every allowed request and mapped into Pingora peer identity (`group_key` and
//! `reuse_hash`), but cross-request connection reuse is deliberately disabled
//! with a zero idle timeout until collision-safe brokered pooling lands. The
//! key documents the intended isolation contract: two requests differing in any
//! security-relevant dimension can never share a pooled connection once pooling
//! is enabled.
//!
//! ## Reuse contract (adversarial review 2026-07-02 — read before enabling reuse)
//!
//! A multi-agent adversarial review of a proposed shared upstream pool produced
//! hard constraints. Whoever enables reuse MUST honor all of them:
//!
//! - **Do not reduce the reuse key.** Every current field, INCLUDING
//!   `credential_identity` and `credential_dlp_mode`, stays in the
//!   `reuse_hash` preimage. A socket that ever carried credential A must never
//!   serve credential B or a plain request: upstreams pin state to the
//!   connection (server-side "already authed as A", connection-scoped cookies,
//!   LB affinity), so header-per-request bytes are not sufficient. Dropping the
//!   credential fields is a credential-crossing regression.
//! - **Connection-oriented auth defeats key-based isolation.** NTLM and
//!   Negotiate/Kerberos authenticate the socket, not the request; no key field
//!   sees auth the proxy did not inject (e.g. a guest completing NTLM inside an
//!   intercepted TLS tunnel). Any connection whose response carried
//!   `401`/`407`/`WWW-Authenticate: NTLM|Negotiate|Kerberos` must be evicted,
//!   never pooled.
//! - **The 64-bit `reuse_hash` is not collision-proof.** Checkout re-validates
//!   only the peer IP:port (`getpeername`) and liveness, not SNI / authority /
//!   TLS-verify / credential. Before trusting a pooled socket, byte-compare the
//!   full [`EgressProxyPoolKey::canonical_preimage_bytes`] bound to the
//!   connection, not just the hash bucket.
//! - **Intercepted-HTTPS re-origination must stay fresh** (per-request
//!   connection, `Connection: close`) unless it is moved onto Pingora's
//!   `HttpSession`/connector for correct drain framing AND it saw no auth
//!   challenge and no guest-supplied credential header.
//! - **The connector must stay per-PEP.** Cross-tenant reuse is impossible only
//!   because each sandbox owns its own connection pool object; a shared/static
//!   node-wide connector would turn a 64-bit hash collision into a cross-tenant
//!   reuse primitive. Keep `tenant_id`/`workload_id` in the preimage as
//!   defense-in-depth regardless.
//!
//! Consequence: safe reuse is narrow (plain forward HTTP, same-context, h1,
//! no auth challenge). Most agent egress is HTTPS — spliced (an opaque tunnel
//! the proxy never pools) or intercepted (fresh) — so the efficiency upside is
//! bounded, which is why enabling reuse stays connection-broker-plan scope
//! rather than a quick flip here.

use std::net::IpAddr;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressProxyPoolIdentity {
    pub tenant_id: TenantId,
    pub workload_id: String,
    pub substrate: EgressProxySubstrate,
}

impl EgressProxyPoolIdentity {
    pub fn new(
        tenant_id: TenantId,
        workload_id: impl Into<String>,
        substrate: EgressProxySubstrate,
    ) -> Self {
        Self {
            tenant_id,
            workload_id: workload_id.into(),
            substrate,
        }
    }
}

impl Default for EgressProxyPoolIdentity {
    fn default() -> Self {
        Self {
            tenant_id: TenantId::new("local").expect("default tenant id should be valid"),
            workload_id: "standalone".to_owned(),
            substrate: EgressProxySubstrate::Container,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EgressProxyCredentialDlpMode {
    Plain,
    Credential,
    Dlp,
    CredentialAndDlp,
}

impl EgressProxyCredentialDlpMode {
    pub(crate) fn from_rule_requirements(has_credential: bool, has_dlp: bool) -> Self {
        match (has_credential, has_dlp) {
            (false, false) => Self::Plain,
            (true, false) => Self::Credential,
            (false, true) => Self::Dlp,
            (true, true) => Self::CredentialAndDlp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EgressProxyPoolKey {
    pub tenant_id: TenantId,
    pub workload_id: String,
    pub substrate: EgressProxySubstrate,
    pub policy_generation: PolicyGeneration,
    pub credential_identity: Option<String>,
    pub credential_dlp_mode: EgressProxyCredentialDlpMode,
    pub destination: String,
    pub resolved_peer: SocketAddr,
    pub sni: Option<String>,
    pub tls_verification: TlsVerificationMode,
    pub client_cert_identity: Option<String>,
    pub alpn: Vec<String>,
    pub proxy_settings: Option<String>,
}

impl EgressProxyPoolKey {
    pub(crate) fn nimbus_group_key(&self) -> u64 {
        let digest = blake3::hash(&self.canonical_preimage_bytes());
        u64::from_le_bytes(
            digest.as_bytes()[..8]
                .try_into()
                .expect("BLAKE3 digest is always at least eight bytes"),
        )
    }

    pub(crate) fn canonical_preimage_bytes(&self) -> Vec<u8> {
        let mut preimage = Vec::new();
        write_str(&mut preimage, "tenant_id", self.tenant_id.as_str());
        write_str(&mut preimage, "workload_id", &self.workload_id);
        write_str(
            &mut preimage,
            "substrate",
            match self.substrate {
                EgressProxySubstrate::Container => "container",
                EgressProxySubstrate::Isolate => "isolate",
                EgressProxySubstrate::Wasm => "wasm",
            },
        );
        write_u64(
            &mut preimage,
            "policy_generation",
            self.policy_generation.get(),
        );
        write_optional_str(
            &mut preimage,
            "credential_identity",
            self.credential_identity.as_deref(),
        );
        write_str(
            &mut preimage,
            "credential_dlp_mode",
            match self.credential_dlp_mode {
                EgressProxyCredentialDlpMode::Plain => "plain",
                EgressProxyCredentialDlpMode::Credential => "credential",
                EgressProxyCredentialDlpMode::Dlp => "dlp",
                EgressProxyCredentialDlpMode::CredentialAndDlp => "credential_and_dlp",
            },
        );
        write_str(&mut preimage, "destination", &self.destination);
        write_socket_addr(&mut preimage, "resolved_peer", self.resolved_peer);
        write_optional_str(&mut preimage, "sni", self.sni.as_deref());
        write_str(
            &mut preimage,
            "tls_verification",
            match self.tls_verification {
                TlsVerificationMode::WebPki => "webpki",
                TlsVerificationMode::Disabled => "disabled",
            },
        );
        write_optional_str(
            &mut preimage,
            "client_cert_identity",
            self.client_cert_identity.as_deref(),
        );
        write_u64(&mut preimage, "alpn_count", self.alpn.len() as u64);
        for alpn in &self.alpn {
            write_str(&mut preimage, "alpn", alpn);
        }
        write_optional_str(
            &mut preimage,
            "proxy_settings",
            self.proxy_settings.as_deref(),
        );
        preimage
    }
}

fn write_str(preimage: &mut Vec<u8>, label: &str, value: &str) {
    preimage.extend_from_slice(label.as_bytes());
    preimage.push(0);
    preimage.extend_from_slice(&(value.len() as u64).to_le_bytes());
    preimage.extend_from_slice(value.as_bytes());
    preimage.push(0xff);
}

fn write_optional_str(preimage: &mut Vec<u8>, label: &str, value: Option<&str>) {
    match value {
        Some(value) => {
            write_str(preimage, label, "some");
            write_str(preimage, label, value);
        }
        None => write_str(preimage, label, "none"),
    }
}

fn write_u64(preimage: &mut Vec<u8>, label: &str, value: u64) {
    preimage.extend_from_slice(label.as_bytes());
    preimage.push(0);
    preimage.extend_from_slice(&8_u64.to_le_bytes());
    preimage.extend_from_slice(&value.to_le_bytes());
    preimage.push(0xff);
}

fn write_socket_addr(preimage: &mut Vec<u8>, label: &str, value: SocketAddr) {
    match value.ip() {
        IpAddr::V4(ip) => {
            write_str(preimage, label, "ipv4");
            preimage.extend_from_slice(&ip.octets());
        }
        IpAddr::V6(ip) => {
            write_str(preimage, label, "ipv6");
            preimage.extend_from_slice(&ip.octets());
        }
    }
    preimage.extend_from_slice(&value.port().to_le_bytes());
    preimage.push(0xff);
}
