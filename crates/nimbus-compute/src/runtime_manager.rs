use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nimbus_core::{Error, Result, TenantId};
use nimbus_engine::{Engine, TenantDeletionLease, TenantRuntimeLease};
use nimbus_runtime::{
    InvocationRequest, RuntimeBackendKind, RuntimeDeploymentAuthorityId,
    RuntimeDeploymentAuthorityLease, RuntimeDeploymentAuthorityLeaseIssuer,
    RuntimeDeploymentAuthorityRevocation, RuntimeExecutionModel, RuntimeExecutor,
    RuntimeInvocationContext, RuntimeLimits, RuntimeMetricsSnapshot, RuntimeOwnerClass,
    RuntimeOwnerId, RuntimeOwnerLease, RuntimeOwnerLeaseIssuer, RuntimeOwnerRevocation,
    RuntimePolicy, RuntimePoolKind, RuntimeProfile, RuntimeRetirementReport,
};
use serde::Serialize;

use crate::config::runtime::RuntimeGovernorConfig;

const RUNTIME_RETIREMENT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedLifecycle {
    Active,
    Retiring,
    Retired,
}

struct ManagedOwner {
    lifecycle: Mutex<ManagedLifecycle>,
    lease: RuntimeOwnerLease,
    revocation: RuntimeOwnerRevocation,
}

struct ManagedDeploymentAuthority {
    lifecycle: Mutex<ManagedLifecycle>,
    lease: RuntimeDeploymentAuthorityLease,
    revocation: RuntimeDeploymentAuthorityRevocation,
}

#[derive(Clone)]
pub struct RuntimeLaneHandle {
    requested_limits: RuntimeLimits,
    policy: Arc<RuntimePolicy>,
    executor: Arc<RuntimeExecutor>,
}

impl std::fmt::Debug for RuntimeLaneHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeLaneHandle")
            .field("requested_limits", &self.requested_limits)
            .finish_non_exhaustive()
    }
}

impl RuntimeLaneHandle {
    pub fn policy(&self) -> Arc<RuntimePolicy> {
        self.policy.clone()
    }

    pub fn executor(&self) -> Arc<RuntimeExecutor> {
        self.executor.clone()
    }

    pub fn requested_limits(&self) -> &RuntimeLimits {
        &self.requested_limits
    }
}

pub struct RuntimeManagerInvocationLease {
    engine_lease: TenantRuntimeLease,
    authority: RuntimeInvocationAuthority,
}

/// Cloneable runtime authority derived from an Engine-held invocation lease.
/// The owning [`RuntimeManagerInvocationLease`] retains the Engine operation
/// guard across active and background drain. Authority clones deliberately
/// carry only revocable runtime credentials so reusable host wrappers cannot
/// pin the Engine deletion fence after the invocation has completed.
#[derive(Clone)]
pub struct RuntimeInvocationAuthority {
    tenant_id: TenantId,
    owner_lease: RuntimeOwnerLease,
    deployment_lease: RuntimeDeploymentAuthorityLease,
}

impl std::fmt::Debug for RuntimeManagerInvocationLease {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeManagerInvocationLease")
            .field("tenant_id", self.engine_lease.tenant_id())
            .field("owner_id", self.authority.owner_lease.owner_id())
            .field(
                "deployment_authority_id",
                self.authority.deployment_lease.authority_id(),
            )
            .finish_non_exhaustive()
    }
}

impl RuntimeManagerInvocationLease {
    pub fn tenant_id(&self) -> &TenantId {
        self.engine_lease.tenant_id()
    }

    pub fn owner_lease(&self) -> RuntimeOwnerLease {
        self.authority.owner_lease.clone()
    }

    pub fn deployment_lease(&self) -> RuntimeDeploymentAuthorityLease {
        self.authority.deployment_lease.clone()
    }

    pub fn authority(&self) -> RuntimeInvocationAuthority {
        self.authority.clone()
    }

    pub fn invocation_context(
        &self,
        request: &InvocationRequest,
        server_request_id: Option<&str>,
        nested: bool,
        bypass_concurrency_limit: bool,
    ) -> RuntimeInvocationContext {
        self.authority.invocation_context(
            request,
            server_request_id,
            nested,
            bypass_concurrency_limit,
        )
    }
}

impl RuntimeInvocationAuthority {
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn invocation_context(
        &self,
        request: &InvocationRequest,
        server_request_id: Option<&str>,
        nested: bool,
        bypass_concurrency_limit: bool,
    ) -> RuntimeInvocationContext {
        let tenant_label = self.tenant_id.to_string();
        let context = match (nested, server_request_id) {
            (false, Some(server_request_id)) => {
                RuntimeInvocationContext::top_level_for_tenant_and_request_with_owner(
                    request,
                    tenant_label,
                    self.owner_lease.clone(),
                    server_request_id,
                )
            }
            (false, None) => RuntimeInvocationContext::top_level_for_tenant_with_owner(
                request,
                tenant_label,
                self.owner_lease.clone(),
            ),
            (true, Some(server_request_id)) => {
                RuntimeInvocationContext::nested_for_tenant_and_request_with_owner(
                    request,
                    tenant_label,
                    self.owner_lease.clone(),
                    server_request_id,
                )
            }
            (true, None) => RuntimeInvocationContext::nested_for_tenant_with_owner(
                request,
                tenant_label,
                self.owner_lease.clone(),
            ),
        }
        .with_deployment_authority(self.deployment_lease.clone());
        if bypass_concurrency_limit {
            context.with_bypassed_concurrency_limit()
        } else {
            context
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RuntimeOwnerClassDiagnostics {
    pub tenant: usize,
    pub system: usize,
    pub operator: usize,
    pub tooling: usize,
}

impl RuntimeOwnerClassDiagnostics {
    fn increment(&mut self, class: RuntimeOwnerClass) {
        match class {
            RuntimeOwnerClass::Tenant => self.tenant += 1,
            RuntimeOwnerClass::System => self.system += 1,
            RuntimeOwnerClass::Operator => self.operator += 1,
            RuntimeOwnerClass::Tooling => self.tooling += 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeManagerLaneDiagnostics {
    pub backend_kind: RuntimeBackendKind,
    pub profile: Option<RuntimeProfile>,
    pub execution_model: RuntimeExecutionModel,
    pub pool_kind: RuntimePoolKind,
    pub worker_threads: usize,
    pub max_retained_entries_per_worker: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeManagerDiagnostics {
    pub lanes: Vec<RuntimeManagerLaneDiagnostics>,
    pub active_owners: usize,
    pub retiring_owners: usize,
    pub active_owners_by_class: RuntimeOwnerClassDiagnostics,
    pub retiring_owners_by_class: RuntimeOwnerClassDiagnostics,
    pub active_deployment_authorities: usize,
    pub retiring_deployment_authorities: usize,
}

pub struct RuntimeManager {
    engine: Arc<Engine>,
    config: RuntimeGovernorConfig,
    lanes: Mutex<Vec<Arc<RuntimeLaneHandle>>>,
    owners: Mutex<HashMap<RuntimeOwnerId, Arc<ManagedOwner>>>,
    deployments: Mutex<HashMap<(RuntimeOwnerClass, u64), Arc<ManagedDeploymentAuthority>>>,
}

impl std::fmt::Debug for RuntimeManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeManager")
            .field("diagnostics", &self.diagnostics())
            .finish()
    }
}

impl RuntimeManager {
    pub fn new(engine: Arc<Engine>, config: RuntimeGovernorConfig) -> Arc<Self> {
        Arc::new(Self {
            engine,
            config,
            lanes: Mutex::new(Vec::new()),
            owners: Mutex::new(HashMap::new()),
            deployments: Mutex::new(HashMap::new()),
        })
    }

    pub fn base_runtime_limits(&self) -> &RuntimeLimits {
        self.config.base_runtime_limits()
    }

    pub fn lane_for_limits(&self, limits: RuntimeLimits) -> Arc<RuntimeLaneHandle> {
        let normalized = limits.normalized();
        let mut lanes = self
            .lanes
            .lock()
            .expect("runtime lane registry lock should not be poisoned");
        if let Some(lane) = lanes
            .iter()
            .find(|lane| lane.requested_limits == normalized)
        {
            return lane.clone();
        }
        let policy = self.config.policy_for_limits(normalized.clone());
        let lane = Arc::new(RuntimeLaneHandle {
            requested_limits: normalized,
            executor: Arc::new(RuntimeExecutor::new(policy.clone())),
            policy,
        });
        lanes.push(lane.clone());
        lane
    }

    pub fn metrics_snapshot_for_limits(&self, limits: &RuntimeLimits) -> RuntimeMetricsSnapshot {
        let normalized = limits.normalized();
        self.lanes
            .lock()
            .expect("runtime lane registry lock should not be poisoned")
            .iter()
            .find(|lane| lane.requested_limits == normalized)
            .map_or_else(RuntimeMetricsSnapshot::default, |lane| {
                lane.policy.metrics_snapshot()
            })
    }

    pub fn executor_started_for_limits(&self, limits: &RuntimeLimits) -> bool {
        self.metrics_snapshot_for_limits(limits)
            .worker_dispatched_invocations
            > 0
    }

    pub async fn acquire_invocation_lease(
        &self,
        tenant_id: TenantId,
        deployment_generation: u64,
    ) -> Result<RuntimeManagerInvocationLease> {
        let engine_lease = self.engine.enter_tenant_runtime_async(tenant_id).await?;
        self.lower_engine_lease(engine_lease, deployment_generation)
    }

    pub fn acquire_invocation_lease_blocking(
        &self,
        tenant_id: &TenantId,
        deployment_generation: u64,
    ) -> Result<RuntimeManagerInvocationLease> {
        let engine_lease = self.engine.enter_tenant_runtime(tenant_id)?;
        self.lower_engine_lease(engine_lease, deployment_generation)
    }

    fn lower_engine_lease(
        &self,
        engine_lease: TenantRuntimeLease,
        deployment_generation: u64,
    ) -> Result<RuntimeManagerInvocationLease> {
        let owner_id = runtime_owner_id(&engine_lease)?;
        let owner = {
            let mut owners = self
                .owners
                .lock()
                .expect("runtime owner registry lock should not be poisoned");
            owners
                .entry(owner_id.clone())
                .or_insert_with(|| {
                    let (lease, revocation) = RuntimeOwnerLeaseIssuer.issue(owner_id);
                    Arc::new(ManagedOwner {
                        lifecycle: Mutex::new(ManagedLifecycle::Active),
                        lease,
                        revocation,
                    })
                })
                .clone()
        };
        ensure_active(&owner.lifecycle, "runtime owner")?;
        let deployment =
            self.deployment_authority(owner.lease.owner_id().class(), deployment_generation)?;
        ensure_active(&deployment.lifecycle, "runtime deployment authority")?;
        let tenant_id = engine_lease.tenant_id().clone();
        Ok(RuntimeManagerInvocationLease {
            authority: RuntimeInvocationAuthority {
                tenant_id,
                owner_lease: owner.lease.clone(),
                deployment_lease: deployment.lease.clone(),
            },
            engine_lease,
        })
    }

    fn deployment_authority(
        &self,
        owner_class: RuntimeOwnerClass,
        generation: u64,
    ) -> Result<Arc<ManagedDeploymentAuthority>> {
        let authority_incarnation = generation
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or_else(|| {
                Error::ResourceExhausted("runtime deployment generation exhausted".to_owned())
            })?;
        let mut deployments = self
            .deployments
            .lock()
            .expect("runtime deployment registry lock should not be poisoned");
        Ok(deployments
            .entry((owner_class, generation))
            .or_insert_with(|| {
                let authority_id = RuntimeDeploymentAuthorityId::new(
                    deployment_authority_subject(owner_class),
                    authority_incarnation,
                )
                .expect("static deployment authority subject is valid");
                let (lease, revocation) = RuntimeDeploymentAuthorityLeaseIssuer.issue(authority_id);
                Arc::new(ManagedDeploymentAuthority {
                    lifecycle: Mutex::new(ManagedLifecycle::Active),
                    lease,
                    revocation,
                })
            })
            .clone())
    }

    /// Establishes the new deployment authority and revokes the previous one
    /// before the active deployment pointer is published. Old admitted work
    /// may drain, but no old retained state can be checked out or returned.
    pub fn rotate_deployment_authority(
        &self,
        previous_generation: u64,
        next_generation: u64,
    ) -> Result<()> {
        let next = self.deployment_authority(RuntimeOwnerClass::Tenant, next_generation)?;
        ensure_active(&next.lifecycle, "new runtime deployment authority")?;
        let previous = self
            .deployments
            .lock()
            .expect("runtime deployment registry lock should not be poisoned")
            .get(&(RuntimeOwnerClass::Tenant, previous_generation))
            .cloned();
        if let Some(previous) = previous
            && begin_retirement(&previous.lifecycle)
        {
            previous.revocation.revoke();
        }
        Ok(())
    }

    /// Revokes and drains the runtime owner for an Engine-fenced tenant
    /// incarnation.
    ///
    /// The deletion lease proves that Engine admission was closed before
    /// revocation began and keeps tenant reload/recreation serialized until
    /// the caller completes durable deletion.
    pub async fn retire_tenant_deletion(
        &self,
        deletion: &TenantDeletionLease<'_>,
    ) -> Result<(RuntimeOwnerId, Vec<RuntimeRetirementReport>)> {
        let owner_id =
            runtime_owner_id_from_parts(deletion.tenant_id(), deletion.tenant_incarnation())?;
        let owner = {
            let mut owners = self
                .owners
                .lock()
                .expect("runtime owner registry lock should not be poisoned");
            owners
                .entry(owner_id.clone())
                .or_insert_with(|| {
                    let (lease, revocation) = RuntimeOwnerLeaseIssuer.issue(owner_id.clone());
                    Arc::new(ManagedOwner {
                        lifecycle: Mutex::new(ManagedLifecycle::Active),
                        lease,
                        revocation,
                    })
                })
                .clone()
        };
        if !begin_retirement(&owner.lifecycle) {
            return Ok((owner_id, Vec::new()));
        }
        owner.revocation.revoke();
        let lanes = self
            .lanes
            .lock()
            .expect("runtime lane registry lock should not be poisoned")
            .clone();
        let mut reports = Vec::with_capacity(lanes.len());
        for lane in lanes {
            reports.push(
                lane.executor
                    .retire_owner(&owner.revocation, RUNTIME_RETIREMENT_TIMEOUT)
                    .await
                    .map_err(runtime_contract_error)?,
            );
        }
        *owner
            .lifecycle
            .lock()
            .expect("runtime owner lifecycle lock should not be poisoned") =
            ManagedLifecycle::Retired;
        Ok((owner_id, reports))
    }

    pub async fn retire_deployment_generation(
        &self,
        generation: u64,
    ) -> Result<Vec<RuntimeRetirementReport>> {
        let Some(deployment) = self
            .deployments
            .lock()
            .expect("runtime deployment registry lock should not be poisoned")
            .get(&(RuntimeOwnerClass::Tenant, generation))
            .cloned()
        else {
            return Ok(Vec::new());
        };
        if !begin_retirement(&deployment.lifecycle) {
            return Ok(Vec::new());
        }
        deployment.revocation.revoke();
        let lanes = self
            .lanes
            .lock()
            .expect("runtime lane registry lock should not be poisoned")
            .clone();
        let mut reports = Vec::with_capacity(lanes.len());
        for lane in lanes {
            reports.push(
                lane.executor
                    .retire_deployment_authority(&deployment.revocation, RUNTIME_RETIREMENT_TIMEOUT)
                    .await
                    .map_err(runtime_contract_error)?,
            );
        }
        *deployment
            .lifecycle
            .lock()
            .expect("runtime deployment lifecycle lock should not be poisoned") =
            ManagedLifecycle::Retired;
        Ok(reports)
    }

    pub fn forget_retired_owner(&self, owner_id: &RuntimeOwnerId) {
        let mut owners = self
            .owners
            .lock()
            .expect("runtime owner registry lock should not be poisoned");
        if owners.get(owner_id).is_some_and(|owner| {
            *owner
                .lifecycle
                .lock()
                .expect("runtime owner lifecycle lock should not be poisoned")
                == ManagedLifecycle::Retired
        }) {
            owners.remove(owner_id);
        }
    }

    pub fn diagnostics(&self) -> RuntimeManagerDiagnostics {
        let lanes = self
            .lanes
            .lock()
            .expect("runtime lane registry lock should not be poisoned")
            .iter()
            .map(|lane| RuntimeManagerLaneDiagnostics {
                backend_kind: lane.requested_limits.backend_kind,
                profile: lane.policy.runtime_profile(),
                execution_model: lane.requested_limits.execution_model,
                pool_kind: lane.requested_limits.runtime_pool_kind,
                worker_threads: lane.requested_limits.worker_threads,
                max_retained_entries_per_worker: lane
                    .requested_limits
                    .max_warm_pool_entries_per_worker,
            })
            .collect();
        let owners = self
            .owners
            .lock()
            .expect("runtime owner registry lock should not be poisoned");
        let mut active_owners = 0;
        let mut retiring_owners = 0;
        let mut active_owners_by_class = RuntimeOwnerClassDiagnostics::default();
        let mut retiring_owners_by_class = RuntimeOwnerClassDiagnostics::default();
        for owner in owners.values() {
            match *owner
                .lifecycle
                .lock()
                .expect("runtime owner lifecycle lock should not be poisoned")
            {
                ManagedLifecycle::Active => {
                    active_owners += 1;
                    active_owners_by_class.increment(owner.lease.owner_id().class());
                }
                ManagedLifecycle::Retiring => {
                    retiring_owners += 1;
                    retiring_owners_by_class.increment(owner.lease.owner_id().class());
                }
                ManagedLifecycle::Retired => {}
            }
        }
        let deployments = self
            .deployments
            .lock()
            .expect("runtime deployment registry lock should not be poisoned");
        let mut active_deployment_authorities = 0;
        let mut retiring_deployment_authorities = 0;
        for deployment in deployments.values() {
            match *deployment
                .lifecycle
                .lock()
                .expect("runtime deployment lifecycle lock should not be poisoned")
            {
                ManagedLifecycle::Active => active_deployment_authorities += 1,
                ManagedLifecycle::Retiring => retiring_deployment_authorities += 1,
                ManagedLifecycle::Retired => {}
            }
        }
        RuntimeManagerDiagnostics {
            lanes,
            active_owners,
            retiring_owners,
            active_owners_by_class,
            retiring_owners_by_class,
            active_deployment_authorities,
            retiring_deployment_authorities,
        }
    }
}

fn ensure_active(lifecycle: &Mutex<ManagedLifecycle>, subject: &str) -> Result<()> {
    if *lifecycle
        .lock()
        .expect("runtime managed lifecycle lock should not be poisoned")
        == ManagedLifecycle::Active
    {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "{subject} is retiring or retired"
    )))
}

fn begin_retirement(lifecycle: &Mutex<ManagedLifecycle>) -> bool {
    let mut lifecycle = lifecycle
        .lock()
        .expect("runtime managed lifecycle lock should not be poisoned");
    match *lifecycle {
        ManagedLifecycle::Active => {
            *lifecycle = ManagedLifecycle::Retiring;
            true
        }
        ManagedLifecycle::Retiring => true,
        ManagedLifecycle::Retired => false,
    }
}

fn runtime_contract_error(error: nimbus_runtime::NimbusRuntimeError) -> Error {
    Error::Internal(format!("runtime manager contract failed: {error}"))
}

const fn deployment_authority_subject(owner_class: RuntimeOwnerClass) -> &'static str {
    match owner_class {
        RuntimeOwnerClass::Tenant => "nimbus-application-deployment",
        RuntimeOwnerClass::System => "nimbus-system-deployment",
        RuntimeOwnerClass::Operator => "nimbus-operator-deployment",
        RuntimeOwnerClass::Tooling => "nimbus-tooling-deployment",
    }
}

fn runtime_owner_id(engine_lease: &TenantRuntimeLease) -> Result<RuntimeOwnerId> {
    runtime_owner_id_from_parts(engine_lease.tenant_id(), engine_lease.tenant_incarnation())
}

fn runtime_owner_id_from_parts(
    tenant_id: &TenantId,
    tenant_incarnation: NonZeroU64,
) -> Result<RuntimeOwnerId> {
    let subject = tenant_id.to_string();
    let owner = if tenant_id.as_str() == "_nimbus" {
        RuntimeOwnerId::system_session(subject.clone(), tenant_incarnation, Some(subject))
    } else {
        RuntimeOwnerId::tenant(subject.clone(), tenant_incarnation, Some(subject))
    };
    owner.map_err(runtime_contract_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_runtime::RuntimeOwnerClass;

    fn manager_fixture() -> (tempfile::TempDir, Arc<Engine>, Arc<RuntimeManager>) {
        let temp = tempfile::tempdir().expect("runtime manager tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let manager = RuntimeManager::new(engine.clone(), RuntimeGovernorConfig::default());
        (temp, engine, manager)
    }

    #[tokio::test]
    async fn tenant_owner_identity_comes_from_engine_and_changes_on_recreation() {
        let (_temp, engine, manager) = manager_fixture();
        let tenant_id = TenantId::new("runtime-manager-tenant").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");

        let original = manager
            .acquire_invocation_lease(tenant_id.clone(), 0)
            .await
            .expect("original runtime lease should build");
        let original_owner = original.owner_lease().owner_id().clone();
        assert_eq!(original_owner.class(), RuntimeOwnerClass::Tenant);
        assert_eq!(original_owner.stable_subject(), tenant_id.as_str());
        drop(original);

        let deletion = engine
            .begin_tenant_delete_async(tenant_id.clone())
            .await
            .expect("tenant deletion fence should begin");
        let (retired_owner, reports) = manager
            .retire_tenant_deletion(&deletion)
            .await
            .expect("tenant runtime owner should retire");
        assert_eq!(retired_owner, original_owner);
        assert!(
            reports.is_empty(),
            "unused manager should have no lanes to drain"
        );
        let rejected = manager
            .acquire_invocation_lease(tenant_id.clone(), 0)
            .await
            .expect_err("retired owner must reject new runtime admission");
        assert!(matches!(rejected, Error::TenantNotFound(ref id) if id == &tenant_id));

        engine
            .finish_tenant_delete_async(deletion)
            .await
            .expect("tenant should delete after runtime retirement");
        manager.forget_retired_owner(&retired_owner);
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("same tenant id should recreate");
        let recreated = manager
            .acquire_invocation_lease(tenant_id, 0)
            .await
            .expect("recreated runtime lease should build");
        let recreated_owner = recreated.owner_lease().owner_id().clone();
        assert_ne!(recreated_owner, original_owner);
        assert!(
            recreated_owner.incarnation() > original_owner.incarnation(),
            "Engine/storage must advance the canonical incarnation"
        );
    }

    #[tokio::test]
    async fn runtime_invocation_lease_holds_engine_delete_fence_through_background_drain() {
        let (_temp, engine, manager) = manager_fixture();
        let tenant_id =
            TenantId::new("runtime-manager-background-drain").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        let invocation = manager
            .acquire_invocation_lease(tenant_id.clone(), 0)
            .await
            .expect("runtime invocation lease should build");
        let deletion = engine
            .begin_tenant_delete_async(tenant_id)
            .await
            .expect("tenant deletion fence should begin");
        manager
            .retire_tenant_deletion(&deletion)
            .await
            .expect("runtime owner should retire");
        let mut finish = Box::pin(engine.finish_tenant_delete_async(deletion));
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut finish)
                .await
                .is_err(),
            "Engine deletion must wait while the runtime invocation lease holds background work"
        );

        drop(invocation);
        tokio::time::timeout(Duration::from_secs(10), finish)
            .await
            .expect("Engine deletion should unblock after background authority drains")
            .expect("tenant deletion should complete");
    }

    #[tokio::test]
    async fn system_tenant_uses_explicit_system_owner_class() {
        let (_temp, engine, manager) = manager_fixture();
        let tenant_id = TenantId::new("_nimbus").expect("system tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("system tenant should create");

        let lease = manager
            .acquire_invocation_lease(tenant_id.clone(), 0)
            .await
            .expect("system runtime lease should build");
        assert_eq!(
            lease.owner_lease().owner_id().class(),
            RuntimeOwnerClass::System
        );
        manager
            .rotate_deployment_authority(0, 1)
            .expect("application deployment authority should rotate");
        assert!(
            !lease.deployment_lease().is_revoked(),
            "application deploy rotation must not revoke the system registry authority"
        );
        manager
            .acquire_invocation_lease(tenant_id, 0)
            .await
            .expect("system generation zero should remain active after application rotation");
    }

    #[tokio::test]
    async fn deployment_generation_overflow_is_rejected_without_aliasing_authority() {
        let (_temp, engine, manager) = manager_fixture();
        let tenant_id =
            TenantId::new("runtime-manager-generation-overflow").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");

        let rejected = manager
            .acquire_invocation_lease(tenant_id, u64::MAX)
            .await
            .expect_err("maximum generation cannot map to a distinct nonzero incarnation");
        assert!(
            matches!(rejected, Error::ResourceExhausted(message) if message.contains("generation exhausted"))
        );
    }

    #[tokio::test]
    async fn lanes_are_canonical_and_deployment_generation_is_independent_authority() {
        let (_temp, engine, manager) = manager_fixture();
        let tenant_id = TenantId::new("runtime-manager-lanes").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");

        let limits = manager.base_runtime_limits().clone();
        let first_lane = manager.lane_for_limits(limits.clone());
        let same_lane = manager.lane_for_limits(limits.clone());
        assert!(Arc::ptr_eq(&first_lane, &same_lane));
        assert_eq!(first_lane.requested_limits(), &limits.normalized());

        let old = manager
            .acquire_invocation_lease(tenant_id.clone(), 41)
            .await
            .expect("old deployment lease should build");
        manager
            .rotate_deployment_authority(41, 42)
            .expect("deployment authority should rotate");
        assert!(old.deployment_lease().is_revoked());
        let new = manager
            .acquire_invocation_lease(tenant_id, 42)
            .await
            .expect("new deployment lease should build");
        assert_eq!(old.owner_lease(), new.owner_lease());
        assert_ne!(
            old.deployment_lease().authority_id(),
            new.deployment_lease().authority_id(),
            "byte-identical deployments still require distinct mutable-state authority"
        );
        let diagnostics = manager.diagnostics();
        assert_eq!(diagnostics.lanes.len(), 1);
        assert_eq!(
            diagnostics.lanes[0].profile,
            first_lane.policy().runtime_profile()
        );
        assert_eq!(diagnostics.active_owners_by_class.tenant, 1);
    }
}
