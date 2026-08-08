use nimbus_core::{Error, TenantId};
use nimbus_sandbox::SandboxHandle;

use crate::{ServiceBackend, ServiceDefinitionObservation};

use super::ServiceManager;
use super::clock::now_millis;
use super::types::TenantServiceKey;

impl ServiceManager {
    /// Read one optional observed projection without provider inspection.
    pub fn service_definition_observation_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceDefinitionObservation> {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .service_definition_observations
            .get(&TenantServiceKey::new(tenant_id, service_name))
            .cloned()
    }

    /// Project one exact sandbox-backed service observation.
    ///
    /// The source definition remains authoritative. Stale, crossed, and
    /// same-generation conflicting observations return without changing the
    /// observed projection.
    pub fn project_service_definition_observation(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        observed_generation: u64,
        handle: SandboxHandle,
    ) -> Result<ServiceDefinitionObservation, Error> {
        self.project_service_definition_observation_inner(
            tenant_id,
            service_name,
            observed_generation,
            None,
            None,
            handle,
        )
    }

    /// Project the execution selected for one exact desired service source.
    ///
    /// Unlike the transitional lifecycle projection, this compute-facing
    /// boundary authenticates source generation, source resource version, and
    /// the deterministic execution ID before the first observed write. A
    /// crossed or stale caller therefore cannot establish provider identity.
    pub fn project_service_definition_execution_observation(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        observed_generation: u64,
        expected_resource_version: &str,
        expected_execution_id: &str,
        handle: SandboxHandle,
    ) -> Result<ServiceDefinitionObservation, Error> {
        self.project_service_definition_observation_inner(
            tenant_id,
            service_name,
            observed_generation,
            Some(expected_resource_version),
            Some(expected_execution_id),
            handle,
        )
    }

    fn project_service_definition_observation_inner(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        observed_generation: u64,
        expected_resource_version: Option<&str>,
        expected_execution_id: Option<&str>,
        handle: SandboxHandle,
    ) -> Result<ServiceDefinitionObservation, Error> {
        // Static catalog definitions are immutable. Dynamic definitions are
        // selected again under the state lock below so an update cannot race a
        // stale first projection into the new generation.
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
        if definition.generation != observed_generation {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` for tenant `{tenant_id}` has generation {}, but observation expected generation {observed_generation}",
                definition.generation
            )));
        }
        if expected_resource_version.is_some_and(|expected| definition.resource_version != expected)
        {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` for tenant `{tenant_id}` has resource version {}, but observation expected {expected_resource_version:?}",
                definition.resource_version
            )));
        }
        let ServiceBackend::Sandbox(spec) = &definition.backend else {
            return Err(Error::InvalidInput(format!(
                "service `{service_name}` for tenant `{tenant_id}` is not sandbox-backed"
            )));
        };
        if handle.tenant_id != *tenant_id
            || handle.name != service_name
            || handle.backend != spec.backend
        {
            return Err(Error::InvalidInput(format!(
                "service `{service_name}` for tenant `{tenant_id}` rejected crossed provider observation sandbox {} tenant {} name {} backend {:?}",
                handle.id, handle.tenant_id, handle.name, handle.backend
            )));
        }
        if expected_execution_id.is_some_and(|expected| handle.id.as_str() != expected) {
            return Err(Error::InvalidInput(format!(
                "service `{service_name}` for tenant `{tenant_id}` rejected provider sandbox {} because the exact execution ID is {expected_execution_id:?}",
                handle.id
            )));
        }
        let exact_first_writer =
            expected_resource_version.is_some() && expected_execution_id.is_some();
        if !exact_first_writer && !state.service_definition_observations.contains_key(&key) {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` for tenant `{tenant_id}` requires an exact source-version and execution-ID projection before transitional refreshes are accepted"
            )));
        }
        let observation = ServiceDefinitionObservation {
            tenant_id: tenant_id.clone(),
            name: service_name.to_owned(),
            observed_generation,
            handle,
            observed_at_millis: now_millis(),
        };
        if let Some(existing) = state.service_definition_observations.get(&key) {
            if existing.observed_generation > observed_generation {
                return Err(Error::PreconditionFailed(format!(
                    "service `{service_name}` for tenant `{tenant_id}` already has newer observed generation {}",
                    existing.observed_generation
                )));
            }
            if existing.observed_generation == observed_generation
                && !same_provider_identity(&existing.handle, &observation.handle)
            {
                return Err(Error::conflict(format!(
                    "service `{service_name}` for tenant `{tenant_id}` generation {observed_generation} already has a different provider observation"
                )));
            }
            if existing.handle == observation.handle {
                return Ok(existing.clone());
            }
        }
        state
            .service_definition_observations
            .insert(key, observation.clone());
        Ok(observation)
    }

    pub(super) fn tenant_service_observations(
        &self,
        tenant_id: &TenantId,
    ) -> Vec<(TenantServiceKey, SandboxHandle)> {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .service_definition_observations
            .iter()
            .filter(|(key, observation)| {
                &key.tenant_id == tenant_id
                    && observation.tenant_id == *tenant_id
                    && observation.name == key.service_name
            })
            .map(|(key, observation)| (key.clone(), observation.handle.clone()))
            .collect()
    }
}

fn same_provider_identity(left: &SandboxHandle, right: &SandboxHandle) -> bool {
    left.id == right.id
        && left.tenant_id == right.tenant_id
        && left.name == right.name
        && left.backend == right.backend
}
