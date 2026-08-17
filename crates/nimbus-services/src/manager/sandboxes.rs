use nimbus_core::{Error, TenantId};
use nimbus_sandbox::{SandboxHandle, SandboxOwnerSpec, SandboxSpec};
use nimbus_workloads::{WorkloadExecutionAttemptId, WorkloadExecutionReference};

use crate::{SandboxResourceObservation, SandboxResourceSnapshot, SandboxResourceSource};

use super::ServiceManager;
use super::clock::now_millis;
use super::types::TenantSandboxResourceKey;

struct SandboxResourceObservationProjection<'a> {
    tenant_id: &'a TenantId,
    stable_resource_id: &'a str,
    source_generation: Option<u64>,
    observed_execution_generation: u64,
    expected_resource_version: Option<&'a str>,
    expected_attempt_id: &'a WorkloadExecutionAttemptId,
    exact_execution: Option<&'a WorkloadExecutionReference>,
    handle: SandboxHandle,
}

impl ServiceManager {
    /// Project one exact provider observation without changing desired source.
    pub fn project_sandbox_resource_observation(
        &self,
        tenant_id: &TenantId,
        stable_resource_id: &str,
        observed_execution_generation: u64,
        expected_attempt_id: &WorkloadExecutionAttemptId,
        handle: SandboxHandle,
    ) -> Result<SandboxResourceObservation, Error> {
        self.project_sandbox_resource_observation_inner(SandboxResourceObservationProjection {
            tenant_id,
            stable_resource_id,
            source_generation: None,
            observed_execution_generation,
            expected_resource_version: None,
            expected_attempt_id,
            exact_execution: None,
            handle,
        })
    }

    /// Project the execution selected for one exact desired sandbox source.
    ///
    /// Source version and the complete execution reference are authenticated
    /// while the source and projection share the manager lock. Failed evidence
    /// leaves the existing observed bytes unchanged.
    pub fn project_sandbox_resource_execution_observation(
        &self,
        tenant_id: &TenantId,
        stable_resource_id: &str,
        source_generation: u64,
        expected_resource_version: &str,
        execution: &WorkloadExecutionReference,
        handle: SandboxHandle,
    ) -> Result<SandboxResourceObservation, Error> {
        self.project_sandbox_resource_observation_inner(SandboxResourceObservationProjection {
            tenant_id,
            stable_resource_id,
            source_generation: Some(source_generation),
            observed_execution_generation: execution.generation().as_u64(),
            expected_resource_version: Some(expected_resource_version),
            expected_attempt_id: execution.attempt_id(),
            exact_execution: Some(execution),
            handle,
        })
    }

    fn project_sandbox_resource_observation_inner(
        &self,
        projection: SandboxResourceObservationProjection<'_>,
    ) -> Result<SandboxResourceObservation, Error> {
        let SandboxResourceObservationProjection {
            tenant_id,
            stable_resource_id,
            source_generation,
            observed_execution_generation,
            expected_resource_version,
            expected_attempt_id,
            exact_execution,
            handle,
        } = projection;
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let key = TenantSandboxResourceKey::new(tenant_id, stable_resource_id);
        let source = state.sandbox_resource_sources.get(&key).ok_or_else(|| {
            Error::NotFound(format!(
                "sandbox resource `{stable_resource_id}` was not found for tenant `{tenant_id}`"
            ))
        })?;
        if &source.tenant_id != tenant_id {
            return Err(Error::NotFound(format!(
                "sandbox resource `{stable_resource_id}` was not found for tenant `{tenant_id}`"
            )));
        }
        let existing = state.sandbox_resource_observations.get(&key);
        let source_generation = source_generation
            .or_else(|| existing.map(|observation| observation.source_generation))
            .ok_or_else(|| {
                Error::PreconditionFailed(format!(
                    "sandbox resource `{stable_resource_id}` requires exact source generation before its first projection"
                ))
            })?;
        if source.generation != source_generation {
            return Err(Error::PreconditionFailed(format!(
                "sandbox resource `{stable_resource_id}` has source generation {}, but observation expected source generation {source_generation}",
                source.generation
            )));
        }
        if expected_resource_version.is_some_and(|expected| source.resource_version != expected) {
            return Err(Error::PreconditionFailed(format!(
                "sandbox resource `{stable_resource_id}` has resource version {}, but observation expected {expected_resource_version:?}",
                source.resource_version
            )));
        }
        validate_sandbox_observation_identity(source, &handle)?;
        if exact_execution.is_some_and(|execution| {
            execution.generation().as_u64() != observed_execution_generation
                || handle.id.as_str() != execution.execution_id().as_str()
        }) {
            return Err(Error::InvalidInput(format!(
                "sandbox resource `{stable_resource_id}` for tenant `{tenant_id}` rejected provider sandbox {} because its exact execution reference is crossed",
                handle.id,
            )));
        }
        let exact_first_writer = expected_resource_version.is_some() && exact_execution.is_some();
        if !exact_first_writer && !state.sandbox_resource_observations.contains_key(&key) {
            return Err(Error::PreconditionFailed(format!(
                "sandbox resource `{stable_resource_id}` for tenant `{tenant_id}` requires an exact source-version and execution-reference projection before transitional refreshes are accepted"
            )));
        }
        if state
            .sandbox_resource_observations
            .iter()
            .any(|(candidate, observation)| candidate != &key && observation.handle.id == handle.id)
        {
            return Err(Error::conflict(format!(
                "sandbox provider handle `{}` is already projected by another resource",
                handle.id
            )));
        }
        if let Some(existing) = existing {
            if existing.source_generation != source_generation {
                return Err(Error::PreconditionFailed(format!(
                    "sandbox resource `{stable_resource_id}` projection crossed source generation {}",
                    existing.source_generation
                )));
            }
            if existing.observed_execution_generation > observed_execution_generation {
                return Err(Error::PreconditionFailed(format!(
                    "sandbox resource `{stable_resource_id}` already has newer observed execution generation {}",
                    existing.observed_execution_generation
                )));
            }
            if existing.observed_execution_generation == observed_execution_generation
                && !same_provider_identity(&existing.handle, &handle)
            {
                return Err(Error::conflict(format!(
                    "sandbox resource `{stable_resource_id}` execution generation {observed_execution_generation} already has a different provider observation"
                )));
            }
            if existing.observed_execution_generation == observed_execution_generation {
                validate_attempt_progression(
                    tenant_id,
                    stable_resource_id,
                    existing,
                    expected_attempt_id,
                    exact_execution,
                )?;
            } else if exact_execution.is_none() {
                return Err(Error::PreconditionFailed(format!(
                    "sandbox resource `{stable_resource_id}` requires an exact execution reference to advance lifecycle generation"
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
                .sandbox_resource_observations
                .get(&key)
                .expect("transitional projection requires an existing exact observation")
                .execution
                .clone(),
        };
        let observation = SandboxResourceObservation {
            tenant_id: tenant_id.clone(),
            id: stable_resource_id.to_owned(),
            source_generation,
            observed_execution_generation,
            execution,
            handle,
            observed_at_millis: now_millis(),
        };
        state
            .sandbox_resource_observations
            .insert(key, observation.clone());
        Ok(observation)
    }

    /// Return the immutable source-owned sandbox snapshot without provider inspection.
    ///
    /// Workload freshness checks use this path so reading source generation,
    /// resource version, and executable input cannot restart or otherwise
    /// mutate the sandbox provider.
    pub fn sandbox_resource_source_for_tenant(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &str,
    ) -> Result<Option<SandboxResourceSource>, Error> {
        Ok(self
            .sandbox_resource_snapshot_for_tenant(tenant_id, sandbox_id)?
            .map(|snapshot| snapshot.source))
    }

    pub fn sandbox_resource_snapshot_for_tenant(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &str,
    ) -> Result<Option<SandboxResourceSnapshot>, Error> {
        let state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let key = TenantSandboxResourceKey::new(tenant_id, sandbox_id);
        let Some(source) = state.sandbox_resource_sources.get(&key).cloned() else {
            return Ok(None);
        };
        Ok(Some(SandboxResourceSnapshot {
            source,
            observation: state.sandbox_resource_observations.get(&key).cloned(),
        }))
    }

    pub fn list_sandbox_resource_snapshots_for_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Vec<SandboxResourceSnapshot> {
        let state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        state
            .sandbox_resource_sources
            .iter()
            .filter(|(key, _)| &key.tenant_id == tenant_id)
            .map(|(key, source)| SandboxResourceSnapshot {
                observation: state.sandbox_resource_observations.get(key).cloned(),
                source: source.clone(),
            })
            .collect()
    }
}

fn validate_attempt_progression(
    tenant_id: &TenantId,
    stable_resource_id: &str,
    existing: &SandboxResourceObservation,
    expected_attempt_id: &WorkloadExecutionAttemptId,
    exact_execution: Option<&WorkloadExecutionReference>,
) -> Result<(), Error> {
    let Some(exact_execution) = exact_execution else {
        if existing.execution.attempt_id() == expected_attempt_id {
            return Ok(());
        }
        return Err(Error::PreconditionFailed(format!(
            "sandbox resource `{stable_resource_id}` for tenant `{tenant_id}` rejected a transitional observation for a different execution attempt"
        )));
    };
    if !same_execution_owner(&existing.execution, exact_execution) {
        return Err(Error::InvalidInput(format!(
            "sandbox resource `{stable_resource_id}` for tenant `{tenant_id}` rejected a crossed execution reference"
        )));
    }
    if exact_execution.restart_epoch() < existing.execution.restart_epoch() {
        return Err(Error::PreconditionFailed(format!(
            "sandbox resource `{stable_resource_id}` for tenant `{tenant_id}` already has newer restart epoch {}",
            existing.execution.restart_epoch()
        )));
    }
    if exact_execution.restart_epoch() == existing.execution.restart_epoch()
        && exact_execution.attempt_id() != existing.execution.attempt_id()
    {
        return Err(Error::InvalidInput(format!(
            "sandbox resource `{stable_resource_id}` for tenant `{tenant_id}` rejected a crossed execution attempt"
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

pub(super) fn same_sandbox_resource_desire(
    left: &SandboxResourceSource,
    right: &SandboxResourceSource,
) -> bool {
    left.tenant_id == right.tenant_id
        && left.id == right.id
        && left.profile == right.profile
        && left.spec == right.spec
        && left.generation == right.generation
        && left.resource_version == right.resource_version
        && left.labels == right.labels
}

fn validate_sandbox_observation_identity(
    source: &SandboxResourceSource,
    observed: &SandboxHandle,
) -> Result<(), Error> {
    if observed.tenant_id == source.tenant_id
        && observed.name == source.spec.display_name()
        && observed.backend == source.spec.backend
    {
        return Ok(());
    }
    Err(Error::InvalidInput(format!(
        "sandbox provider returned crossed observation for source `{}` tenant {}: expected name {} backend {:?}, observed sandbox {} tenant {} name {} backend {:?}",
        source.id,
        source.tenant_id,
        source.spec.display_name(),
        source.spec.backend,
        observed.id,
        observed.tenant_id,
        observed.name,
        observed.backend
    )))
}

fn same_provider_identity(left: &SandboxHandle, right: &SandboxHandle) -> bool {
    left.id == right.id
        && left.tenant_id == right.tenant_id
        && left.name == right.name
        && left.backend == right.backend
}

pub(super) fn validate_sandbox_resource_spec(
    tenant_id: &TenantId,
    spec: &SandboxSpec,
) -> Result<(), Error> {
    if &spec.tenant_id != tenant_id {
        return Err(Error::InvalidInput(format!(
            "sandbox spec tenant {} does not match route tenant {tenant_id}",
            spec.tenant_id
        )));
    }
    if matches!(spec.owner, SandboxOwnerSpec::Service { .. }) {
        return Err(Error::InvalidInput(
            "sandbox resource create requires standalone sandbox owner metadata; service-owned sandboxes are created by service lifecycle".to_owned(),
        ));
    }
    if spec.root.is_unspecified_rootfs() {
        return Err(Error::InvalidInput(
            "sandbox resource create requires a rootfs or OCI image root".to_owned(),
        ));
    }
    Ok(())
}
