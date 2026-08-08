use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;

use nimbus_core::{Error, TenantId};
use nimbus_sandbox::{SandboxCleanupObservation, SandboxHandle, SandboxInspection, SandboxStatus};
use nimbus_tenant::TenantIsolationDecision;

use crate::SandboxResourceSnapshot;

use super::ServiceManager;
use super::types::{TenantSandboxResourceKey, TenantServiceKey, sandbox_backend_error};

pub type TenantServiceRetirementFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), Error>> + Send + 'a>>;

/// Explicit tenant-retirement effect capability retained for NNC6.5.
///
/// This seam can stop already-observed workloads and remove tenant artifacts.
/// It has no provision, activation, source-reservation, or provider-start
/// operation and is intentionally separate from [`crate::RuntimeServiceRegistry`].
pub trait TenantServiceRetirement: Send + Sync + 'static {
    fn retire_tenant_async<'a>(
        &'a self,
        tenant_id: &'a TenantId,
    ) -> TenantServiceRetirementFuture<'a>;
}

impl ServiceManager {
    /// Retire one already-observed service under an exact admitted decision.
    pub async fn retire_service_for_decision_async(
        &self,
        decision: &TenantIsolationDecision,
        service_name: &str,
    ) -> Result<Option<SandboxHandle>, Error> {
        decision.service_access(service_name, "sandbox-backed service retirement")?;
        let key = TenantServiceKey::new(decision.tenant_id(), service_name);
        let Some(observation) =
            self.service_definition_observation_for_tenant(decision.tenant_id(), service_name)
        else {
            return Ok(None);
        };
        let inspection = self
            .inspect_service_for_retirement(&key, &observation.handle)
            .await?;
        let Some(inspection) = inspection else {
            self.remove_service_observation(&key);
            return Ok(None);
        };
        if inspection.cleanup != SandboxCleanupObservation::Finalized {
            self.stop_service_observation(&key, &inspection.handle)
                .await?;
        }
        let mut stopped = inspection.handle;
        stopped.status = SandboxStatus::Stopped;
        stopped.published_endpoints.clear();
        self.record_service_handle(&key, &stopped).await?;
        self.remove_service_observation(&key);
        Ok(Some(stopped))
    }

    /// Retire one standalone desired source. Provider inspection exists only
    /// in this explicit effect seam; ordinary reads consume snapshots directly.
    pub async fn retire_sandbox_resource_async(
        &self,
        tenant_id: &TenantId,
        resource_id: &str,
    ) -> Result<Option<SandboxResourceSnapshot>, Error> {
        let Some(snapshot) = self.sandbox_resource_snapshot_for_tenant(tenant_id, resource_id)?
        else {
            return Ok(None);
        };
        let Some(observation) = snapshot.observation.as_ref() else {
            return Ok(Some(snapshot));
        };
        let inspection = self
            .sandbox_backend
            .inspect(&observation.handle.id)
            .await
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to inspect sandbox resource `{resource_id}` for tenant {tenant_id} during retirement: {error}"
                ))
            })?;
        let Some(inspection) = inspection else {
            self.state
                .lock()
                .expect("manager lock should not be poisoned")
                .sandbox_resource_observations
                .remove(&TenantSandboxResourceKey::new(tenant_id, resource_id));
            return self.sandbox_resource_snapshot_for_tenant(tenant_id, resource_id);
        };
        validate_sandbox_retirement_identity(
            tenant_id,
            resource_id,
            &observation.handle,
            &inspection,
        )?;
        if inspection.cleanup != SandboxCleanupObservation::Finalized {
            self.sandbox_backend
                .stop(&inspection.handle.id)
                .await
                .map_err(|error| {
                    Error::Internal(format!(
                        "failed to stop sandbox resource `{resource_id}` for tenant {tenant_id}: {error}"
                    ))
                })?;
        }
        let mut stopped = inspection.handle;
        stopped.status = SandboxStatus::Stopped;
        stopped.published_endpoints.clear();
        self.project_sandbox_resource_observation(
            tenant_id,
            resource_id,
            snapshot.source.generation,
            observation.execution.attempt_id(),
            stopped,
        )?;
        self.sandbox_resource_snapshot_for_tenant(tenant_id, resource_id)
    }

    /// Definition-delete retirement after the precise definition mutation gate
    /// has been claimed. Non-forced deletion never stops a running provider.
    pub(super) async fn retire_service_for_definition_delete(
        &self,
        key: &TenantServiceKey,
        force: bool,
    ) -> Result<(), Error> {
        let Some(observation) =
            self.service_definition_observation_for_tenant(&key.tenant_id, &key.service_name)
        else {
            return Ok(());
        };
        let inspection = self
            .inspect_service_for_retirement(key, &observation.handle)
            .await?;
        let Some(inspection) = inspection else {
            self.remove_service_observation(key);
            return Ok(());
        };
        let running = !matches!(
            inspection.handle.status,
            SandboxStatus::Stopped | SandboxStatus::Stopping | SandboxStatus::Failed
        );
        if running && !force {
            return Err(Error::conflict(format!(
                "service `{}` for tenant `{}` is running; retire it first or pass an authorized force delete policy",
                key.service_name, key.tenant_id
            )));
        }
        if running || inspection.cleanup != SandboxCleanupObservation::Finalized {
            self.stop_service_observation(key, &inspection.handle)
                .await?;
            let mut stopped = inspection.handle;
            stopped.status = SandboxStatus::Stopped;
            stopped.published_endpoints.clear();
            self.record_service_handle(key, &stopped).await?;
        }
        self.remove_service_observation(key);
        Ok(())
    }

    async fn inspect_service_for_retirement(
        &self,
        key: &TenantServiceKey,
        expected: &SandboxHandle,
    ) -> Result<Option<SandboxInspection>, Error> {
        let inspection = self
            .sandbox_backend
            .inspect(&expected.id)
            .await
            .map_err(|error| sandbox_backend_error(key, "inspect for retirement", &error))?;
        if let Some(inspection) = inspection.as_ref() {
            validate_service_retirement_identity(key, expected, inspection)?;
        }
        Ok(inspection)
    }

    async fn stop_service_observation(
        &self,
        key: &TenantServiceKey,
        handle: &SandboxHandle,
    ) -> Result<(), Error> {
        self.sandbox_backend
            .stop(&handle.id)
            .await
            .map_err(|error| sandbox_backend_error(key, "stop for retirement", &error))
    }

    fn remove_service_observation(&self, key: &TenantServiceKey) {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .service_definition_observations
            .remove(key);
    }
}

impl TenantServiceRetirement for ServiceManager {
    fn retire_tenant_async<'a>(
        &'a self,
        tenant_id: &'a TenantId,
    ) -> TenantServiceRetirementFuture<'a> {
        Box::pin(async move {
            let tenant_services = self.tenant_service_observations(tenant_id);
            let tenant_sandboxes = self.list_sandbox_resource_snapshots_for_tenant(tenant_id);
            let mut stopped_sandbox_ids = BTreeSet::new();
            let mut failed_sandbox_ids = BTreeSet::new();
            let mut retired_service_keys = BTreeSet::new();
            let mut retired_resource_ids = BTreeSet::new();
            let mut errors = Vec::new();

            for (key, handle) in &tenant_services {
                let sandbox_id = handle.id.as_str().to_owned();
                let stopped = stop_once(
                    self,
                    &sandbox_id,
                    &handle.id,
                    &mut stopped_sandbox_ids,
                    &mut failed_sandbox_ids,
                )
                .await;
                match stopped {
                    Ok(()) => {
                        let mut stopped_handle = handle.clone();
                        stopped_handle.status = SandboxStatus::Stopped;
                        stopped_handle.published_endpoints.clear();
                        match self.record_service_handle(key, &stopped_handle).await {
                            Ok(()) => {
                                retired_service_keys.insert(key.clone());
                            }
                            Err(error) => errors.push(format!(
                                "failed to record stopped handle for service {} in tenant {}: {error}",
                                key.service_name, key.tenant_id
                            )),
                        }
                    }
                    Err(error) => errors.push(
                        sandbox_backend_error(key, "stop during tenant retirement", &error)
                            .to_string(),
                    ),
                }
            }

            for snapshot in &tenant_sandboxes {
                let Some(observation) = snapshot.observation.as_ref() else {
                    retired_resource_ids.insert(snapshot.source.id.clone());
                    continue;
                };
                let handle = &observation.handle;
                match stop_once(
                    self,
                    handle.id.as_str(),
                    &handle.id,
                    &mut stopped_sandbox_ids,
                    &mut failed_sandbox_ids,
                )
                .await
                {
                    Ok(()) => {
                        retired_resource_ids.insert(snapshot.source.id.clone());
                    }
                    Err(error) => errors.push(
                        standalone_sandbox_retirement_error(tenant_id, handle, &error).to_string(),
                    ),
                }
            }

            if let Err(error) = self
                .sandbox_backend
                .remove_tenant_artifacts(tenant_id.clone())
                .await
            {
                errors.push(format!(
                    "failed to remove sandbox artifacts for tenant {tenant_id}: {error}"
                ));
            }

            let mut state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            for key in &retired_service_keys {
                state.service_definition_observations.remove(key);
                state.definition_mutations_in_progress.remove(key);
            }
            if errors.is_empty() {
                state
                    .service_definition_observations
                    .retain(|key, _| &key.tenant_id != tenant_id);
                state
                    .definitions
                    .retain(|key, _| &key.tenant_id != tenant_id);
                state
                    .sandbox_resource_sources
                    .retain(|key, _| &key.tenant_id != tenant_id);
                state
                    .sandbox_resource_observations
                    .retain(|key, _| &key.tenant_id != tenant_id);
                state
                    .sessions
                    .retain(|_, session| &session.tenant_id != tenant_id);
                state
                    .definition_mutations_in_progress
                    .retain(|key| &key.tenant_id != tenant_id);
            } else {
                state
                    .definitions
                    .retain(|key, _| !retired_service_keys.contains(key));
                state.sandbox_resource_sources.retain(|key, _| {
                    &key.tenant_id != tenant_id
                        || !retired_resource_ids.contains(key.resource_id.as_str())
                });
                state.sandbox_resource_observations.retain(|key, _| {
                    &key.tenant_id != tenant_id
                        || !retired_resource_ids.contains(key.resource_id.as_str())
                });
                state.sessions.retain(|_, session| match &session.target {
                    crate::SessionTarget::Service { name } => {
                        !retired_service_keys.contains(&TenantServiceKey::new(tenant_id, name))
                    }
                    crate::SessionTarget::Sandbox { id } => !retired_resource_ids.contains(id),
                });
            }
            drop(state);
            self.definition_mutation_notify.notify_waiters();
            if errors.is_empty() {
                Ok(())
            } else {
                Err(tenant_retirement_aggregate_error(tenant_id, errors))
            }
        })
    }
}

async fn stop_once(
    manager: &ServiceManager,
    sandbox_key: &str,
    sandbox_id: &nimbus_sandbox::SandboxId,
    stopped: &mut BTreeSet<String>,
    failed: &mut BTreeSet<String>,
) -> Result<(), nimbus_sandbox::SandboxError> {
    if stopped.contains(sandbox_key) {
        return Ok(());
    }
    if failed.contains(sandbox_key) {
        return Err(nimbus_sandbox::SandboxError::OperationFailed {
            message: format!("sandbox `{sandbox_key}` already failed retirement in this attempt"),
        });
    }
    match manager.sandbox_backend.stop(sandbox_id).await {
        Ok(()) => {
            stopped.insert(sandbox_key.to_owned());
            Ok(())
        }
        Err(error) => {
            failed.insert(sandbox_key.to_owned());
            Err(error)
        }
    }
}

fn validate_service_retirement_identity(
    key: &TenantServiceKey,
    expected: &SandboxHandle,
    inspection: &SandboxInspection,
) -> Result<(), Error> {
    let observed = &inspection.handle;
    if observed.id == expected.id
        && observed.tenant_id == key.tenant_id
        && observed.name == key.service_name
        && observed.backend == expected.backend
    {
        return Ok(());
    }
    Err(Error::Internal(format!(
        "sandbox backend returned crossed retirement identity for service {} tenant {}",
        key.service_name, key.tenant_id
    )))
}

fn validate_sandbox_retirement_identity(
    tenant_id: &TenantId,
    resource_id: &str,
    expected: &SandboxHandle,
    inspection: &SandboxInspection,
) -> Result<(), Error> {
    let observed = &inspection.handle;
    if observed.id == expected.id
        && observed.tenant_id == *tenant_id
        && observed.name == expected.name
        && observed.backend == expected.backend
    {
        return Ok(());
    }
    Err(Error::Internal(format!(
        "sandbox backend returned crossed retirement identity for resource `{resource_id}` tenant {tenant_id}"
    )))
}

fn tenant_retirement_aggregate_error(tenant_id: &TenantId, errors: Vec<String>) -> Error {
    Error::Internal(format!(
        "tenant {tenant_id} retirement failed after best-effort cleanup: {}",
        errors.join("; ")
    ))
}

fn standalone_sandbox_retirement_error(
    tenant_id: &TenantId,
    handle: &SandboxHandle,
    error: &nimbus_sandbox::SandboxError,
) -> Error {
    Error::Internal(format!(
        "failed to stop standalone sandbox {} for tenant {} during tenant retirement: {error}",
        handle.id, tenant_id
    ))
}
