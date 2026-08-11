use nimbus_core::{Error, TenantId};
use nimbus_sandbox::SandboxHandle;
use nimbus_workloads::{WorkloadExecutionAttemptId, WorkloadExecutionReference};

use crate::{ServiceBackend, ServiceDefinitionObservation};

use super::ServiceManager;
use super::clock::now_millis;
use super::types::TenantServiceKey;

struct ServiceDefinitionObservationProjection<'a> {
    tenant_id: &'a TenantId,
    service_name: &'a str,
    source_generation: Option<u64>,
    observed_execution_generation: u64,
    expected_resource_version: Option<&'a str>,
    expected_attempt_id: &'a WorkloadExecutionAttemptId,
    exact_execution: Option<&'a WorkloadExecutionReference>,
    handle: SandboxHandle,
}

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
        observed_execution_generation: u64,
        expected_attempt_id: &WorkloadExecutionAttemptId,
        handle: SandboxHandle,
    ) -> Result<ServiceDefinitionObservation, Error> {
        self.project_service_definition_observation_inner(ServiceDefinitionObservationProjection {
            tenant_id,
            service_name,
            source_generation: None,
            observed_execution_generation,
            expected_resource_version: None,
            expected_attempt_id,
            exact_execution: None,
            handle,
        })
    }

    /// Project the execution selected for one exact desired service source.
    ///
    /// Unlike the transitional lifecycle projection, this compute-facing
    /// boundary authenticates source generation, source resource version, and
    /// the complete execution reference before the first observed write or a
    /// same-generation attempt advance. A crossed or stale caller therefore
    /// cannot establish provider identity.
    pub fn project_service_definition_execution_observation(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        source_generation: u64,
        expected_resource_version: &str,
        execution: &WorkloadExecutionReference,
        handle: SandboxHandle,
    ) -> Result<ServiceDefinitionObservation, Error> {
        self.project_service_definition_observation_inner(ServiceDefinitionObservationProjection {
            tenant_id,
            service_name,
            source_generation: Some(source_generation),
            observed_execution_generation: execution.generation().as_u64(),
            expected_resource_version: Some(expected_resource_version),
            expected_attempt_id: execution.attempt_id(),
            exact_execution: Some(execution),
            handle,
        })
    }

    fn project_service_definition_observation_inner(
        &self,
        projection: ServiceDefinitionObservationProjection<'_>,
    ) -> Result<ServiceDefinitionObservation, Error> {
        let ServiceDefinitionObservationProjection {
            tenant_id,
            service_name,
            source_generation,
            observed_execution_generation,
            expected_resource_version,
            expected_attempt_id,
            exact_execution,
            handle,
        } = projection;
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
        let existing = state.service_definition_observations.get(&key);
        let source_generation = source_generation
            .or_else(|| existing.map(|observation| observation.source_generation))
            .ok_or_else(|| {
                Error::PreconditionFailed(format!(
                    "service `{service_name}` for tenant `{tenant_id}` requires exact source generation before its first projection"
                ))
            })?;
        if definition.generation != source_generation {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` for tenant `{tenant_id}` has source generation {}, but observation expected source generation {source_generation}",
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
        if exact_execution.is_some_and(|execution| {
            execution.generation().as_u64() != observed_execution_generation
                || handle.id.as_str() != execution.execution_id().as_str()
        }) {
            return Err(Error::InvalidInput(format!(
                "service `{service_name}` for tenant `{tenant_id}` rejected provider sandbox {} because its exact execution reference is crossed",
                handle.id,
            )));
        }
        let exact_first_writer = expected_resource_version.is_some() && exact_execution.is_some();
        if !exact_first_writer && !state.service_definition_observations.contains_key(&key) {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` for tenant `{tenant_id}` requires an exact source-version and execution-reference projection before transitional refreshes are accepted"
            )));
        }
        if let Some(existing) = existing {
            if existing.source_generation != source_generation {
                return Err(Error::PreconditionFailed(format!(
                    "service `{service_name}` for tenant `{tenant_id}` projection crossed source generation {}",
                    existing.source_generation
                )));
            }
            if existing.observed_execution_generation > observed_execution_generation {
                return Err(Error::PreconditionFailed(format!(
                    "service `{service_name}` for tenant `{tenant_id}` already has newer observed execution generation {}",
                    existing.observed_execution_generation
                )));
            }
            if existing.observed_execution_generation == observed_execution_generation
                && !same_provider_identity(&existing.handle, &handle)
            {
                return Err(Error::conflict(format!(
                    "service `{service_name}` for tenant `{tenant_id}` execution generation {observed_execution_generation} already has a different provider observation"
                )));
            }
            if existing.observed_execution_generation == observed_execution_generation {
                validate_attempt_progression(
                    tenant_id,
                    service_name,
                    existing,
                    expected_attempt_id,
                    exact_execution,
                )?;
            } else if exact_execution.is_none() {
                return Err(Error::PreconditionFailed(format!(
                    "service `{service_name}` for tenant `{tenant_id}` requires an exact execution reference to advance lifecycle generation"
                )));
            }
            if existing.handle == handle
                && existing.execution.attempt_id() == expected_attempt_id
                && existing.observed_execution_generation == observed_execution_generation
            {
                return Ok(existing.clone());
            }
        }
        let execution = match exact_execution {
            Some(execution) => execution.clone(),
            None => state
                .service_definition_observations
                .get(&key)
                .expect("transitional projection requires an existing exact observation")
                .execution
                .clone(),
        };
        let observation = ServiceDefinitionObservation {
            tenant_id: tenant_id.clone(),
            name: service_name.to_owned(),
            source_generation,
            observed_execution_generation,
            execution,
            handle,
            observed_at_millis: now_millis(),
        };
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

fn validate_attempt_progression(
    tenant_id: &TenantId,
    service_name: &str,
    existing: &ServiceDefinitionObservation,
    expected_attempt_id: &WorkloadExecutionAttemptId,
    exact_execution: Option<&WorkloadExecutionReference>,
) -> Result<(), Error> {
    let Some(exact_execution) = exact_execution else {
        if existing.execution.attempt_id() == expected_attempt_id {
            return Ok(());
        }
        return Err(Error::PreconditionFailed(format!(
            "service `{service_name}` for tenant `{tenant_id}` rejected a transitional observation for a different execution attempt"
        )));
    };
    if !same_execution_owner(&existing.execution, exact_execution) {
        return Err(Error::InvalidInput(format!(
            "service `{service_name}` for tenant `{tenant_id}` rejected a crossed execution reference"
        )));
    }
    if exact_execution.restart_epoch() < existing.execution.restart_epoch() {
        return Err(Error::PreconditionFailed(format!(
            "service `{service_name}` for tenant `{tenant_id}` already has newer restart epoch {}",
            existing.execution.restart_epoch()
        )));
    }
    if exact_execution.restart_epoch() == existing.execution.restart_epoch()
        && exact_execution.attempt_id() != existing.execution.attempt_id()
    {
        return Err(Error::InvalidInput(format!(
            "service `{service_name}` for tenant `{tenant_id}` rejected a crossed execution attempt"
        )));
    }
    Ok(())
}

fn same_execution_owner(
    left: &WorkloadExecutionReference,
    right: &WorkloadExecutionReference,
) -> bool {
    left.workload_uid() == right.workload_uid()
        && left.node_identity() == right.node_identity()
        && left.execution_id() == right.execution_id()
        && left.generation() == right.generation()
        && left.desired_digest() == right.desired_digest()
}

fn same_provider_identity(left: &SandboxHandle, right: &SandboxHandle) -> bool {
    left.id == right.id
        && left.tenant_id == right.tenant_id
        && left.name == right.name
        && left.backend == right.backend
}
