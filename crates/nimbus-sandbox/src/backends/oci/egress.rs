//! Shared per-sandbox egress PEP (Policy Enforcement Point) lifecycle.
//!
//! Every sandbox backend places its workload inside a deny-by-default network
//! namespace whose only outbound path is a host-side `nimbus_proxy::EgressProxy`
//! bound on the bridge gateway. The "compile policy -> start the PEP -> register
//! / reload / stop" glue is security-critical and identical across backends
//! (the container backend today, the krun microVM backend next), so it lives
//! here once instead of being forked per backend.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use nimbus_core::TenantId;
use nimbus_egress::{
    CompiledEgressPolicy, EGRESS_PROXY_URL_ENV, EGRESS_RESERVED_ENV_KEYS, EgressPolicy,
};
use nimbus_proxy::{
    AppendOnlyDecisionLogSink, DecisionLogSinkContext, EgressProxy, EgressProxyConfig,
    EgressProxyError, EgressProxyReadiness,
};
use serde::{Deserialize, Serialize};

use crate::backends::oci::network::{OciNetworkConfig, bridge_gateway_addr};
use crate::backends::oci::port_manager::PortManager;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

/// Registry of running per-sandbox egress proxies, shared by every sandbox
/// backend. Cloning shares the underlying registry (it is `Arc`-backed), so a
/// backend can hold one and hand clones to its async tasks.
#[derive(Clone)]
pub(crate) struct EgressProxyRegistry {
    proxies: Arc<Mutex<HashMap<SandboxId, EgressProxy>>>,
    decision_log_root: Arc<PathBuf>,
}

impl EgressProxyRegistry {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_decision_log_root(std::env::temp_dir().join("nimbus-egress-decision-logs"))
    }

    pub(crate) fn with_decision_log_root(decision_log_root: impl Into<PathBuf>) -> Self {
        Self {
            proxies: Arc::new(Mutex::new(HashMap::new())),
            decision_log_root: Arc::new(decision_log_root.into()),
        }
    }

    /// Ensure a PEP is running for `id`, bound on `bind_addr`.
    ///
    /// Idempotent: a no-op if a proxy is already registered for `id`.
    /// Fail-closed: a policy compile error or proxy start error returns `Err`
    /// and registers nothing — callers must treat that as deny.
    pub(crate) fn ensure_running(
        &self,
        tenant_id: &TenantId,
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
        let decision_log_path = self.decision_log_path(tenant_id, id);
        let decision_logger = AppendOnlyDecisionLogSink::open(
            &decision_log_path,
            DecisionLogSinkContext::new(tenant_id.as_str(), id.as_str()),
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to prepare append-only egress decision log for sandbox {id} at {}: {error}",
                decision_log_path.display()
            ),
        })?
        .logger();
        let proxy = EgressProxy::start(
            EgressProxyConfig::new(compiled)
                .with_bind_addr(bind_addr)
                .with_decision_logger(decision_logger),
        )
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

    #[cfg(test)]
    pub(crate) fn local_addr(&self, id: &SandboxId) -> Result<Option<SocketAddr>> {
        Ok(self.lock()?.get(id).map(EgressProxy::local_addr))
    }

    #[cfg(test)]
    pub(crate) fn decision_log_path_for_test(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
    ) -> PathBuf {
        self.decision_log_path(tenant_id, id)
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

    fn decision_log_path(&self, tenant_id: &TenantId, id: &SandboxId) -> PathBuf {
        self.decision_log_root
            .join(tenant_id.as_str())
            .join(format!("{}.jsonl", id.as_str()))
    }
}

pub(crate) fn egress_decision_log_root(state_root: &Path) -> PathBuf {
    state_root.join("egress-decision-logs")
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

/// Tier-neutral host-side egress PEP assignment for an execute-mode sandbox.
///
/// The proxy binds on the bridge gateway address so it is the only reachable
/// outbound path from inside the sandbox's deny-by-default network namespace.
/// Every sandbox backend (container today, krun microVM next) embeds an
/// `Option<EgressProxyAssignment>` in its persisted manifest and renders the
/// guest-facing proxy URL through [`EgressProxyAssignment::proxy_url`], so the
/// assignment shape and its IPv6-safe URL rendering live here once instead of
/// being forked per backend. (egress audit M9.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EgressProxyAssignment {
    pub(crate) host: String,
    pub(crate) port: u16,
}

impl EgressProxyAssignment {
    /// Bind address the PEP listens on. The host must be an IP literal (the
    /// bridge gateway), so a non-IP value fails closed as an invalid spec.
    pub(crate) fn bind_addr(&self) -> Result<SocketAddr> {
        let host = self
            .host
            .parse::<IpAddr>()
            .map_err(|_| SandboxError::InvalidSpec {
                message: format!("egress proxy host {:?} must be an IP address", self.host),
            })?;
        Ok(SocketAddr::new(host, self.port))
    }

    /// Container-shape proxy URL the guest env is pointed at. Rendered through
    /// [`SocketAddr`] so an IPv6 gateway is bracketed correctly
    /// (`http://[::1]:15000`, never the malformed `http://::1:15000`).
    pub(crate) fn proxy_url(&self) -> Result<String> {
        Ok(format!("http://{}", self.bind_addr()?))
    }
}

/// Assign a host-side egress PEP for an execute-mode launch: the proxy binds on
/// the bridge gateway address so it is the only outbound path reachable from
/// inside the sandbox's deny-by-default network namespace. Shared by every
/// sandbox backend so the gateway+port allocation is defined once.
pub(crate) fn allocate_egress_proxy(
    network_config: &OciNetworkConfig,
    port_manager: &PortManager,
    existing_bindings: &[SandboxPortBinding],
) -> Result<EgressProxyAssignment> {
    let gateway = bridge_gateway_addr(network_config)?;
    let port = port_manager.allocate_internal_host_port(existing_bindings)?;
    Ok(EgressProxyAssignment {
        host: gateway.to_string(),
        port,
    })
}

/// Start the host-side egress PEP for a sandbox on its assigned bridge-gateway
/// bind address. Fail-closed: a missing assignment or a proxy start error
/// returns `Err`, which every backend's launch path treats as deny. Shared so
/// the "no assignment means deny" invariant cannot drift between backends.
pub(crate) fn ensure_egress_proxy_running(
    registry: &EgressProxyRegistry,
    tenant_id: &TenantId,
    id: &SandboxId,
    assignment: Option<&EgressProxyAssignment>,
    policy: &EgressPolicy,
) -> Result<()> {
    let Some(assignment) = assignment else {
        return Err(SandboxError::OperationFailed {
            message: format!("sandbox {id} has no egress proxy assignment"),
        });
    };
    let bind_addr = assignment.bind_addr()?;
    registry.ensure_running(tenant_id, id, policy, bind_addr)
}

/// Extract the key (`KEY` of `KEY=VALUE`) of an OCI process env entry, or `None`
/// for a malformed or empty-key entry.
fn env_key(entry: &str) -> Option<&str> {
    let (key, _) = entry.split_once('=')?;
    (!key.is_empty()).then_some(key)
}

/// Scrub every reserved egress env key ([`EGRESS_RESERVED_ENV_KEYS`]) from a
/// process env vector so a tenant-supplied proxy override can never survive into
/// the launched workload.
///
/// This is the security companion of [`egress_proxy_env_entries`]: a backend
/// MUST scrub before injecting the PEP env so the two halves can never be
/// half-applied. Defined here once so both backends call the same scrub.
/// (egress audit L11.)
pub(crate) fn scrub_reserved_egress_env(env: &mut Vec<String>) {
    env.retain(|entry| env_key(entry).is_none_or(|key| !EGRESS_RESERVED_ENV_KEYS.contains(&key)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::Duration;

    fn loopback() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
    }

    fn tenant() -> TenantId {
        TenantId::new("tenant-egress").expect("test tenant id should be valid")
    }

    #[test]
    fn ensure_running_registers_is_idempotent_and_stop_deregisters() {
        let registry = EgressProxyRegistry::new();
        let tenant = tenant();
        let id = SandboxId::new("egress-seam-01");
        let policy = EgressPolicy::deny_all();

        assert!(!registry.contains(&id).unwrap());
        registry
            .ensure_running(&tenant, &id, &policy, loopback())
            .unwrap();
        assert!(registry.contains(&id).unwrap());

        // idempotent: a second ensure neither errors nor double-registers
        registry
            .ensure_running(&tenant, &id, &policy, loopback())
            .unwrap();
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
        let tenant = tenant();
        let id = SandboxId::new("egress-seam-readiness-ready");
        registry
            .ensure_running(&tenant, &id, &EgressPolicy::deny_all(), loopback())
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
        let tenant = tenant();
        let id = SandboxId::new("egress-seam-reload");
        registry
            .ensure_running(&tenant, &id, &EgressPolicy::deny_all(), loopback())
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

    #[test]
    fn egress_proxy_assignment_renders_ipv4_proxy_url() {
        let assignment = EgressProxyAssignment {
            host: "10.89.0.1".to_owned(),
            port: 15000,
        };
        assert_eq!(
            assignment.proxy_url().expect("ipv4 url renders"),
            "http://10.89.0.1:15000"
        );
        assert_eq!(
            assignment.bind_addr().expect("ipv4 bind addr"),
            "10.89.0.1:15000".parse().unwrap()
        );
    }

    #[test]
    fn egress_proxy_assignment_brackets_ipv6_proxy_url() {
        // Rendering through SocketAddr is what makes an IPv6 gateway safe: a raw
        // `format!("http://{host}:{port}")` would emit the malformed
        // `http://::1:15000`. Guard the IPv6-bracket behavior directly.
        let assignment = EgressProxyAssignment {
            host: "::1".to_owned(),
            port: 15000,
        };
        assert_eq!(
            assignment.proxy_url().expect("ipv6 url renders"),
            "http://[::1]:15000"
        );
    }

    #[test]
    fn egress_proxy_assignment_rejects_non_ip_host() {
        let assignment = EgressProxyAssignment {
            host: "gateway.example.com".to_owned(),
            port: 15000,
        };
        let error = assignment
            .bind_addr()
            .expect_err("a non-IP gateway host must fail closed");
        assert!(matches!(error, SandboxError::InvalidSpec { .. }));
        assert!(assignment.proxy_url().is_err());
    }

    #[test]
    fn ensure_egress_proxy_running_denies_when_assignment_absent() {
        let registry = EgressProxyRegistry::new();
        let tenant = tenant();
        let id = SandboxId::new("egress-no-assignment");
        let error =
            ensure_egress_proxy_running(&registry, &tenant, &id, None, &EgressPolicy::deny_all())
                .expect_err("a missing assignment must fail closed");
        assert!(matches!(error, SandboxError::OperationFailed { .. }));
        assert!(
            !registry.contains(&id).unwrap(),
            "a denied launch must register no PEP"
        );
    }

    #[test]
    fn ensure_egress_proxy_running_starts_pep_for_assignment() {
        let registry = EgressProxyRegistry::new();
        let tenant = tenant();
        let id = SandboxId::new("egress-with-assignment");
        let assignment = EgressProxyAssignment {
            host: "127.0.0.1".to_owned(),
            port: 0,
        };
        ensure_egress_proxy_running(
            &registry,
            &tenant,
            &id,
            Some(&assignment),
            &EgressPolicy::deny_all(),
        )
        .expect("a valid assignment should start the PEP");
        assert!(registry.contains(&id).unwrap());
        registry.stop(&id).unwrap();
    }

    #[test]
    fn live_sandbox_pep_path_uses_decision_logger_not_noop() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let registry = EgressProxyRegistry::with_decision_log_root(temp_dir.path().join("logs"));
        let tenant = tenant();
        let id = SandboxId::new("egress-live-audit");
        let assignment = EgressProxyAssignment {
            host: "127.0.0.1".to_owned(),
            port: 0,
        };

        ensure_egress_proxy_running(
            &registry,
            &tenant,
            &id,
            Some(&assignment),
            &EgressPolicy::deny_all(),
        )
        .expect("a valid assignment should start the audited PEP");
        let local_addr = registry
            .local_addr(&id)
            .expect("registry lookup should succeed")
            .expect("a PEP should be registered");
        let mut stream = TcpStream::connect(local_addr).expect("client should connect to PEP");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should set");
        stream
            .write_all(
                b"GET http://blocked.test:80/secret?token=supersecret HTTP/1.1\r\nHost: blocked.test\r\nAuthorization: Bearer topsecret\r\n\r\n",
            )
            .expect("client should write request");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("client should read response");
        assert!(
            response.starts_with("HTTP/1.1 403 Forbidden"),
            "deny-all live PEP should reject the request, got: {response}"
        );

        let log_path = registry.decision_log_path_for_test(&tenant, &id);
        let log_text = fs::read_to_string(&log_path).unwrap_or_else(|error| {
            panic!("decision log {} should read: {error}", log_path.display())
        });
        let lines = log_text.lines().collect::<Vec<_>>();
        assert_eq!(
            lines.len(),
            1,
            "live PEP decision_logger must emit exactly one terminal event, not use noop: {log_text:?}"
        );
        let event: serde_json::Value =
            serde_json::from_str(lines[0]).expect("decision log line should be JSON");
        assert_eq!(event["tenant_id"], tenant.as_str());
        assert_eq!(event["workload_id"], id.as_str());
        assert_eq!(event["policy_generation"], 1);
        assert_eq!(event["decision"], "deny");
        assert_eq!(event["reason_class"], "default_deny");
        assert_eq!(event["protocol"], "http");
        assert_eq!(event["canonical_host"], "blocked.test");
        assert_eq!(event["port"], 80);
        let rendered_event = event.to_string();
        assert!(
            rendered_event.contains("token=<redacted>")
                && !rendered_event.contains("supersecret")
                && !rendered_event.contains("topsecret"),
            "live audit event must redact query values and omit bearer tokens: {rendered_event}"
        );
        registry.stop(&id).unwrap();
    }

    #[test]
    fn scrub_reserved_egress_env_removes_only_reserved_keys_and_keeps_others() {
        let mut env = vec![
            "PATH=/usr/bin".to_owned(),
            "HTTP_PROXY=http://attacker:1".to_owned(),
            "https_proxy=http://attacker:2".to_owned(),
            "MALFORMED".to_owned(),
            "API_KEY=keep-me".to_owned(),
        ];
        scrub_reserved_egress_env(&mut env);

        for reserved in EGRESS_RESERVED_ENV_KEYS {
            assert!(
                !env.iter().any(|entry| env_key(entry) == Some(reserved)),
                "reserved key {reserved} must be scrubbed: {env:?}"
            );
        }
        assert!(
            env.contains(&"PATH=/usr/bin".to_owned())
                && env.contains(&"API_KEY=keep-me".to_owned())
                && env.contains(&"MALFORMED".to_owned()),
            "non-reserved entries must be preserved: {env:?}"
        );
    }
}
