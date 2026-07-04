//! Shared per-sandbox egress PEP (Policy Enforcement Point) lifecycle.
//!
//! Every sandbox backend places its workload inside a deny-by-default network
//! namespace whose only outbound path is a host-side `nimbus_proxy::WorkloadPep`
//! bound on the bridge gateway. The "compile policy -> start the PEP -> register
//! / reload / stop" glue is security-critical and identical across backends
//! (the container backend today, the krun microVM backend next), so it lives
//! here once instead of being forked per backend.

use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_core::{TenantId, WorkloadId};
use nimbus_egress::{
    CompiledEgressPolicy, EGRESS_CA_BUNDLE_ENV, EGRESS_NODE_EXTRA_CA_CERTS_ENV,
    EGRESS_PROXY_URL_ENV, EGRESS_RESERVED_ENV_KEYS, EgressPolicy,
};
use nimbus_proxy::{
    AppendOnlyDecisionLogSink, DecisionLogSinkContext, EgressEngine, EgressProxyError, WorkloadPep,
    WorkloadPepConfig, WorkloadPepReadiness, WorkloadPepTlsAuthority, fan_out_decision_loggers,
    tenant_decision_counter_sink,
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
///
/// The transport-clean lifecycle map lives in the node-scoped
/// [`nimbus_proxy::EgressEngine`], keyed by the opaque `nimbus-core`
/// [`WorkloadId`] (never `SandboxId`, so `nimbus-proxy` never depends on
/// `nimbus-sandbox`). This registry remains the sandbox-facing surface and
/// keeps the sandbox-layer publishing machinery — the trust-anchor writer, the
/// decision-log / trust-anchor roots, and path derivation — injecting its
/// published trust-anchor path as the engine entry's opaque attachment, so the
/// published CA file still lives in the same registry entry (under one lock)
/// as the PEP it belongs to.
#[derive(Clone)]
pub(crate) struct EgressProxyRegistry {
    engine: Arc<EgressEngine<RegisteredArtifacts>>,
    decision_log_root: Arc<PathBuf>,
    trust_anchor_root: Arc<PathBuf>,
}

/// Sandbox-owned per-registration artifacts riding the engine entry as its
/// opaque attachment: the published trust-anchor path (unwound at stop) and
/// the tenant fairness handle (pin released at stop so the node-wide fairness
/// map cannot grow monotonically).
struct RegisteredArtifacts {
    trust_anchor_path: Option<PathBuf>,
    /// RAII pin on the tenant's fairness entry; dropping it (at stop, or on
    /// any registration failure unwind) releases the pin — the registry
    /// evicts at zero leases, and capture+pin are atomic so no fork is
    /// possible.
    tenant_lease: nimbus_proxy::TenantLease,
}

impl EgressProxyRegistry {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::with_decision_log_root(std::env::temp_dir().join("nimbus-egress-decision-logs"))
    }

    #[cfg(test)]
    pub(crate) fn with_decision_log_root(decision_log_root: impl Into<PathBuf>) -> Self {
        let decision_log_root = decision_log_root.into();
        let trust_anchor_root = decision_log_root
            .parent()
            .map(|parent| parent.join("egress-trust-anchors"))
            .unwrap_or_else(|| PathBuf::from("egress-trust-anchors"));
        Self::with_roots(decision_log_root, trust_anchor_root)
    }

    pub(crate) fn with_roots(
        decision_log_root: impl Into<PathBuf>,
        trust_anchor_root: impl Into<PathBuf>,
    ) -> Self {
        Self {
            engine: Arc::new(EgressEngine::new()),
            decision_log_root: Arc::new(decision_log_root.into()),
            trust_anchor_root: Arc::new(trust_anchor_root.into()),
        }
    }

    /// Derive the engine's opaque workload id from a sandbox id.
    ///
    /// Fail-closed: `WorkloadId` rejects only the empty string, and an empty
    /// sandbox id is a spec bug — surfaced as `InvalidSpec`, never registered.
    fn workload_id(id: &SandboxId) -> Result<WorkloadId> {
        WorkloadId::new(id.as_str()).map_err(|_| SandboxError::InvalidSpec {
            message: "sandbox id cannot be empty (required for egress PEP registration)".to_owned(),
        })
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
        let decision_log_path = self.decision_log_path(tenant_id, id);
        let trust_anchor_path = self.trust_anchor_path(tenant_id, id);
        let workload_id = Self::workload_id(id)?;
        if self
            .engine
            .contains(&workload_id)
            .map_err(egress_proxy_error)?
        {
            return Ok(());
        }
        // Expensive preparation (policy compile, decision-log open, CA keypair
        // generation) runs outside the registry lock so one slow start cannot
        // stall reload/readiness/stop for every other sandbox.
        let compiled = policy
            .compile()
            .map_err(|message| SandboxError::InvalidSpec { message })?;
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
        let tls_authority =
            WorkloadPepTlsAuthority::generate_ephemeral().map_err(egress_proxy_error)?;
        // EE3/EE4: check out the tenant's fairness lease once, at
        // registration — capture+pin atomic; any failure below auto-releases
        // the pin via Drop (no zero-pin zombie entries).
        let tenant_lease = self.engine.fairness().checkout(tenant_id);
        let tenant_fairness = Arc::clone(tenant_lease.handle());
        // EE4: fan the decision stream out — the SELH append-only sink stays
        // FIRST (the durability baseline receives every event, unchanged);
        // the per-tenant counter sink subscribes behind it.
        let decision_logger = fan_out_decision_loggers(vec![
            decision_logger,
            tenant_decision_counter_sink(Arc::clone(&tenant_fairness)),
        ]);
        // Publish + register under one lock hold: the engine's registration
        // slot re-checks a concurrent winner first and holds the registry lock
        // until commit, so the trust-anchor file on disk is written by the same
        // call that registers its proxy — the file can never carry a different
        // CA than the PEP the workload is routed through.
        let Some(slot) = self
            .engine
            .try_reserve(workload_id)
            .map_err(egress_proxy_error)?
        else {
            return Ok(());
        };
        write_trust_anchor_file(
            &self.trust_anchor_root,
            &trust_anchor_path,
            &tls_authority.trust_anchor_pem(),
        )?;
        let proxy = WorkloadPep::start(
            WorkloadPepConfig::new(compiled)
                .with_bind_addr(bind_addr)
                .with_tls_authority(tls_authority)
                .with_decision_logger(decision_logger)
                // EE3: capture the tenant's fairness handle at registration —
                // the request path never looks tenants up.
                .with_tenant_fairness(Arc::clone(&tenant_fairness)),
        )
        .map_err(|error| {
            let _ = remove_trust_anchor_file(&trust_anchor_path);
            egress_proxy_error(error)
        })?;
        slot.commit(
            proxy,
            RegisteredArtifacts {
                trust_anchor_path: Some(trust_anchor_path),
                tenant_lease,
            },
        );
        Ok(())
    }

    /// Hot-reload the policy on the running PEP for `id`.
    ///
    /// Errors if no proxy is registered for `id` (the caller ensures it is
    /// running first). Fail-closed: a reload error is surfaced, not swallowed.
    pub(crate) fn reload(&self, id: &SandboxId, compiled: CompiledEgressPolicy) -> Result<()> {
        let workload_id = Self::workload_id(id)?;
        self.engine
            .with_pep(&workload_id, |pep| pep.reload_policy(compiled))
            .map_err(egress_proxy_error)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!("egress proxy for sandbox {id} is not running"),
            })?
            .map_err(egress_proxy_error)?;
        Ok(())
    }

    /// Stop and deregister the PEP for `id`. Dropping the `WorkloadPep` stops it.
    /// No-op if none is registered.
    pub(crate) fn stop(&self, id: &SandboxId) -> Result<()> {
        let workload_id = Self::workload_id(id)?;
        let removed = self
            .engine
            .deregister(&workload_id)
            .map_err(egress_proxy_error)?;
        let Some((proxy, artifacts)) = removed else {
            return Ok(());
        };
        // Stop the proxy before deleting its published trust anchor so a
        // still-running PEP is never left serving leaves the workload can no
        // longer verify.
        drop(proxy);
        // Lease drop releases the tenant's fairness pin (evicts at zero).
        drop(artifacts.tenant_lease);
        if let Some(trust_anchor_path) = artifacts.trust_anchor_path {
            remove_trust_anchor_file(&trust_anchor_path)?;
        }
        Ok(())
    }

    /// Report the readiness of the PEP registered for `id`.
    ///
    /// Returns `Ok(None)` when no proxy is registered (so the caller treats an
    /// absent PEP as deny), and `Ok(Some(readiness))` carrying the proxy's
    /// active-policy state otherwise. A readiness gate must require both that a
    /// proxy is registered AND that its `WorkloadPepReadiness` reports an active
    /// policy generation before permitting a workload to launch.
    pub(crate) fn readiness(&self, id: &SandboxId) -> Result<Option<WorkloadPepReadiness>> {
        let workload_id = Self::workload_id(id)?;
        self.engine
            .with_pep(&workload_id, |pep| pep.readiness())
            .map_err(egress_proxy_error)?
            .map(|readiness| readiness.map_err(egress_proxy_error))
            .transpose()
    }

    /// True if a PEP is currently registered for `id`.
    #[cfg(test)]
    pub(crate) fn contains(&self, id: &SandboxId) -> Result<bool> {
        let workload_id = Self::workload_id(id)?;
        self.engine
            .contains(&workload_id)
            .map_err(egress_proxy_error)
    }

    #[cfg(test)]
    pub(crate) fn local_addr(&self, id: &SandboxId) -> Result<Option<SocketAddr>> {
        let workload_id = Self::workload_id(id)?;
        self.engine
            .with_pep(&workload_id, |pep| pep.local_addr())
            .map_err(egress_proxy_error)
    }

    #[cfg(test)]
    pub(crate) fn decision_log_path_for_test(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
    ) -> PathBuf {
        self.decision_log_path(tenant_id, id)
    }

    #[cfg(test)]
    pub(crate) fn trust_anchor_path_for_test(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
    ) -> PathBuf {
        self.trust_anchor_path(tenant_id, id)
    }

    /// Register an already-started proxy for `id` under test, so a readiness
    /// gate can be exercised against a not-ready (policy-less) PEP without a
    /// live VMM. Production code only ever registers a PEP through
    /// [`EgressProxyRegistry::ensure_running`], which always loads a policy.
    #[cfg(test)]
    pub(crate) fn insert_running_for_test(&self, id: &SandboxId, proxy: WorkloadPep) -> Result<()> {
        let workload_id = Self::workload_id(id)?;
        // Preserve the old `insert` replace semantics: drop any prior entry
        // before reserving the slot for the replacement.
        let _ = self
            .engine
            .deregister(&workload_id)
            .map_err(egress_proxy_error)?;
        let slot = self
            .engine
            .try_reserve(workload_id)
            .map_err(egress_proxy_error)?
            .expect("slot must be free after deregistration");
        let tenant = TenantId::new("test-tenant").expect("static test tenant id");
        slot.commit(
            proxy,
            RegisteredArtifacts {
                trust_anchor_path: None,
                tenant_lease: self.engine.fairness().checkout(&tenant),
            },
        );
        Ok(())
    }

    fn decision_log_path(&self, tenant_id: &TenantId, id: &SandboxId) -> PathBuf {
        self.decision_log_root
            .join(tenant_id.as_str())
            .join(format!("{}.jsonl", id.as_str()))
    }

    fn trust_anchor_path(&self, tenant_id: &TenantId, id: &SandboxId) -> PathBuf {
        egress_trust_anchor_path(&self.trust_anchor_root, tenant_id, id)
    }
}

pub(crate) fn egress_decision_log_root(state_root: &Path) -> PathBuf {
    state_root.join("egress-decision-logs")
}

pub(crate) fn egress_trust_anchor_root(state_root: &Path) -> PathBuf {
    state_root.join("egress-trust-anchors")
}

pub(crate) const EGRESS_TRUST_ANCHOR_GUEST_PATH: &str = "/run/nimbus/egress/ca.pem";

const EGRESS_TRUST_ANCHOR_PLACEHOLDER: &str =
    "# Nimbus egress trust anchor placeholder; overwritten before launch\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EgressTrustAnchorMount {
    pub(crate) host_path: PathBuf,
    pub(crate) guest_path: String,
}

pub(crate) fn egress_trust_anchor_mount(
    state_root: &Path,
    tenant_id: &TenantId,
    id: &SandboxId,
) -> Result<EgressTrustAnchorMount> {
    let trust_anchor_root = egress_trust_anchor_root(state_root);
    let host_path = egress_trust_anchor_path(&trust_anchor_root, tenant_id, id);
    prepare_egress_trust_anchor_file(&trust_anchor_root, &host_path)?;
    Ok(EgressTrustAnchorMount {
        host_path,
        guest_path: EGRESS_TRUST_ANCHOR_GUEST_PATH.to_owned(),
    })
}

pub(crate) fn egress_trust_anchor_path(
    trust_anchor_root: &Path,
    tenant_id: &TenantId,
    id: &SandboxId,
) -> PathBuf {
    trust_anchor_root
        .join(tenant_id.as_str())
        .join(format!("{}.pem", id.as_str()))
}

pub(crate) fn prepare_egress_trust_anchor_file(
    trust_anchor_root: &Path,
    path: &Path,
) -> Result<()> {
    write_trust_anchor_file(trust_anchor_root, path, EGRESS_TRUST_ANCHOR_PLACEHOLDER)
}

/// Canonical trust-anchor writer: every trust-anchor byte that reaches disk
/// goes through here. The write is rooted (the target must sit inside
/// `trust_anchor_root` with no traversal components), permissioned explicitly
/// (0644 — the guest bind-mounts the file read-only and must be able to read
/// it), durable (file fsync before rename, directory fsync after), and atomic
/// (temp file in the target directory, then rename), so a concurrent reader —
/// including a guest that already bind-mounted the path — can never observe a
/// torn or half-written PEM.
///
/// Trust boundary: the root is host-owned Nimbus state that only this writer
/// populates, and the tenant/sandbox ids are validated, so the path guards are
/// defense-in-depth against a path-construction bug or a tampered/stale state
/// tree — not a sandbox against an attacker who already holds host write access
/// under our state dir.
fn write_trust_anchor_file(trust_anchor_root: &Path, path: &Path, contents: &str) -> Result<()> {
    validate_trust_anchor_path(trust_anchor_root, path)?;
    let parent = path.parent().ok_or_else(|| SandboxError::OperationFailed {
        message: format!(
            "egress trust-anchor path {} has no parent directory",
            path.display()
        ),
    })?;
    fs::create_dir_all(parent).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to create egress trust-anchor directory {}: {error}",
            parent.display()
        ),
    })?;
    // Tamper tripwire the lexical check can't provide: fail closed if any
    // directory component under the root is a symlink, so a tampered state tree
    // can never redirect where the anchor lands. The final-component write is
    // already symlink-safe on its own — `create_new` refuses a symlinked temp
    // path and `rename` replaces the destination name without following it — so
    // together no anchor bytes are ever written through a symlink.
    reject_symlinked_components(trust_anchor_root, parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "egress trust-anchor path {} has no file name",
                path.display()
            ),
        })?;
    static TEMP_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temp_path = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let write_result = (|| -> std::io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut temp_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&temp_path)?;
        temp_file.write_all(contents.as_bytes())?;
        temp_file.sync_all()?;
        fs::rename(&temp_path, path)?;
        // Make the rename itself durable: fsync the containing directory so a
        // crash cannot resurrect the old entry after the guest saw the new one.
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    write_result.map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        SandboxError::OperationFailed {
            message: format!(
                "failed to write egress trust anchor {}: {error}",
                path.display()
            ),
        }
    })
}

/// Fail closed if any path component from `trust_anchor_root` down to and
/// including `parent` is a symlink. `lstat`-based, so it never follows a link;
/// combined with the symlink-safe final-component write it guarantees no anchor
/// bytes are written through a symlinked directory. Callers pass a `parent`
/// already validated to sit under the root by [`validate_trust_anchor_path`].
fn reject_symlinked_components(trust_anchor_root: &Path, parent: &Path) -> Result<()> {
    let Ok(relative) = parent.strip_prefix(trust_anchor_root) else {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "egress trust-anchor directory {} is not under root {}",
                parent.display(),
                trust_anchor_root.display()
            ),
        });
    };
    let mut current = trust_anchor_root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        let metadata =
            fs::symlink_metadata(&current).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to stat egress trust-anchor component {}: {error}",
                    current.display()
                ),
            })?;
        if metadata.file_type().is_symlink() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "egress trust-anchor component {} is a symlink; refusing to write through it",
                    current.display()
                ),
            });
        }
    }
    Ok(())
}

/// Reject any trust-anchor target that does not sit strictly inside
/// `trust_anchor_root` via normal path components. The tenant/sandbox
/// identifiers that build these paths are already validated, so this is a
/// defense-in-depth boundary: no caller can turn the canonical writer into an
/// arbitrary-file write primitive.
fn validate_trust_anchor_path(trust_anchor_root: &Path, path: &Path) -> Result<()> {
    let invalid = || SandboxError::OperationFailed {
        message: format!(
            "egress trust-anchor path {} escapes trust-anchor root {}",
            path.display(),
            trust_anchor_root.display()
        ),
    };
    let relative = path
        .strip_prefix(trust_anchor_root)
        .map_err(|_| invalid())?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(invalid());
    }
    Ok(())
}

fn remove_trust_anchor_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to remove egress trust anchor {}: {error}",
                path.display()
            ),
        }),
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

/// Build trust-anchor env entries for workloads that are routed through a
/// host-side PEP capable of selective HTTPS interception.
pub(crate) fn egress_trust_anchor_env_entries(guest_path: &str) -> Vec<String> {
    [
        (EGRESS_CA_BUNDLE_ENV, guest_path),
        (EGRESS_NODE_EXTRA_CA_CERTS_ENV, guest_path),
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
        let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);

        assert!(!registry.contains(&id).unwrap());
        registry
            .ensure_running(&tenant, &id, &policy, loopback())
            .unwrap();
        assert!(registry.contains(&id).unwrap());
        assert!(
            trust_anchor_path.is_file(),
            "starting a PEP must publish a workload-scoped trust anchor"
        );

        // idempotent: a second ensure neither errors nor double-registers
        registry
            .ensure_running(&tenant, &id, &policy, loopback())
            .unwrap();
        assert!(registry.contains(&id).unwrap());

        registry.stop(&id).unwrap();
        assert!(!registry.contains(&id).unwrap());
        assert!(
            !trust_anchor_path.exists(),
            "stopping a PEP must clean up its workload-scoped trust anchor"
        );
        // stop is a no-op when nothing is registered
        registry.stop(&id).unwrap();
    }

    #[test]
    fn ensure_running_replaces_placeholder_with_public_trust_anchor_only() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let registry = EgressProxyRegistry::with_roots(
            temp_dir.path().join("logs"),
            temp_dir.path().join("trust"),
        );
        let tenant = tenant();
        let id = SandboxId::new("egress-trust-anchor");
        let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);
        prepare_egress_trust_anchor_file(&temp_dir.path().join("trust"), &trust_anchor_path)
            .expect("planning should materialize a placeholder trust-anchor file");
        assert!(
            fs::read_to_string(&trust_anchor_path)
                .expect("placeholder should read")
                .contains("placeholder"),
            "planning should create a placeholder at the deterministic mount source"
        );

        registry
            .ensure_running(&tenant, &id, &EgressPolicy::deny_all(), loopback())
            .expect("starting a PEP should publish the real public trust anchor");

        let pem = fs::read_to_string(&trust_anchor_path).expect("trust anchor should read");
        assert!(
            pem.contains("-----BEGIN CERTIFICATE-----")
                && pem.contains("-----END CERTIFICATE-----")
                && !pem.contains("PRIVATE KEY")
                && !pem.contains("placeholder"),
            "workloads must receive only the public CA certificate, never the private key or stale placeholder: {pem}"
        );
        registry.stop(&id).expect("PEP stop should clean up");
        assert!(
            !trust_anchor_path.exists(),
            "trust-anchor cleanup should remove the workload-scoped CA file"
        );
    }

    #[test]
    fn distinct_sandboxes_receive_distinct_ephemeral_cas() {
        // Cross-sandbox isolation invariant: the registry must publish a
        // DIFFERENT ephemeral CA per sandbox. A shared/centralized CA would be a
        // cross-tenant MITM blast radius — the property that distinguishes our
        // per-sandbox PEP from the shared-CA gateway designs.
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let registry = EgressProxyRegistry::with_roots(
            temp_dir.path().join("logs"),
            temp_dir.path().join("trust"),
        );
        let tenant = tenant();
        let first = SandboxId::new("egress-ca-a");
        let second = SandboxId::new("egress-ca-b");
        registry
            .ensure_running(&tenant, &first, &EgressPolicy::deny_all(), loopback())
            .expect("first PEP should start");
        registry
            .ensure_running(&tenant, &second, &EgressPolicy::deny_all(), loopback())
            .expect("second PEP should start");

        let first_pem = fs::read_to_string(registry.trust_anchor_path_for_test(&tenant, &first))
            .expect("first trust anchor should read");
        let second_pem = fs::read_to_string(registry.trust_anchor_path_for_test(&tenant, &second))
            .expect("second trust anchor should read");
        assert!(
            first_pem.contains("-----BEGIN CERTIFICATE-----")
                && !first_pem.contains("PRIVATE KEY")
                && !second_pem.contains("PRIVATE KEY"),
            "each published anchor must be public-cert-only"
        );
        assert_ne!(
            first_pem, second_pem,
            "two sandboxes must receive distinct ephemeral CAs, never a shared one"
        );
        registry.stop(&first).expect("first stop");
        registry.stop(&second).expect("second stop");
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
        let proxy = WorkloadPep::start(WorkloadPepConfig::without_active_policy())
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
    fn egress_trust_anchor_env_entries_emit_additive_trust_shape() {
        let entries = egress_trust_anchor_env_entries(EGRESS_TRUST_ANCHOR_GUEST_PATH);

        for expected in [
            format!("{EGRESS_CA_BUNDLE_ENV}={EGRESS_TRUST_ANCHOR_GUEST_PATH}"),
            format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}={EGRESS_TRUST_ANCHOR_GUEST_PATH}"),
        ] {
            assert!(
                entries.contains(&expected),
                "expected shared trust-anchor env entry {expected:?} in {entries:?}"
            );
        }
        assert!(
            entries
                .iter()
                .all(|entry| !entry.starts_with("SSL_CERT_FILE=")
                    && !entry.starts_with("CURL_CA_BUNDLE=")
                    && !entry.starts_with("REQUESTS_CA_BUNDLE=")),
            "trust env must be additive and not replace system roots: {entries:?}"
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
    fn trust_anchor_writer_rejects_paths_outside_root() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let root = temp_dir.path().join("trust");

        for escape in [
            temp_dir.path().join("elsewhere/ca.pem"),
            root.join("../escaped.pem"),
            root.join("tenant/../../escaped.pem"),
            root.clone(),
        ] {
            let error = prepare_egress_trust_anchor_file(&root, &escape)
                .expect_err("a target outside the trust-anchor root must fail closed");
            assert!(
                matches!(error, SandboxError::OperationFailed { .. }),
                "path {} must be rejected",
                escape.display()
            );
            assert!(
                !escape.is_file(),
                "no file may be created at the rejected target {}",
                escape.display()
            );
        }
    }

    #[test]
    fn trust_anchor_writer_rejects_symlinked_directory_escape() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let root = temp_dir.path().join("trust");
        let outside = temp_dir.path().join("outside");
        fs::create_dir_all(&root).expect("trust root should create");
        fs::create_dir_all(&outside).expect("outside dir should create");
        // A tenant directory under the root is a symlink pointing outside it:
        // the lexical component check passes, but the canonical parent escapes.
        symlink(&outside, root.join("tenant-a")).expect("symlink should create");

        let escaped = root.join("tenant-a/sandbox-a.pem");
        let error = prepare_egress_trust_anchor_file(&root, &escaped)
            .expect_err("a symlinked directory escape must fail closed");
        assert!(matches!(error, SandboxError::OperationFailed { .. }));
        assert!(
            !outside.join("sandbox-a.pem").exists(),
            "no trust-anchor bytes may be written through an escaping symlink"
        );
    }

    #[test]
    fn trust_anchor_writer_publishes_atomically_with_explicit_permissions() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let root = temp_dir.path().join("trust");
        let path = root.join("tenant-a/sandbox-a.pem");

        prepare_egress_trust_anchor_file(&root, &path).expect("placeholder should publish");
        // Overwrite (placeholder -> real content) must also go through the
        // temp+rename path and leave no temp residue beside the target.
        prepare_egress_trust_anchor_file(&root, &path).expect("re-publish should succeed");

        let entries: Vec<String> = fs::read_dir(path.parent().unwrap())
            .expect("trust-anchor directory should list")
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            entries,
            vec!["sandbox-a.pem".to_owned()],
            "the writer must leave only the published file, no temp residue"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path)
                .expect("published trust anchor should stat")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(
                mode, 0o644,
                "trust anchor must be world-readable for the guest bind mount and writable only by the owner"
            );
        }
    }

    #[test]
    fn ensure_running_fails_closed_when_trust_anchor_root_is_unwritable() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let trust_root = temp_dir.path().join("trust");
        fs::create_dir_all(&trust_root).expect("trust root should create");
        fs::set_permissions(&trust_root, fs::Permissions::from_mode(0o555))
            .expect("trust root should become read-only");

        let registry =
            EgressProxyRegistry::with_roots(temp_dir.path().join("logs"), trust_root.clone());
        let tenant = tenant();
        let id = SandboxId::new("egress-unwritable-trust-root");
        let result = registry.ensure_running(&tenant, &id, &EgressPolicy::deny_all(), loopback());

        // Restore permissions before asserting so TempDir cleanup succeeds
        // even if an assertion fails.
        fs::set_permissions(&trust_root, fs::Permissions::from_mode(0o755))
            .expect("trust root permissions should restore");

        let error = result.expect_err("an unwritable trust-anchor root must fail the PEP start");
        assert!(matches!(error, SandboxError::OperationFailed { .. }));
        assert!(
            !registry.contains(&id).unwrap(),
            "a failed trust-anchor publish must register no PEP"
        );
    }

    #[test]
    fn concurrent_ensure_running_registers_exactly_one_pep() {
        let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
        let registry = EgressProxyRegistry::with_roots(
            temp_dir.path().join("logs"),
            temp_dir.path().join("trust"),
        );
        let tenant = tenant();
        let id = SandboxId::new("egress-concurrent-start");
        let trust_anchor_path = registry.trust_anchor_path_for_test(&tenant, &id);

        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    let registry = registry.clone();
                    let tenant = tenant.clone();
                    let id = id.clone();
                    scope.spawn(move || {
                        registry.ensure_running(&tenant, &id, &EgressPolicy::deny_all(), loopback())
                    })
                })
                .collect();
            for handle in handles {
                handle
                    .join()
                    .expect("ensure_running thread should not panic")
                    .expect("every concurrent ensure_running must succeed");
            }
        });

        assert!(registry.contains(&id).unwrap());
        let pem = fs::read_to_string(&trust_anchor_path)
            .expect("the winning PEP must have published its trust anchor");
        assert!(
            pem.contains("-----BEGIN CERTIFICATE-----") && !pem.contains("PRIVATE KEY"),
            "published trust anchor must be the public certificate: {pem}"
        );
        registry.stop(&id).expect("PEP stop should clean up");
        assert!(
            !trust_anchor_path.exists(),
            "stop must remove the published trust anchor"
        );
    }

    #[test]
    fn scrub_reserved_egress_env_removes_only_reserved_keys_and_keeps_others() {
        let mut env = vec![
            "PATH=/usr/bin".to_owned(),
            "HTTP_PROXY=http://attacker:1".to_owned(),
            "https_proxy=http://attacker:2".to_owned(),
            format!("{EGRESS_CA_BUNDLE_ENV}=/tmp/attacker-ca.pem"),
            format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}=/tmp/attacker-node-ca.pem"),
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
