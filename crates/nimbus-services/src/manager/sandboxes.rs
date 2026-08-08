use nimbus_core::{Error, TenantId};
use nimbus_sandbox::{SandboxHandle, SandboxOwnerSpec, SandboxSpec};

use crate::{SandboxResourceObservation, SandboxResourceSnapshot, SandboxResourceSource};

use super::ServiceManager;
use super::clock::now_millis;
use super::types::TenantSandboxResourceKey;

impl ServiceManager {
    /// Project one exact provider observation without changing desired source.
    pub fn project_sandbox_resource_observation(
        &self,
        tenant_id: &TenantId,
        stable_resource_id: &str,
        observed_generation: u64,
        handle: SandboxHandle,
    ) -> Result<SandboxResourceObservation, Error> {
        self.project_sandbox_resource_observation_inner(
            tenant_id,
            stable_resource_id,
            observed_generation,
            None,
            None,
            handle,
        )
    }

    /// Project the execution selected for one exact desired sandbox source.
    ///
    /// Source version and deterministic execution identity are authenticated
    /// while the source and first projection share the manager lock. Failed
    /// evidence leaves the existing observed bytes unchanged.
    pub fn project_sandbox_resource_execution_observation(
        &self,
        tenant_id: &TenantId,
        stable_resource_id: &str,
        observed_generation: u64,
        expected_resource_version: &str,
        expected_execution_id: &str,
        handle: SandboxHandle,
    ) -> Result<SandboxResourceObservation, Error> {
        self.project_sandbox_resource_observation_inner(
            tenant_id,
            stable_resource_id,
            observed_generation,
            Some(expected_resource_version),
            Some(expected_execution_id),
            handle,
        )
    }

    fn project_sandbox_resource_observation_inner(
        &self,
        tenant_id: &TenantId,
        stable_resource_id: &str,
        observed_generation: u64,
        expected_resource_version: Option<&str>,
        expected_execution_id: Option<&str>,
        handle: SandboxHandle,
    ) -> Result<SandboxResourceObservation, Error> {
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
        if source.generation != observed_generation {
            return Err(Error::PreconditionFailed(format!(
                "sandbox resource `{stable_resource_id}` has generation {}, but observation expected generation {observed_generation}",
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
        if expected_execution_id.is_some_and(|expected| handle.id.as_str() != expected) {
            return Err(Error::InvalidInput(format!(
                "sandbox resource `{stable_resource_id}` for tenant `{tenant_id}` rejected provider sandbox {} because the exact execution ID is {expected_execution_id:?}",
                handle.id
            )));
        }
        let exact_first_writer =
            expected_resource_version.is_some() && expected_execution_id.is_some();
        if !exact_first_writer && !state.sandbox_resource_observations.contains_key(&key) {
            return Err(Error::PreconditionFailed(format!(
                "sandbox resource `{stable_resource_id}` for tenant `{tenant_id}` requires an exact source-version and execution-ID projection before transitional refreshes are accepted"
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
        let observation = SandboxResourceObservation {
            tenant_id: tenant_id.clone(),
            id: stable_resource_id.to_owned(),
            observed_generation,
            handle,
            observed_at_millis: now_millis(),
        };
        if let Some(existing) = state.sandbox_resource_observations.get(&key) {
            if existing.observed_generation > observed_generation {
                return Err(Error::PreconditionFailed(format!(
                    "sandbox resource `{stable_resource_id}` already has newer observed generation {}",
                    existing.observed_generation
                )));
            }
            if existing.observed_generation == observed_generation
                && !same_provider_identity(&existing.handle, &observation.handle)
            {
                return Err(Error::conflict(format!(
                    "sandbox resource `{stable_resource_id}` generation {observed_generation} already has a different provider observation"
                )));
            }
            if existing.handle == observation.handle {
                return Ok(existing.clone());
            }
        }
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
