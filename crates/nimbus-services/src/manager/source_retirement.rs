//! Process-local source fencing and terminal projection for native retirement.
//!
//! This module owns no provider effect and no durable lifecycle transition.
//! Compute presents exact saga facts; services fences its source bytes until
//! compute proves that the corresponding teardown reached `Recorded`.

use nimbus_core::{Error, TenantId};
use nimbus_sandbox::SandboxStatus;
use nimbus_workloads::{
    DesiredWorkloadState, WorkloadGeneration, WorkloadProvisionSourceKind, WorkloadSagaPhase,
    WorkloadSagaRecord, WorkloadSagaRevision,
};

use crate::{
    SandboxResourceSnapshot, ServiceBackend, ServiceDefinition, ServiceDefinitionSource,
    SessionLifecycleState, SessionTarget,
};

use super::ServiceManager;
use super::clock::now_millis;
use super::session_channels::close_session_channels;
use super::types::{
    ServiceManagerState, TenantSandboxResourceKey, TenantServiceKey, WorkloadSourceRetirementKey,
};

/// Stable services-owned identity of one native desired source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkloadSourceRetirementIdentity {
    SandboxBackedService { name: String },
    StandaloneSandbox { id: String },
}

/// Source-policy operation fenced by one claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadSourceRetirementOperation {
    Stop,
    DeleteDefinition { force: bool },
}

/// Exact process-local claim that compute can replay but cannot weaken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadSourceRetirementClaim {
    tenant_id: TenantId,
    identity: WorkloadSourceRetirementIdentity,
    source_generation: u64,
    resource_version: String,
    operation: WorkloadSourceRetirementOperation,
    saga_generation: WorkloadGeneration,
    saga_revision: WorkloadSagaRevision,
    captured_session_ids: Vec<String>,
}

impl WorkloadSourceRetirementClaim {
    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn identity(&self) -> &WorkloadSourceRetirementIdentity {
        &self.identity
    }

    pub const fn source_generation(&self) -> u64 {
        self.source_generation
    }

    pub fn resource_version(&self) -> &str {
        &self.resource_version
    }

    pub const fn operation(&self) -> WorkloadSourceRetirementOperation {
        self.operation
    }

    pub const fn saga_generation(&self) -> WorkloadGeneration {
        self.saga_generation
    }

    pub const fn saga_revision(&self) -> WorkloadSagaRevision {
        self.saga_revision
    }
}

impl ServiceManager {
    /// Delete a dynamic definition that has no sandbox workload lifecycle.
    /// Built-in and external definitions need only the services-owned source
    /// and session transaction; they cannot invoke a workload provider.
    pub fn finalize_unmanaged_service_definition_deletion(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        expected_generation: u64,
        force: bool,
    ) -> Result<ServiceDefinition, Error> {
        if self
            .service_definitions
            .service_definition_for_tenant(tenant_id, service_name)
            .is_some()
        {
            return Err(Error::conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` is static and cannot be deleted through dynamic service definition routes"
            )));
        }
        let key = TenantServiceKey::new(tenant_id, service_name);
        let retirement_key = WorkloadSourceRetirementKey::Service(key.clone());
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        if state.source_retirement_claims.contains_key(&retirement_key) {
            return Err(Error::conflict(
                "service has a retirement claim in progress".to_owned(),
            ));
        }
        let definition = state.definitions.get(&key).cloned().ok_or_else(|| {
            Error::NotFound(format!(
                "service `{service_name}` for tenant `{tenant_id}` was not found"
            ))
        })?;
        if definition.source != ServiceDefinitionSource::Dynamic {
            return Err(Error::conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` is static and cannot be deleted"
            )));
        }
        if matches!(definition.backend, ServiceBackend::Sandbox(_)) {
            return Err(Error::PreconditionFailed(
                "sandbox-backed definition deletion requires durable workload retirement"
                    .to_owned(),
            ));
        }
        if definition.generation != expected_generation {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` has generation {}, but delete expected {expected_generation}",
                definition.generation
            )));
        }
        let session_ids = service_session_ids(&state, tenant_id, service_name);
        let has_open = session_ids.iter().any(|id| {
            state.sessions.get(id).is_some_and(|session| {
                session.lifecycle_state == SessionLifecycleState::Open
                    && now_millis() < session.expires_at_millis
            })
        });
        if has_open && !force {
            return Err(Error::conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` has open sessions; close sessions first or use authorized force deletion"
            )));
        }
        let removed = state.definitions.remove(&key).ok_or_else(|| {
            Error::NotFound(format!(
                "service `{service_name}` for tenant `{tenant_id}` was not found"
            ))
        })?;
        if force {
            for session_id in &session_ids {
                let should_close = state
                    .sessions
                    .get(session_id)
                    .is_some_and(|session| session.lifecycle_state == SessionLifecycleState::Open);
                if should_close {
                    let now = now_millis();
                    if let Some(session) = state.sessions.get_mut(session_id) {
                        session.lifecycle_state = SessionLifecycleState::Closed;
                        session.closed_at_millis = Some(now);
                        session.updated_at_millis = now;
                        session.close_reason = Some("service_force_deleted".to_owned());
                    }
                    close_session_channels(&mut state, session_id, "service_force_deleted");
                }
            }
        }
        Ok(removed)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the source claim binds each independent source and saga fence"
    )]
    pub fn claim_service_definition_retirement(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        source_generation: u64,
        resource_version: &str,
        operation: WorkloadSourceRetirementOperation,
        saga_generation: WorkloadGeneration,
        saga_revision: WorkloadSagaRevision,
    ) -> Result<WorkloadSourceRetirementClaim, Error> {
        let catalog_definition = self
            .service_definitions
            .service_definition_for_tenant(tenant_id, service_name);
        let key = TenantServiceKey::new(tenant_id, service_name);
        let retirement_key = WorkloadSourceRetirementKey::Service(key.clone());
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let definition = state
            .definitions
            .get(&key)
            .or(catalog_definition.as_ref())
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "service `{service_name}` for tenant `{tenant_id}` was not found"
                ))
            })?;
        let ServiceBackend::Sandbox(_) = &definition.backend else {
            return Err(Error::InvalidInput(format!(
                "service `{service_name}` for tenant `{tenant_id}` is not sandbox-backed"
            )));
        };
        if definition.generation != source_generation
            || definition.resource_version != resource_version
        {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` changed before retirement source claim"
            )));
        }
        if matches!(
            operation,
            WorkloadSourceRetirementOperation::DeleteDefinition { .. }
        ) && definition.source != ServiceDefinitionSource::Dynamic
        {
            return Err(Error::conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` is static and cannot be deleted"
            )));
        }
        let captured_session_ids = service_session_ids(&state, tenant_id, service_name);
        if matches!(
            operation,
            WorkloadSourceRetirementOperation::DeleteDefinition { force: false }
        ) && captured_session_ids.iter().any(|id| {
            state.sessions.get(id).is_some_and(|session| {
                session.lifecycle_state == SessionLifecycleState::Open
                    && now_millis() < session.expires_at_millis
            })
        }) {
            return Err(Error::conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` has open sessions; close sessions first or use authorized force deletion"
            )));
        }
        let claim = WorkloadSourceRetirementClaim {
            tenant_id: tenant_id.clone(),
            identity: WorkloadSourceRetirementIdentity::SandboxBackedService {
                name: service_name.to_owned(),
            },
            source_generation,
            resource_version: resource_version.to_owned(),
            operation,
            saga_generation,
            saga_revision,
            captured_session_ids,
        };
        insert_or_replay_claim(&mut state.source_retirement_claims, retirement_key, claim)
    }

    pub fn claim_standalone_sandbox_retirement(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &str,
        source_generation: u64,
        resource_version: &str,
        saga_generation: WorkloadGeneration,
        saga_revision: WorkloadSagaRevision,
    ) -> Result<WorkloadSourceRetirementClaim, Error> {
        let key = TenantSandboxResourceKey::new(tenant_id, sandbox_id);
        let retirement_key = WorkloadSourceRetirementKey::Sandbox(key.clone());
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let source = state.sandbox_resource_sources.get(&key).ok_or_else(|| {
            Error::NotFound(format!(
                "sandbox `{sandbox_id}` was not found for tenant `{tenant_id}`"
            ))
        })?;
        if source.generation != source_generation || source.resource_version != resource_version {
            return Err(Error::PreconditionFailed(format!(
                "sandbox `{sandbox_id}` changed before retirement source claim"
            )));
        }
        let claim = WorkloadSourceRetirementClaim {
            tenant_id: tenant_id.clone(),
            identity: WorkloadSourceRetirementIdentity::StandaloneSandbox {
                id: sandbox_id.to_owned(),
            },
            source_generation,
            resource_version: resource_version.to_owned(),
            operation: WorkloadSourceRetirementOperation::Stop,
            saga_generation,
            saga_revision,
            captured_session_ids: Vec::new(),
        };
        insert_or_replay_claim(&mut state.source_retirement_claims, retirement_key, claim)
    }

    /// Advance only the saga fence after compute joins an in-flight provision.
    /// The source identity, source bytes, and operation remain immutable.
    pub fn advance_source_retirement_claim_saga_fence(
        &self,
        claim: &WorkloadSourceRetirementClaim,
        saga_generation: WorkloadGeneration,
        saga_revision: WorkloadSagaRevision,
    ) -> Result<WorkloadSourceRetirementClaim, Error> {
        if saga_generation < claim.saga_generation
            || saga_generation == claim.saga_generation && saga_revision < claim.saga_revision
        {
            return Err(Error::PreconditionFailed(format!(
                "source retirement saga fence cannot move backward from generation {:?} revision {:?} to generation {:?} revision {:?}",
                claim.saga_generation, claim.saga_revision, saga_generation, saga_revision
            )));
        }
        let key = retirement_key(claim);
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        authenticate_claim(&state.source_retirement_claims, &key, claim)?;
        let mut advanced = claim.clone();
        advanced.saga_generation = saga_generation;
        advanced.saga_revision = saga_revision;
        state.source_retirement_claims.insert(key, advanced.clone());
        Ok(advanced)
    }

    /// Release only the lower-bound claim installed before compute's first
    /// saga read. This is valid for a deterministic preflight rejection before
    /// retirement changes durable truth or invokes a provider.
    pub fn release_unadvanced_source_retirement_claim(
        &self,
        claim: &WorkloadSourceRetirementClaim,
    ) -> Result<(), Error> {
        authenticate_unadvanced_source_retirement_claim(claim)?;
        let key = retirement_key(claim);
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        authenticate_claim(&state.source_retirement_claims, &key, claim)?;
        state.source_retirement_claims.remove(&key);
        Ok(())
    }

    /// Complete a stopped source claim that has no saga or provider evidence.
    /// This removes only the process-local fence and does not mutate desire.
    pub fn finalize_unstarted_source_stop(
        &self,
        claim: &WorkloadSourceRetirementClaim,
    ) -> Result<(), Error> {
        if claim.operation != WorkloadSourceRetirementOperation::Stop {
            return Err(Error::InvalidInput(
                "unstarted source stop cannot finalize a definition-deletion claim".to_owned(),
            ));
        }
        authenticate_unadvanced_source_retirement_claim(claim)?;
        let key = retirement_key(claim);
        let catalog_definition = match claim.identity() {
            WorkloadSourceRetirementIdentity::SandboxBackedService { name } => self
                .service_definitions
                .service_definition_for_tenant(&claim.tenant_id, name),
            WorkloadSourceRetirementIdentity::StandaloneSandbox { .. } => None,
        };
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        authenticate_claim(&state.source_retirement_claims, &key, claim)?;
        match claim.identity() {
            WorkloadSourceRetirementIdentity::SandboxBackedService { name } => {
                let source_key = TenantServiceKey::new(&claim.tenant_id, name);
                let definition = state
                    .definitions
                    .get(&source_key)
                    .or(catalog_definition.as_ref())
                    .ok_or_else(|| {
                        Error::NotFound(format!(
                            "service `{name}` for tenant `{}` was removed before unstarted stop finalization",
                            claim.tenant_id
                        ))
                    })?;
                authenticate_service_source(definition, claim)?;
                if state
                    .service_definition_observations
                    .contains_key(&source_key)
                {
                    return Err(Error::PreconditionFailed(
                        "unstarted service stop found a provider observation without a saga"
                            .to_owned(),
                    ));
                }
            }
            WorkloadSourceRetirementIdentity::StandaloneSandbox { id } => {
                let source_key = TenantSandboxResourceKey::new(&claim.tenant_id, id);
                let source = state
                    .sandbox_resource_sources
                    .get(&source_key)
                    .ok_or_else(|| {
                        Error::NotFound(format!(
                            "sandbox `{id}` for tenant `{}` was removed before unstarted stop finalization",
                            claim.tenant_id
                        ))
                    })?;
                if source.generation != claim.source_generation
                    || source.resource_version != claim.resource_version
                {
                    return Err(Error::PreconditionFailed(
                        "sandbox source changed before unstarted stop finalization".to_owned(),
                    ));
                }
                if state
                    .sandbox_resource_observations
                    .contains_key(&source_key)
                {
                    return Err(Error::PreconditionFailed(
                        "unstarted sandbox stop found a provider observation without a saga"
                            .to_owned(),
                    ));
                }
            }
        }
        state.source_retirement_claims.remove(&key);
        Ok(())
    }

    /// Delete a dynamic definition that never acquired saga or provider
    /// evidence. This consumes the exact source claim atomically.
    pub fn finalize_unstarted_service_definition_deletion(
        &self,
        claim: &WorkloadSourceRetirementClaim,
    ) -> Result<ServiceDefinition, Error> {
        let WorkloadSourceRetirementIdentity::SandboxBackedService { name } = claim.identity()
        else {
            return Err(Error::InvalidInput(
                "unstarted definition deletion requires a service source claim".to_owned(),
            ));
        };
        let WorkloadSourceRetirementOperation::DeleteDefinition { force } = claim.operation else {
            return Err(Error::InvalidInput(
                "unstarted definition deletion cannot finalize a stop-only claim".to_owned(),
            ));
        };
        authenticate_unadvanced_source_retirement_claim(claim)?;
        if self
            .service_definitions
            .service_definition_for_tenant(&claim.tenant_id, name)
            .is_some()
        {
            return Err(Error::conflict(format!(
                "service `{name}` for tenant `{}` is static and cannot be deleted through dynamic service definition routes",
                claim.tenant_id
            )));
        }
        let key = TenantServiceKey::new(&claim.tenant_id, name);
        let retirement_key = WorkloadSourceRetirementKey::Service(key.clone());
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        authenticate_claim(&state.source_retirement_claims, &retirement_key, claim)?;
        let definition = state.definitions.get(&key).cloned().ok_or_else(|| {
            Error::NotFound(format!(
                "service `{name}` for tenant `{}` was not found",
                claim.tenant_id
            ))
        })?;
        if definition.source != ServiceDefinitionSource::Dynamic {
            return Err(Error::conflict(format!(
                "service `{name}` for tenant `{}` is static and cannot be deleted",
                claim.tenant_id
            )));
        }
        let session_ids = service_session_ids(&state, &claim.tenant_id, name);
        authenticate_definition_finalization(&definition, &session_ids, claim)?;
        if state.service_definition_observations.contains_key(&key) {
            return Err(Error::PreconditionFailed(
                "unstarted definition deletion found a provider observation without a saga"
                    .to_owned(),
            ));
        }
        let has_open = session_ids.iter().any(|id| {
            state.sessions.get(id).is_some_and(|session| {
                session.lifecycle_state == SessionLifecycleState::Open
                    && now_millis() < session.expires_at_millis
            })
        });
        if has_open && !force {
            return Err(Error::conflict(
                "service has open sessions and non-force deletion cannot finalize".to_owned(),
            ));
        }
        let removed = state.definitions.remove(&key).ok_or_else(|| {
            Error::NotFound(format!(
                "service `{name}` for tenant `{}` was not found",
                claim.tenant_id
            ))
        })?;
        if force {
            for session_id in session_ids {
                let should_close = state
                    .sessions
                    .get(&session_id)
                    .is_some_and(|session| session.lifecycle_state == SessionLifecycleState::Open);
                if should_close {
                    let now = now_millis();
                    if let Some(session) = state.sessions.get_mut(&session_id) {
                        session.lifecycle_state = SessionLifecycleState::Closed;
                        session.closed_at_millis = Some(now);
                        session.updated_at_millis = now;
                        session.close_reason = Some("service_force_deleted".to_owned());
                    }
                    close_session_channels(&mut state, &session_id, "service_force_deleted");
                }
            }
        }
        state.source_retirement_claims.remove(&retirement_key);
        Ok(removed)
    }

    pub fn project_recorded_service_teardown(
        &self,
        claim: &WorkloadSourceRetirementClaim,
        recorded: &WorkloadSagaRecord,
    ) -> Result<Option<nimbus_sandbox::SandboxHandle>, Error> {
        let WorkloadSourceRetirementIdentity::SandboxBackedService { name } = claim.identity()
        else {
            return Err(Error::InvalidInput(
                "service teardown finalization requires a service source claim".to_owned(),
            ));
        };
        if claim.operation != WorkloadSourceRetirementOperation::Stop {
            return Err(Error::InvalidInput(
                "service stop projection cannot finalize a definition-deletion claim".to_owned(),
            ));
        }
        let key = TenantServiceKey::new(&claim.tenant_id, name);
        let retirement_key = WorkloadSourceRetirementKey::Service(key.clone());
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        authenticate_claim(&state.source_retirement_claims, &retirement_key, claim)?;
        authenticate_recorded_retirement(claim, recorded)?;
        let definition = state.definitions.get(&key).cloned().or_else(|| {
            self.service_definitions
                .service_definition_for_tenant(&claim.tenant_id, name)
        });
        let definition = definition.ok_or_else(|| {
            Error::NotFound(format!(
                "service `{name}` for tenant `{}` was removed before teardown finalization",
                claim.tenant_id
            ))
        })?;
        authenticate_service_source(&definition, claim)?;
        let handle = state
            .service_definition_observations
            .get_mut(&key)
            .map(|observation| {
                observation.handle.status = SandboxStatus::Stopped;
                observation.handle.published_endpoints.clear();
                observation.observed_at_millis = now_millis();
                observation.handle.clone()
            });
        state.source_retirement_claims.remove(&retirement_key);
        Ok(handle)
    }

    pub fn project_recorded_sandbox_teardown(
        &self,
        claim: &WorkloadSourceRetirementClaim,
        recorded: &WorkloadSagaRecord,
    ) -> Result<SandboxResourceSnapshot, Error> {
        let WorkloadSourceRetirementIdentity::StandaloneSandbox { id } = claim.identity() else {
            return Err(Error::InvalidInput(
                "sandbox teardown finalization requires a standalone sandbox claim".to_owned(),
            ));
        };
        let key = TenantSandboxResourceKey::new(&claim.tenant_id, id);
        let retirement_key = WorkloadSourceRetirementKey::Sandbox(key.clone());
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        authenticate_claim(&state.source_retirement_claims, &retirement_key, claim)?;
        authenticate_recorded_retirement(claim, recorded)?;
        let source = state
            .sandbox_resource_sources
            .get(&key)
            .cloned()
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "sandbox `{id}` for tenant `{}` was removed before teardown finalization",
                    claim.tenant_id
                ))
            })?;
        if source.generation != claim.source_generation
            || source.resource_version != claim.resource_version
        {
            return Err(Error::PreconditionFailed(
                "sandbox source changed before teardown finalization".to_owned(),
            ));
        }
        if let Some(observation) = state.sandbox_resource_observations.get_mut(&key) {
            observation.handle.status = SandboxStatus::Stopped;
            observation.handle.published_endpoints.clear();
            observation.observed_at_millis = now_millis();
        }
        let observation = state.sandbox_resource_observations.get(&key).cloned();
        state.source_retirement_claims.remove(&retirement_key);
        Ok(SandboxResourceSnapshot {
            source,
            observation,
        })
    }

    pub fn finalize_service_definition_after_recorded(
        &self,
        claim: &WorkloadSourceRetirementClaim,
        recorded: &WorkloadSagaRecord,
    ) -> Result<ServiceDefinition, Error> {
        let WorkloadSourceRetirementIdentity::SandboxBackedService { name } = claim.identity()
        else {
            return Err(Error::InvalidInput(
                "definition deletion requires a service source claim".to_owned(),
            ));
        };
        let WorkloadSourceRetirementOperation::DeleteDefinition { force } = claim.operation else {
            return Err(Error::InvalidInput(
                "definition deletion cannot finalize a stop-only claim".to_owned(),
            ));
        };
        let key = TenantServiceKey::new(&claim.tenant_id, name);
        let retirement_key = WorkloadSourceRetirementKey::Service(key.clone());
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        authenticate_claim(&state.source_retirement_claims, &retirement_key, claim)?;
        authenticate_recorded_retirement(claim, recorded)?;
        let definition = state.definitions.get(&key).cloned().ok_or_else(|| {
            Error::NotFound(format!(
                "service `{name}` for tenant `{}` was removed before deletion finalization",
                claim.tenant_id
            ))
        })?;
        let current_session_ids = service_session_ids(&state, &claim.tenant_id, name);
        authenticate_definition_finalization(&definition, &current_session_ids, claim)?;
        if !force
            && current_session_ids.iter().any(|id| {
                state.sessions.get(id).is_some_and(|session| {
                    session.lifecycle_state == SessionLifecycleState::Open
                        && now_millis() < session.expires_at_millis
                })
            })
        {
            return Err(Error::conflict(
                "service has open sessions and non-force deletion cannot finalize".to_owned(),
            ));
        }
        state.service_definition_observations.remove(&key);
        let removed = state.definitions.remove(&key).ok_or_else(|| {
            Error::NotFound(format!(
                "service `{name}` for tenant `{}` was not found",
                claim.tenant_id
            ))
        })?;
        if force {
            for session_id in &claim.captured_session_ids {
                let should_close = state
                    .sessions
                    .get(session_id)
                    .is_some_and(|session| session.lifecycle_state == SessionLifecycleState::Open);
                if should_close {
                    let now = now_millis();
                    if let Some(session) = state.sessions.get_mut(session_id) {
                        session.lifecycle_state = SessionLifecycleState::Closed;
                        session.closed_at_millis = Some(now);
                        session.updated_at_millis = now;
                        session.close_reason = Some("service_force_deleted".to_owned());
                    }
                    close_session_channels(&mut state, session_id, "service_force_deleted");
                }
            }
        }
        state.source_retirement_claims.remove(&retirement_key);
        Ok(removed)
    }

    pub(super) fn source_retirement_claim_exists(
        state: &ServiceManagerState,
        key: &WorkloadSourceRetirementKey,
    ) -> bool {
        state.source_retirement_claims.contains_key(key)
    }
}

fn insert_or_replay_claim(
    claims: &mut std::collections::BTreeMap<
        WorkloadSourceRetirementKey,
        WorkloadSourceRetirementClaim,
    >,
    key: WorkloadSourceRetirementKey,
    claim: WorkloadSourceRetirementClaim,
) -> Result<WorkloadSourceRetirementClaim, Error> {
    match claims.get(&key) {
        Some(current) if same_retirement_claim(current, &claim) => {
            let generation_order = claim.saga_generation.cmp(&current.saga_generation);
            let revision_order = claim.saga_revision.cmp(&current.saga_revision);
            if generation_order.is_gt() || generation_order.is_eq() && revision_order.is_gt() {
                claims.insert(key, claim.clone());
                Ok(claim)
            } else {
                Ok(current.clone())
            }
        }
        Some(_) => Err(Error::conflict(
            "source has a crossed retirement claim in progress".to_owned(),
        )),
        None => {
            claims.insert(key, claim.clone());
            Ok(claim)
        }
    }
}

fn authenticate_unadvanced_source_retirement_claim(
    claim: &WorkloadSourceRetirementClaim,
) -> Result<(), Error> {
    if claim.saga_generation != WorkloadGeneration::new(0)
        || claim.saga_revision != WorkloadSagaRevision::new(0)
    {
        return Err(Error::PreconditionFailed(
            "an advanced source retirement claim cannot use an unstarted terminal path".to_owned(),
        ));
    }
    Ok(())
}

fn same_retirement_claim(
    left: &WorkloadSourceRetirementClaim,
    right: &WorkloadSourceRetirementClaim,
) -> bool {
    left.tenant_id == right.tenant_id
        && left.identity == right.identity
        && left.source_generation == right.source_generation
        && left.resource_version == right.resource_version
        && left.operation == right.operation
        && left.captured_session_ids == right.captured_session_ids
}

fn retirement_key(claim: &WorkloadSourceRetirementClaim) -> WorkloadSourceRetirementKey {
    match &claim.identity {
        WorkloadSourceRetirementIdentity::SandboxBackedService { name } => {
            WorkloadSourceRetirementKey::Service(TenantServiceKey::new(&claim.tenant_id, name))
        }
        WorkloadSourceRetirementIdentity::StandaloneSandbox { id } => {
            WorkloadSourceRetirementKey::Sandbox(TenantSandboxResourceKey::new(
                &claim.tenant_id,
                id,
            ))
        }
    }
}

fn authenticate_claim(
    claims: &std::collections::BTreeMap<WorkloadSourceRetirementKey, WorkloadSourceRetirementClaim>,
    key: &WorkloadSourceRetirementKey,
    expected: &WorkloadSourceRetirementClaim,
) -> Result<(), Error> {
    match claims.get(key) {
        Some(current) if current == expected => Ok(()),
        Some(_) => Err(Error::PreconditionFailed(
            "source retirement claim changed before terminal finalization".to_owned(),
        )),
        None => Err(Error::PreconditionFailed(
            "source retirement claim is missing before terminal finalization".to_owned(),
        )),
    }
}

fn authenticate_service_source(
    definition: &ServiceDefinition,
    claim: &WorkloadSourceRetirementClaim,
) -> Result<(), Error> {
    if definition.generation == claim.source_generation
        && definition.resource_version == claim.resource_version
    {
        Ok(())
    } else {
        Err(Error::PreconditionFailed(
            "service definition changed before retirement finalization".to_owned(),
        ))
    }
}

pub(super) fn authenticate_definition_finalization(
    definition: &ServiceDefinition,
    current_session_ids: &[String],
    claim: &WorkloadSourceRetirementClaim,
) -> Result<(), Error> {
    authenticate_service_source(definition, claim)?;
    if current_session_ids != claim.captured_session_ids {
        return Err(Error::PreconditionFailed(
            "service session set changed before definition deletion finalization".to_owned(),
        ));
    }
    Ok(())
}

fn authenticate_recorded_retirement(
    claim: &WorkloadSourceRetirementClaim,
    recorded: &WorkloadSagaRecord,
) -> Result<(), Error> {
    let active = recorded.active_intent();
    let expected_kind = match claim.identity() {
        WorkloadSourceRetirementIdentity::SandboxBackedService { .. } => {
            WorkloadProvisionSourceKind::SandboxBackedService
        }
        WorkloadSourceRetirementIdentity::StandaloneSandbox { .. } => {
            WorkloadProvisionSourceKind::StandaloneSandbox
        }
    };
    let expected_name = match claim.identity() {
        WorkloadSourceRetirementIdentity::SandboxBackedService { name } => name,
        WorkloadSourceRetirementIdentity::StandaloneSandbox { id } => id,
    };
    let source = active.source();
    if recorded.phase() != WorkloadSagaPhase::Recorded
        || active.desired_state() != DesiredWorkloadState::Stopped
        || recorded.successor_intent().is_some()
        || recorded.key().tenant_id() != claim.tenant_id()
        || recorded.key().workload_id().as_str() != expected_name
        || active.generation() != claim.saga_generation()
        || recorded.revision() != claim.saga_revision()
        || source.source_identity().kind() != expected_kind
        || source.source_identity().stable_name() != expected_name
        || source.source_generation().as_u64() != claim.source_generation()
        || source.resource_version().as_str() != claim.resource_version()
    {
        return Err(Error::PreconditionFailed(
            "terminal services mutation requires the exact Recorded retirement record".to_owned(),
        ));
    }
    Ok(())
}

fn service_session_ids(
    state: &ServiceManagerState,
    tenant_id: &TenantId,
    service_name: &str,
) -> Vec<String> {
    state
        .sessions
        .values()
        .filter(|session| {
            &session.tenant_id == tenant_id
                && matches!(&session.target, SessionTarget::Service { name } if name == service_name)
        })
        .map(|session| session.id.clone())
        .collect()
}
