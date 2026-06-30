use std::collections::BTreeMap;

use nimbus_core::{Error, TenantId};
use nimbus_sandbox::{SandboxHandle, SandboxOwnerSpec, SandboxSpec, SandboxStatus};
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationDecision, TenantIsolationPolicyInput, WorkloadAttributes,
};
use nimbus_workloads::{DesiredWorkload, DesiredWorkloadState, DesiredWorkloadStore};

use crate::SandboxResource;

use super::ServiceManager;
use super::clock::{next_version, now_millis};

impl ServiceManager {
    pub async fn create_sandbox_resource_async(
        &self,
        tenant_id: &TenantId,
        profile: impl Into<String>,
        spec: SandboxSpec,
        labels: BTreeMap<String, String>,
    ) -> Result<SandboxResource, Error> {
        let context = TenantIsolationContext::system(tenant_id.clone(), "sandbox.resource.create");
        self.create_sandbox_resource_for_context_async(&context, profile, spec, labels)
            .await
    }

    pub async fn create_sandbox_resource_for_context_async(
        &self,
        isolation: &TenantIsolationContext,
        profile: impl Into<String>,
        spec: SandboxSpec,
        labels: BTreeMap<String, String>,
    ) -> Result<SandboxResource, Error> {
        let profile = profile.into();
        let decision = self.sandbox_resource_decision(isolation, &profile, &spec)?;
        self.create_sandbox_resource_for_decision_async(&decision, profile, spec, labels)
            .await
    }

    pub async fn create_sandbox_resource_for_decision_async(
        &self,
        decision: &TenantIsolationDecision,
        profile: impl Into<String>,
        spec: SandboxSpec,
        labels: BTreeMap<String, String>,
    ) -> Result<SandboxResource, Error> {
        let tenant_id = decision.tenant_id();
        let profile = profile.into();
        validate_sandbox_resource_spec(tenant_id, &spec)?;
        let actual_backend = self.sandbox_backend.kind();
        decision.ensure_sandbox_spec_matches(&spec, actual_backend, "standalone sandbox create")?;
        decision
            .network()
            .ensure_sandbox_egress_matches(&spec, "standalone sandbox create")?;
        decision
            .volumes()
            .ensure_sandbox_mounts_match(&spec, "standalone sandbox create")?;
        self.admit_sandbox_root(decision, &spec)?;
        let handle = self
            .sandbox_backend
            .start(spec.clone())
            .await
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to start sandbox resource for tenant {tenant_id}: {error}"
                ))
            })?;
        if handle.tenant_id != *tenant_id {
            let error = Error::InvalidInput(format!(
                "sandbox backend returned handle for tenant {}, but sandbox create requested tenant {tenant_id}",
                handle.tenant_id
            ));
            self.stop_started_sandbox_resource_after_create_error(
                &handle,
                "backend returned a mismatched tenant handle",
            )
            .await?;
            return Err(error);
        }

        let id = handle.id.as_str().to_owned();
        let desired =
            DesiredWorkload::sandbox(tenant_id.clone(), &id, DesiredWorkloadState::Running, 1)?;
        let duplicate_id = {
            let state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            state.sandbox_resources.contains_key(&id)
        };
        if duplicate_id {
            return Err(Error::Conflict(format!(
                "sandbox backend returned duplicate sandbox id `{id}`"
            )));
        }
        let now = now_millis();
        let mut handle = Some(handle);
        let mut spec = Some(spec);
        let inserted = {
            let mut state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            if state.sandbox_resources.contains_key(&id) {
                None
            } else {
                let resource = SandboxResource {
                    tenant_id: tenant_id.clone(),
                    id: id.clone(),
                    profile,
                    spec: spec
                        .take()
                        .expect("sandbox spec should be available before insertion"),
                    handle: handle
                        .take()
                        .expect("sandbox handle should be available before insertion"),
                    generation: 1,
                    resource_version: next_version(
                        &mut state.next_sandbox_resource_version,
                        "sandbox",
                    ),
                    created_at_millis: now,
                    updated_at_millis: now,
                    labels,
                };
                state
                    .desired_workloads
                    .upsert_desired_workload(desired.clone());
                state.sandbox_resources.insert(id.clone(), resource.clone());
                Some(resource)
            }
        };
        if let Some(resource) = inserted {
            return Ok(resource);
        }

        let error = Error::Conflict(format!(
            "sandbox backend returned duplicate sandbox id `{id}`"
        ));
        // The duplicate id is already associated with a tracked sandbox, so a
        // backend stop by id could stop an existing resource without stop
        // authorization. Return the conflict and leave backend ownership intact.
        Err(error)
    }

    async fn stop_started_sandbox_resource_after_create_error(
        &self,
        handle: &SandboxHandle,
        reason: &str,
    ) -> Result<(), Error> {
        if matches!(
            handle.status,
            SandboxStatus::Stopped | SandboxStatus::Stopping
        ) {
            return Ok(());
        }
        self.sandbox_backend.stop(&handle.id).await.map_err(|error| {
            Error::Internal(format!(
                "standalone sandbox create failed after backend start ({reason}); failed to stop untracked sandbox `{}`: {error}",
                handle.id.as_str()
            ))
        })
    }

    pub async fn get_sandbox_resource_async(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &str,
    ) -> Result<Option<SandboxResource>, Error> {
        let current = self.current_sandbox_resource(tenant_id, sandbox_id)?;
        let Some(current) = current else {
            return Ok(None);
        };
        let inspected = self
            .sandbox_backend
            .inspect(&current.handle.id)
            .await
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to inspect sandbox resource `{sandbox_id}` for tenant {tenant_id}: {error}"
                ))
            })?;
        let Some(handle) = inspected else {
            self.state
                .lock()
                .expect("manager lock should not be poisoned")
                .sandbox_resources
                .remove(sandbox_id);
            return Ok(None);
        };
        if handle.tenant_id != *tenant_id {
            return Err(Error::PermissionDenied(format!(
                "sandbox backend returned sandbox `{sandbox_id}` for tenant {}, but route requested tenant {tenant_id}",
                handle.tenant_id
            )));
        }

        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let Some(resource) = state.sandbox_resources.get_mut(sandbox_id) else {
            return Ok(None);
        };
        resource.handle = handle;
        resource.updated_at_millis = now_millis();
        Ok(Some(resource.clone()))
    }

    pub fn list_sandbox_resources_for_tenant(&self, tenant_id: &TenantId) -> Vec<SandboxResource> {
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .sandbox_resources
            .values()
            .filter(|resource| &resource.tenant_id == tenant_id)
            .cloned()
            .collect()
    }

    pub async fn stop_sandbox_resource_async(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &str,
    ) -> Result<Option<SandboxResource>, Error> {
        let Some(mut resource) = self.current_sandbox_resource(tenant_id, sandbox_id)? else {
            return Ok(None);
        };
        if !matches!(
            resource.handle.status,
            SandboxStatus::Stopped | SandboxStatus::Stopping
        ) {
            self.sandbox_backend
                .stop(&resource.handle.id)
                .await
                .map_err(|error| {
                    Error::Internal(format!(
                        "failed to stop sandbox resource `{sandbox_id}` for tenant {tenant_id}: {error}"
                    ))
                })?;
        }
        resource.handle.status = SandboxStatus::Stopped;
        resource.handle.published_endpoints.clear();
        resource.updated_at_millis = now_millis();
        let desired = DesiredWorkload::sandbox(
            tenant_id.clone(),
            sandbox_id,
            DesiredWorkloadState::Stopped,
            resource.generation,
        )?;

        {
            let mut state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            state.desired_workloads.upsert_desired_workload(desired);
            state
                .sandbox_resources
                .insert(sandbox_id.to_owned(), resource.clone());
        }
        Ok(Some(resource))
    }

    fn current_sandbox_resource(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &str,
    ) -> Result<Option<SandboxResource>, Error> {
        let Some(resource) = self
            .state
            .lock()
            .expect("manager lock should not be poisoned")
            .sandbox_resources
            .get(sandbox_id)
            .cloned()
        else {
            return Ok(None);
        };
        if &resource.tenant_id != tenant_id {
            return Ok(None);
        }
        Ok(Some(resource))
    }

    fn sandbox_resource_decision(
        &self,
        isolation: &TenantIsolationContext,
        profile: &str,
        spec: &SandboxSpec,
    ) -> Result<TenantIsolationDecision, Error> {
        isolation.admit_decision(
            TenantIsolationPolicyInput::new(
                WorkloadAttributes::sandbox(profile).with_sandbox_backend(spec.backend),
            )
            .with_image(self.manager_image_policy()),
        )
    }
}

fn validate_sandbox_resource_spec(tenant_id: &TenantId, spec: &SandboxSpec) -> Result<(), Error> {
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
