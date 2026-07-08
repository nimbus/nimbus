use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use nimbus_core::base64_encode_standard;
use nimbus_egress::canonicalize_authority_host;
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, PKCS_ECDSA_P256_SHA256,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use time::OffsetDateTime;

use crate::error::{EgressProxyError, Result};

const NIMBUS_CA_CN: &str = "Nimbus Egress Proxy CA";
const CA_VALIDITY_DAYS: i64 = 3650;
const LEAF_VALIDITY_HOURS: i64 = 24;
const LEAF_REFRESH_BUFFER: Duration = Duration::from_secs(60 * 60);
pub(crate) const LEAF_CACHE_CAP: usize = 64;

#[derive(Clone)]
pub struct WorkloadPepTlsAuthority {
    inner: Arc<TlsAuthorityInner>,
}

struct TlsAuthorityInner {
    ca_params: CertificateParams,
    ca_key: KeyPair,
    ca_cert_der: CertificateDer<'static>,
    // The cache is intentionally tiny and bounded: each intercepted hostname can
    // mint a distinct in-process leaf, so untrusted destination cardinality must
    // not translate into unbounded certificate/key retention.
    leaf_cache: RwLock<BTreeMap<String, CachedLeaf>>,
    upstream_roots: Arc<RootCertStore>,
}

struct CachedLeaf {
    server_config: Arc<ServerConfig>,
    expires_at: SystemTime,
}

impl WorkloadPepTlsAuthority {
    pub fn generate_ephemeral() -> Result<Self> {
        Self::generate_with_upstream_roots(default_upstream_roots())
    }

    pub fn trust_anchor_der(&self) -> CertificateDer<'static> {
        self.inner.ca_cert_der.clone()
    }

    pub fn trust_anchor_pem(&self) -> String {
        der_to_pem("CERTIFICATE", self.inner.ca_cert_der.as_ref())
    }

    pub(crate) fn server_config_for_host(&self, hostname: &str) -> Result<Arc<ServerConfig>> {
        let hostname = canonicalize_authority_host(hostname).map_err(|error| {
            operation_failed(format!(
                "TLS interception leaf hostname is invalid: {error}"
            ))
        })?;
        let now = SystemTime::now();
        {
            let guard = self.inner.leaf_cache.read().map_err(|_| {
                operation_failed("TLS interception leaf cache lock is poisoned".to_owned())
            })?;
            if let Some(cached) = guard.get(&hostname) {
                let refresh_at = cached
                    .expires_at
                    .checked_sub(LEAF_REFRESH_BUFFER)
                    .unwrap_or(cached.expires_at);
                if now < refresh_at {
                    return Ok(Arc::clone(&cached.server_config));
                }
            }
        }

        let server_config = Arc::new(self.generate_leaf_server_config(&hostname)?);
        let mut guard = self.inner.leaf_cache.write().map_err(|_| {
            operation_failed("TLS interception leaf cache lock is poisoned".to_owned())
        })?;
        if let Some(cached) = guard.get(&hostname) {
            let refresh_at = cached
                .expires_at
                .checked_sub(LEAF_REFRESH_BUFFER)
                .unwrap_or(cached.expires_at);
            if now < refresh_at {
                return Ok(Arc::clone(&cached.server_config));
            }
        }
        evict_leaf_cache_for_insert(&mut guard, now);
        guard.insert(
            hostname,
            CachedLeaf {
                server_config: Arc::clone(&server_config),
                expires_at: now + Duration::from_secs(LEAF_VALIDITY_HOURS as u64 * 60 * 60),
            },
        );
        Ok(server_config)
    }

    #[cfg(test)]
    pub(crate) fn generate_ephemeral_with_upstream_trust_anchors(
        trust_anchors: impl IntoIterator<Item = CertificateDer<'static>>,
    ) -> Result<Self> {
        let mut roots = RootCertStore::empty();
        for trust_anchor in trust_anchors {
            roots.add(trust_anchor).map_err(|error| {
                operation_failed(format!("failed to add upstream test trust anchor: {error}"))
            })?;
        }
        Self::generate_with_upstream_roots(Arc::new(roots))
    }

    pub(crate) fn upstream_client_config(&self) -> Result<Arc<ClientConfig>> {
        let mut config = ClientConfig::builder_with_provider(ring_provider())
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
            .map_err(|error| {
                operation_failed(format!(
                    "failed to configure upstream TLS protocol versions: {error}"
                ))
            })?
            .with_root_certificates(self.upstream_roots())
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }

    fn generate_with_upstream_roots(upstream_roots: Arc<RootCertStore>) -> Result<Self> {
        let ca_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).map_err(|error| {
            operation_failed(format!("failed to generate proxy CA key: {error}"))
        })?;
        let ca_params = ca_params();
        let ca_cert = ca_params
            .self_signed(&ca_key)
            .map_err(|error| operation_failed(format!("failed to self-sign proxy CA: {error}")))?;
        let ca_cert_der = ca_cert.der().clone();
        Ok(Self {
            inner: Arc::new(TlsAuthorityInner {
                ca_params,
                ca_key,
                ca_cert_der,
                leaf_cache: RwLock::new(BTreeMap::new()),
                upstream_roots,
            }),
        })
    }

    fn generate_leaf_server_config(&self, hostname: &str) -> Result<ServerConfig> {
        let leaf_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).map_err(|error| {
            operation_failed(format!(
                "failed to generate TLS interception leaf key: {error}"
            ))
        })?;
        let mut params = CertificateParams::new(vec![hostname.to_owned()]).map_err(|error| {
            operation_failed(format!(
                "failed to create TLS interception leaf parameters: {error}"
            ))
        })?;
        params.distinguished_name.push(DnType::CommonName, hostname);
        params.use_authority_key_identifier_extension = true;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.not_before = OffsetDateTime::now_utc() - time::Duration::minutes(5);
        params.not_after = OffsetDateTime::now_utc() + time::Duration::hours(LEAF_VALIDITY_HOURS);
        let issuer = rcgen::Issuer::from_params(&self.inner.ca_params, &self.inner.ca_key);
        let leaf_cert = params.signed_by(&leaf_key, &issuer).map_err(|error| {
            operation_failed(format!("failed to sign TLS interception leaf: {error}"))
        })?;
        let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
        let mut config = ServerConfig::builder_with_provider(ring_provider())
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
            .map_err(|error| {
                operation_failed(format!(
                    "failed to configure TLS interception protocol versions: {error}"
                ))
            })?
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf_cert.der().clone(), self.trust_anchor_der()],
                key_der,
            )
            .map_err(|error| {
                operation_failed(format!(
                    "failed to build TLS interception server config: {error}"
                ))
            })?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(config)
    }

    fn upstream_roots(&self) -> RootCertStore {
        RootCertStore {
            roots: self.inner.upstream_roots.roots.clone(),
        }
    }

    #[cfg(test)]
    pub(crate) fn leaf_cache_len_for_test(&self) -> usize {
        self.inner
            .leaf_cache
            .read()
            .expect("leaf cache lock should not be poisoned")
            .len()
    }

    #[cfg(test)]
    pub(crate) fn leaf_cache_contains_for_test(&self, hostname: &str) -> bool {
        self.inner
            .leaf_cache
            .read()
            .expect("leaf cache lock should not be poisoned")
            .contains_key(hostname)
    }

    #[cfg(test)]
    pub(crate) fn expire_leaf_for_test(&self, hostname: &str) {
        let mut guard = self
            .inner
            .leaf_cache
            .write()
            .expect("leaf cache lock should not be poisoned");
        if let Some(cached) = guard.get_mut(hostname) {
            cached.expires_at = SystemTime::UNIX_EPOCH;
        }
    }
}

fn evict_leaf_cache_for_insert(cache: &mut BTreeMap<String, CachedLeaf>, now: SystemTime) {
    if cache.len() < LEAF_CACHE_CAP {
        return;
    }
    cache.retain(|_, cached| cached.expires_at > now);
    if cache.len() < LEAF_CACHE_CAP {
        return;
    }
    if let Some(oldest_host) = cache
        .iter()
        .min_by_key(|(_, cached)| cached.expires_at)
        .map(|(hostname, _)| hostname.clone())
    {
        cache.remove(&oldest_host);
    }
}

fn ca_params() -> CertificateParams {
    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, NIMBUS_CA_CN);
    params
        .distinguished_name
        .push(DnType::OrganizationName, "Nimbus");
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params.not_before = OffsetDateTime::now_utc() - time::Duration::minutes(5);
    params.not_after = OffsetDateTime::now_utc() + time::Duration::days(CA_VALIDITY_DAYS);
    params
}

fn default_upstream_roots() -> Arc<RootCertStore> {
    Arc::new(RootCertStore::from_iter(
        webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
    ))
}

pub(crate) fn ring_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn operation_failed(message: String) -> EgressProxyError {
    EgressProxyError::OperationFailed { message }
}

fn der_to_pem(label: &str, der: &[u8]) -> String {
    let encoded = base64_encode_standard(der);
    let mut pem = format!("-----BEGIN {label}-----\n");
    for chunk in encoded.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).expect("base64 output must be ASCII"));
        pem.push('\n');
    }
    pem.push_str(&format!("-----END {label}-----\n"));
    pem
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_cache_never_exceeds_cap_and_keeps_newest_leaf() {
        let authority =
            WorkloadPepTlsAuthority::generate_ephemeral().expect("authority should generate");

        for index in 0..(LEAF_CACHE_CAP + 8) {
            authority
                .server_config_for_host(&format!("host-{index}.test"))
                .expect("leaf should build");
        }

        assert_eq!(authority.leaf_cache_len_for_test(), LEAF_CACHE_CAP);
        assert!(
            authority.leaf_cache_contains_for_test(&format!("host-{}.test", LEAF_CACHE_CAP + 7)),
            "newest hostname should remain cached after capacity eviction"
        );
    }

    #[test]
    fn leaf_cache_evicts_expired_entries_before_live_entries() {
        let authority =
            WorkloadPepTlsAuthority::generate_ephemeral().expect("authority should generate");
        for index in 0..LEAF_CACHE_CAP {
            authority
                .server_config_for_host(&format!("host-{index}.test"))
                .expect("leaf should build");
        }
        authority.expire_leaf_for_test("host-7.test");

        authority
            .server_config_for_host("new.test")
            .expect("new leaf should build");

        assert_eq!(authority.leaf_cache_len_for_test(), LEAF_CACHE_CAP);
        assert!(!authority.leaf_cache_contains_for_test("host-7.test"));
        assert!(authority.leaf_cache_contains_for_test("new.test"));
    }

    #[test]
    fn ephemeral_authorities_are_distinct_and_export_only_public_material() {
        // Isolation invariant: each sandbox mints its OWN ephemeral CA. Two
        // independent authorities must never share a trust anchor — if they did,
        // one sandbox's workload could be MITM'd by another sandbox's proxy.
        let first = WorkloadPepTlsAuthority::generate_ephemeral().expect("first CA");
        let second = WorkloadPepTlsAuthority::generate_ephemeral().expect("second CA");
        assert_ne!(
            first.trust_anchor_der(),
            second.trust_anchor_der(),
            "each sandbox must mint a distinct ephemeral CA; a shared CA is a cross-sandbox MITM blast radius"
        );

        // Custody invariant: workloads receive only the public certificate. The
        // exported material must be a CERTIFICATE and must never contain private
        // key material.
        let pem = first.trust_anchor_pem();
        assert!(
            pem.contains("-----BEGIN CERTIFICATE-----")
                && pem.contains("-----END CERTIFICATE-----"),
            "trust anchor export must be the public certificate: {pem}"
        );
        assert!(
            !pem.contains("PRIVATE KEY"),
            "trust anchor export must never contain private key material: {pem}"
        );
    }

    #[test]
    fn server_config_for_host_reuses_cached_config_inside_ttl() {
        let authority =
            WorkloadPepTlsAuthority::generate_ephemeral().expect("authority should generate");
        let first = authority
            .server_config_for_host("allowed.test")
            .expect("first leaf should build");
        let second = authority
            .server_config_for_host("allowed.test")
            .expect("second leaf should be cached");

        assert!(Arc::ptr_eq(&first, &second));
    }
}
