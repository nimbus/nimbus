use std::net::SocketAddr;

use pingora_core::upstreams::peer::HttpPeer;

use crate::pool::{EgressProxyPoolKey, TlsVerificationMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PingoraTlsMode {
    Plaintext,
    UpstreamTls,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PingoraPeerPlan {
    pub(crate) resolved_peer: SocketAddr,
    pub(crate) sni: Option<String>,
    pub(crate) tls_mode: PingoraTlsMode,
    pub(crate) tls_verification: TlsVerificationMode,
    pub(crate) group_key: u64,
    pub(crate) canonical_pool_key: Vec<u8>,
}

impl PingoraPeerPlan {
    pub(crate) fn from_pool_key(pool_key: &EgressProxyPoolKey) -> Self {
        Self {
            resolved_peer: pool_key.resolved_peer,
            sni: pool_key.sni.clone(),
            tls_mode: if pool_key.sni.is_some() {
                PingoraTlsMode::UpstreamTls
            } else {
                PingoraTlsMode::Plaintext
            },
            tls_verification: pool_key.tls_verification,
            group_key: pool_key.nimbus_group_key(),
            canonical_pool_key: pool_key.canonical_preimage_bytes(),
        }
    }

    pub(crate) fn to_pingora_peer(&self) -> HttpPeer {
        let mut peer = HttpPeer::new(
            self.resolved_peer,
            matches!(self.tls_mode, PingoraTlsMode::UpstreamTls),
            self.sni.clone().unwrap_or_default(),
        );
        peer.group_key = self.group_key;
        if matches!(self.tls_verification, TlsVerificationMode::Disabled) {
            peer.options.verify_cert = false;
            peer.options.verify_hostname = false;
        }
        peer
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use nimbus_core::TenantId;
    use pingora_core::upstreams::peer::Peer;

    use super::*;
    use crate::policy_state::PolicyGeneration;
    use crate::pool::{
        EgressProxyCredentialDlpMode, EgressProxyPoolKey, EgressProxySubstrate, TlsVerificationMode,
    };

    #[test]
    fn pingora_peer_reuse_hash_changes_for_every_pool_dimension() {
        let base = base_pool_key();
        let base_plan = PingoraPeerPlan::from_pool_key(&base);
        let base_peer = base_plan.to_pingora_peer();
        let base_reuse_hash = base_peer.reuse_hash();

        type Mutator = fn(&mut EgressProxyPoolKey);
        let mutators: Vec<(&str, Mutator)> = vec![
            ("tenant", |key| {
                key.tenant_id = TenantId::new("tenant-b").expect("tenant id should be valid");
            }),
            ("workload", |key| {
                key.workload_id = "workload-b".to_owned();
            }),
            ("substrate", |key| {
                key.substrate = EgressProxySubstrate::Isolate;
            }),
            ("policy generation", |key| {
                key.policy_generation = key
                    .policy_generation
                    .next()
                    .expect("fixture policy generation should not overflow");
            }),
            ("credential", |key| {
                key.credential_identity = Some("credential:github".to_owned());
            }),
            ("credential/DLP mode", |key| {
                key.credential_dlp_mode = EgressProxyCredentialDlpMode::Credential;
            }),
            ("destination", |key| {
                key.destination = "https://api.github.com:443".to_owned();
            }),
            ("resolved peer", |key| {
                key.resolved_peer = SocketAddr::from(([203, 0, 113, 11], 443));
            }),
            ("SNI", |key| {
                key.sni = Some("uploads.example.test".to_owned());
            }),
            ("TLS verification", |key| {
                key.tls_verification = TlsVerificationMode::Disabled;
            }),
            ("client cert", |key| {
                key.client_cert_identity = Some("client-cert:alt".to_owned());
            }),
            ("ALPN", |key| {
                key.alpn = vec!["http/1.1".to_owned()];
            }),
            ("proxy settings", |key| {
                key.proxy_settings = Some("egress-proxy-a".to_owned());
            }),
        ];

        for (dimension, mutate) in mutators {
            let mut changed = base.clone();
            mutate(&mut changed);
            let changed_plan = PingoraPeerPlan::from_pool_key(&changed);
            let changed_peer = changed_plan.to_pingora_peer();
            assert_ne!(
                base_plan.canonical_pool_key, changed_plan.canonical_pool_key,
                "canonical pool-key preimage must include {dimension}"
            );
            assert_ne!(
                base_plan.group_key, changed_plan.group_key,
                "Nimbus group_key must include {dimension}"
            );
            assert_ne!(
                base_reuse_hash,
                changed_peer.reuse_hash(),
                "Pingora reuse_hash must diverge for {dimension}"
            );
        }
    }

    #[test]
    fn pingora_peer_applies_tls_verification_mode() {
        let mut disabled = base_pool_key();
        disabled.tls_verification = TlsVerificationMode::Disabled;

        let peer = PingoraPeerPlan::from_pool_key(&disabled).to_pingora_peer();

        assert!(!peer.verify_cert());
        assert!(!peer.verify_hostname());
    }

    fn base_pool_key() -> EgressProxyPoolKey {
        EgressProxyPoolKey {
            tenant_id: TenantId::new("tenant-a").expect("tenant id should be valid"),
            workload_id: "workload-a".to_owned(),
            substrate: EgressProxySubstrate::Container,
            policy_generation: PolicyGeneration::initial(),
            credential_identity: Some("credential:stripe".to_owned()),
            credential_dlp_mode: EgressProxyCredentialDlpMode::CredentialAndDlp,
            destination: "https://api.stripe.com:443".to_owned(),
            resolved_peer: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10)), 443),
            sni: Some("api.stripe.com".to_owned()),
            tls_verification: TlsVerificationMode::WebPki,
            client_cert_identity: Some("client-cert:payments".to_owned()),
            alpn: vec!["h2".to_owned(), "http/1.1".to_owned()],
            proxy_settings: Some("direct".to_owned()),
        }
    }
}
