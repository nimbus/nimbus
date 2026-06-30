//! Shared per-sandbox egress PEP (Policy Enforcement Point) lifecycle.
//!
//! Every sandbox backend places its workload inside a deny-by-default network
//! namespace whose only outbound path is a host-side `nimbus_proxy::EgressProxy`
//! bound on the bridge gateway. The "compile policy -> start the PEP -> register
//! / reload / stop" glue is security-critical and identical across backends
//! (the container backend today, the krun microVM backend next), so it lives
//! here once instead of being forked per backend.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};

use nimbus_egress::{CompiledEgressPolicy, EGRESS_PROXY_URL_ENV, EgressPolicy};
use nimbus_proxy::{EgressProxy, EgressProxyConfig, EgressProxyError, EgressProxyReadiness};

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

/// Registry of running per-sandbox egress proxies, shared by every sandbox
/// backend. Cloning shares the underlying registry (it is `Arc`-backed), so a
/// backend can hold one and hand clones to its async tasks.
#[derive(Clone, Default)]
pub(crate) struct EgressProxyRegistry {
    proxies: Arc<Mutex<HashMap<SandboxId, EgressProxy>>>,
}

impl EgressProxyRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Ensure a PEP is running for `id`, bound on `bind_addr`.
    ///
    /// Idempotent: a no-op if a proxy is already registered for `id`.
    /// Fail-closed: a policy compile error or proxy start error returns `Err`
    /// and registers nothing — callers must treat that as deny.
    pub(crate) fn ensure_running(
        &self,
        id: &SandboxId,
        policy: &EgressPolicy,
        bind_addr: SocketAddr,
    ) -> Result<()> {
        let mut proxies = self.lock()?;
        if proxies.contains_key(id) {
            return Ok(());
        }
        let compiled = policy
            .compile()
            .map_err(|message| SandboxError::InvalidSpec { message })?;
        let proxy = EgressProxy::start(EgressProxyConfig::new(compiled).with_bind_addr(bind_addr))
            .map_err(egress_proxy_error)?;
        proxies.insert(id.clone(), proxy);
        Ok(())
    }

    /// Hot-reload the policy on the running PEP for `id`.
    ///
    /// Errors if no proxy is registered for `id` (the caller ensures it is
    /// running first). Fail-closed: a reload error is surfaced, not swallowed.
    pub(crate) fn reload(&self, id: &SandboxId, compiled: CompiledEgressPolicy) -> Result<()> {
        let proxies = self.lock()?;
        let proxy = proxies
            .get(id)
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!("egress proxy for sandbox {id} is not running"),
            })?;
        proxy.reload_policy(compiled).map_err(egress_proxy_error)?;
        Ok(())
    }

    /// Stop and deregister the PEP for `id`. Dropping the `EgressProxy` stops it.
    /// No-op if none is registered.
    pub(crate) fn stop(&self, id: &SandboxId) -> Result<()> {
        self.lock()?.remove(id);
        Ok(())
    }

    /// Report the readiness of the PEP registered for `id`.
    ///
    /// Returns `Ok(None)` when no proxy is registered (so the caller treats an
    /// absent PEP as deny), and `Ok(Some(readiness))` carrying the proxy's
    /// active-policy state otherwise. A readiness gate must require both that a
    /// proxy is registered AND that its `EgressProxyReadiness` reports an active
    /// policy generation before permitting a workload to launch.
    pub(crate) fn readiness(&self, id: &SandboxId) -> Result<Option<EgressProxyReadiness>> {
        let proxies = self.lock()?;
        proxies
            .get(id)
            .map(|proxy| proxy.readiness().map_err(egress_proxy_error))
            .transpose()
    }

    /// True if a PEP is currently registered for `id`.
    #[cfg(test)]
    pub(crate) fn contains(&self, id: &SandboxId) -> Result<bool> {
        Ok(self.lock()?.contains_key(id))
    }

    /// Register an already-started proxy for `id` under test, so a readiness
    /// gate can be exercised against a not-ready (policy-less) PEP without a
    /// live VMM. Production code only ever registers a PEP through
    /// [`EgressProxyRegistry::ensure_running`], which always loads a policy.
    #[cfg(test)]
    pub(crate) fn insert_running_for_test(&self, id: &SandboxId, proxy: EgressProxy) -> Result<()> {
        self.lock()?.insert(id.clone(), proxy);
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, HashMap<SandboxId, EgressProxy>>> {
        self.proxies
            .lock()
            .map_err(|_| SandboxError::OperationFailed {
                message: "egress proxy registry lock is poisoned".to_owned(),
            })
    }
}

/// Map a `nimbus_proxy` error into a sandbox operation failure.
pub(crate) fn egress_proxy_error(error: EgressProxyError) -> SandboxError {
    SandboxError::OperationFailed {
        message: error.to_string(),
    }
}

/// Build the container-shape HTTP-proxy environment entries that point a sandbox
/// workload at its host-side egress PEP.
///
/// The shape (`HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY` plus the lowercase variants,
/// the Nimbus `EGRESS_PROXY_URL_ENV` handle, and an empty `NO_PROXY` so nothing
/// is exempt) is backend-agnostic, so both the container backend and the krun
/// microVM backend call this one helper instead of forking the env shape. The
/// caller is responsible for first scrubbing `EGRESS_RESERVED_ENV_KEYS` so a
/// tenant-supplied proxy override can never survive into the launched workload.
pub(crate) fn egress_proxy_env_entries(egress_proxy_url: &str) -> Vec<String> {
    [
        (EGRESS_PROXY_URL_ENV, egress_proxy_url),
        ("HTTP_PROXY", egress_proxy_url),
        ("http_proxy", egress_proxy_url),
        ("HTTPS_PROXY", egress_proxy_url),
        ("https_proxy", egress_proxy_url),
        ("ALL_PROXY", egress_proxy_url),
        ("all_proxy", egress_proxy_url),
        ("NO_PROXY", ""),
        ("no_proxy", ""),
    ]
    .into_iter()
    .map(|(key, value)| format!("{key}={value}"))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn loopback() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
    }

    #[test]
    fn ensure_running_registers_is_idempotent_and_stop_deregisters() {
        let registry = EgressProxyRegistry::new();
        let id = SandboxId::new("egress-seam-01");
        let policy = EgressPolicy::deny_all();

        assert!(!registry.contains(&id).unwrap());
        registry.ensure_running(&id, &policy, loopback()).unwrap();
        assert!(registry.contains(&id).unwrap());

        // idempotent: a second ensure neither errors nor double-registers
        registry.ensure_running(&id, &policy, loopback()).unwrap();
        assert!(registry.contains(&id).unwrap());

        registry.stop(&id).unwrap();
        assert!(!registry.contains(&id).unwrap());
        // stop is a no-op when nothing is registered
        registry.stop(&id).unwrap();
    }

    #[test]
    fn readiness_is_none_when_no_proxy_is_registered() {
        let registry = EgressProxyRegistry::new();
        let id = SandboxId::new("egress-seam-readiness-absent");

        assert!(
            registry
                .readiness(&id)
                .expect("readiness lookup should succeed")
                .is_none(),
            "an unregistered sandbox must report no PEP so the gate denies"
        );
    }

    #[test]
    fn readiness_reports_active_policy_for_a_running_proxy() {
        let registry = EgressProxyRegistry::new();
        let id = SandboxId::new("egress-seam-readiness-ready");
        registry
            .ensure_running(&id, &EgressPolicy::deny_all(), loopback())
            .unwrap();

        let readiness = registry
            .readiness(&id)
            .expect("readiness lookup should succeed")
            .expect("a registered proxy should report readiness");
        assert!(
            readiness.ready && readiness.policy_generation.is_some(),
            "a PEP started with a compiled policy must be ready with an active generation: {readiness:?}"
        );
    }

    #[test]
    fn readiness_reports_not_ready_for_a_policyless_proxy() {
        let registry = EgressProxyRegistry::new();
        let id = SandboxId::new("egress-seam-readiness-policyless");
        let proxy = EgressProxy::start(EgressProxyConfig::without_active_policy())
            .expect("a policy-less PEP should still bind and start");
        registry.insert_running_for_test(&id, proxy).unwrap();

        let readiness = registry
            .readiness(&id)
            .expect("readiness lookup should succeed")
            .expect("the registered proxy should report readiness");
        assert!(
            !readiness.ready && readiness.policy_generation.is_none(),
            "a PEP with no loaded policy must report not-ready so the gate denies: {readiness:?}"
        );
    }

    #[test]
    fn reload_fails_closed_when_no_proxy_is_running() {
        let registry = EgressProxyRegistry::new();
        let id = SandboxId::new("egress-seam-missing");
        let err = registry
            .reload(&id, CompiledEgressPolicy::deny_all())
            .unwrap_err();
        assert!(matches!(err, SandboxError::OperationFailed { .. }));
    }

    #[test]
    fn reload_updates_a_running_proxy() {
        let registry = EgressProxyRegistry::new();
        let id = SandboxId::new("egress-seam-reload");
        registry
            .ensure_running(&id, &EgressPolicy::deny_all(), loopback())
            .unwrap();
        registry
            .reload(&id, CompiledEgressPolicy::deny_all())
            .unwrap();
        registry.stop(&id).unwrap();
    }

    #[test]
    fn egress_proxy_env_entries_emit_container_shape_for_every_backend() {
        let entries = egress_proxy_env_entries("http://10.89.0.1:15000");

        for expected in [
            "NIMBUS_SANDBOX_EGRESS_PROXY_URL=http://10.89.0.1:15000",
            "HTTP_PROXY=http://10.89.0.1:15000",
            "http_proxy=http://10.89.0.1:15000",
            "HTTPS_PROXY=http://10.89.0.1:15000",
            "https_proxy=http://10.89.0.1:15000",
            "ALL_PROXY=http://10.89.0.1:15000",
            "all_proxy=http://10.89.0.1:15000",
            "NO_PROXY=",
            "no_proxy=",
        ] {
            assert!(
                entries.iter().any(|entry| entry == expected),
                "expected shared proxy env entry {expected:?} in {entries:?}"
            );
        }
        // NO_PROXY/no_proxy must stay empty so nothing is exempt from the PEP.
        assert!(
            entries
                .iter()
                .all(|entry| entry != "NO_PROXY=http://10.89.0.1:15000"),
            "NO_PROXY must remain empty so no destination bypasses the PEP: {entries:?}"
        );
    }
}
