use std::collections::BTreeMap;

use nimbus_core::{Error, TenantId};
use nimbus_runtime::{InvocationServiceBinding, InvocationServices};
use nimbus_sandbox::SandboxStatus;
use nimbus_workloads::WorkloadExecutionAttemptId;

use crate::ServiceBackend;
use crate::ServiceInstanceObservation;
use crate::registry::{RuntimeServiceRegistry, service_binding_from_instance};

use super::ServiceManager;
use super::types::{
    ServiceManagerState, ServiceResolutionWithdrawal, TenantServiceKey, WorkloadSourceRetirementKey,
};

/// Read-only runtime naming projection over services-owned observations.
///
/// This implementation has no provider handle, cancellation token, future,
/// or lifecycle capability. An exact compute projection is visible
/// immediately; missing/pending observations resolve as absent without
/// provider inspection.
impl RuntimeServiceRegistry for ServiceManager {
    fn snapshot_for_tenant(&self, tenant_id: &TenantId) -> InvocationServices {
        self.service_instances_for_resolution(tenant_id)
            .into_iter()
            .filter_map(|(service_name, observation)| {
                service_binding_from_instance(&observation).map(|binding| (service_name, binding))
            })
            .collect()
    }

    fn resolve_service_binding(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        let Some(observation) = self.service_instance_for_resolution(tenant_id, service_name)?
        else {
            return Ok(None);
        };
        Ok(service_binding_from_instance(&observation))
    }
}

impl ServiceManager {
    /// Fence logical resolution before compute awaits restart publication
    /// withdrawal. This operation owns no provider effect or durable saga
    /// transition.
    pub fn claim_service_resolution_withdrawal(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        source_generation: u64,
        resource_version: &str,
        source_attempt_id: &WorkloadExecutionAttemptId,
        target_attempt_id: &WorkloadExecutionAttemptId,
    ) -> Result<(), Error> {
        if source_attempt_id == target_attempt_id {
            return Err(Error::InvalidInput(
                "service resolution withdrawal requires distinct source and target attempts"
                    .to_owned(),
            ));
        }
        let catalog_definition = self
            .service_definitions
            .service_definition_for_tenant(tenant_id, service_name);
        let key = TenantServiceKey::new(tenant_id, service_name);
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
        if !matches!(definition.backend, ServiceBackend::Sandbox(_)) {
            return Err(Error::InvalidInput(format!(
                "service `{service_name}` for tenant `{tenant_id}` is not sandbox-backed"
            )));
        }
        if definition.generation != source_generation
            || definition.resource_version != resource_version
        {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` changed before resolution withdrawal"
            )));
        }
        let withdrawal = ServiceResolutionWithdrawal {
            source_generation,
            resource_version: resource_version.to_owned(),
            source_attempt_id: source_attempt_id.clone(),
            target_attempt_id: target_attempt_id.clone(),
            active: true,
        };
        match state.service_resolution_withdrawals.get(&key) {
            Some(current) if current == &withdrawal => Ok(()),
            Some(current)
                if current.source_generation == source_generation
                    && current.resource_version == resource_version
                    && &current.target_attempt_id == source_attempt_id =>
            {
                state.service_resolution_withdrawals.insert(key, withdrawal);
                Ok(())
            }
            Some(current) if current.active => Err(Error::conflict(format!(
                "service `{service_name}` has a crossed active resolution withdrawal"
            ))),
            Some(_) => Err(Error::PreconditionFailed(format!(
                "service `{service_name}` rejected a stale or crossed resolution withdrawal"
            ))),
            None => {
                if let Some(observation) = state.service_definition_observations.get(&key)
                    && observation.execution.attempt_id() != source_attempt_id
                {
                    return Err(Error::PreconditionFailed(format!(
                        "service `{service_name}` changed execution before resolution withdrawal"
                    )));
                }
                // A workload can exit after portable observation but before
                // its first services projection. Resolution is already
                // absent in that race, but the exact active fence must still
                // be installed so the restart target cannot become visible
                // before compute releases it.
                state.service_resolution_withdrawals.insert(key, withdrawal);
                Ok(())
            }
        }
    }

    /// Report whether the exact restart target still has an active logical
    /// resolution fence. Missing state is already restored; crossed state
    /// fails closed.
    pub fn service_resolution_withdrawal_requires_restore(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        source_generation: u64,
        resource_version: &str,
        target_attempt_id: &WorkloadExecutionAttemptId,
    ) -> Result<bool, Error> {
        let key = TenantServiceKey::new(tenant_id, service_name);
        let state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let Some(current) = state.service_resolution_withdrawals.get(&key) else {
            return Ok(false);
        };
        if current.source_generation != source_generation
            || current.resource_version != resource_version
            || &current.target_attempt_id != target_attempt_id
        {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` rejected a stale or crossed resolution restore check"
            )));
        }
        Ok(current.active)
    }

    /// Release only the exact restart fence after durable publication
    /// observation completes. Replays after release are idempotent.
    pub fn release_service_resolution_withdrawal(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        source_generation: u64,
        resource_version: &str,
        target_attempt_id: &WorkloadExecutionAttemptId,
    ) -> Result<(), Error> {
        let key = TenantServiceKey::new(tenant_id, service_name);
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let Some(current) = state.service_resolution_withdrawals.get(&key) else {
            return Ok(());
        };
        if current.source_generation != source_generation
            || current.resource_version != resource_version
            || &current.target_attempt_id != target_attempt_id
        {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` rejected a stale or crossed resolution release"
            )));
        }
        if current.active {
            let observation = state
                .service_definition_observations
                .get(&key)
                .ok_or_else(|| {
                    Error::PreconditionFailed(format!(
                        "service `{service_name}` has no target execution observation to release"
                    ))
                })?;
            if observation.execution.attempt_id() != target_attempt_id {
                return Err(Error::PreconditionFailed(format!(
                    "service `{service_name}` has not observed the restart target attempt"
                )));
            }
            let mut released = current.clone();
            released.active = false;
            state.service_resolution_withdrawals.insert(key, released);
        }
        Ok(())
    }

    pub(super) fn service_instances_for_resolution(
        &self,
        tenant_id: &TenantId,
    ) -> BTreeMap<String, ServiceInstanceObservation> {
        let state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        state
            .service_definition_observations
            .iter()
            .filter(|(key, observation)| {
                &key.tenant_id == tenant_id
                    && observation.tenant_id == *tenant_id
                    && observation.name == key.service_name
                    && !service_resolution_is_fenced(&state, key)
                    && !matches!(
                        observation.handle.status,
                        SandboxStatus::Stopped | SandboxStatus::Failed
                    )
            })
            .filter_map(|(key, observation)| {
                observation
                    .service_instance()
                    .ok()
                    .map(|instance| (key.service_name.clone(), instance))
            })
            .collect()
    }

    fn service_instance_for_resolution(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<ServiceInstanceObservation>, Error> {
        let state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let key = TenantServiceKey::new(tenant_id, service_name);
        if service_resolution_is_fenced(&state, &key) {
            return Ok(None);
        }
        let Some(observation) = state.service_definition_observations.get(&key) else {
            return Ok(None);
        };
        if observation.tenant_id != *tenant_id || observation.name != service_name {
            return Err(Error::PermissionDenied(format!(
                "service observation for `{service_name}` is crossed with tenant `{tenant_id}`"
            )));
        }
        Ok(Some(observation.service_instance()?))
    }
}

fn service_resolution_is_fenced(state: &ServiceManagerState, key: &TenantServiceKey) -> bool {
    state.tenant_source_retirements.contains_key(&key.tenant_id)
        || state
            .service_resolution_withdrawals
            .get(key)
            .is_some_and(|withdrawal| withdrawal.active)
        || ServiceManager::source_retirement_claim_exists(
            state,
            &WorkloadSourceRetirementKey::Service(key.clone()),
        )
}
