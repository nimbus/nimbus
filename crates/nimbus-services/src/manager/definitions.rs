use std::collections::BTreeMap;
use std::time::Instant;

use nimbus_core::{Error, TenantId};
use nimbus_sandbox::SandboxStatus;
use nimbus_tenant::TenantVolumePolicyDecision;
use url::Url;

use crate::{
    HealthCheckPolicy, ServiceBackend, ServiceDefinition, ServiceDefinitionSource,
    SessionLifecycleState, SessionTarget,
};

use super::ServiceManager;
use super::clock::{next_version, now_millis};
use super::session_channels::close_session_channels;
use super::types::{ServiceManagerState, TenantServiceKey, sandbox_backend_error};

const SUPPORTED_BUILT_IN_PROVIDERS: &[&str] = &[
    "loadBalancer",
    "serviceDiscovery",
    "browser",
    "modelGateway",
];

/// The resolved inputs for activating one tenant's service: which backend to
/// start and the tenant's volume policy to enforce against the sandbox spec.
pub(super) struct ServiceActivationPlan {
    pub(super) backend: ServiceBackend,
    pub(super) volume_policy: TenantVolumePolicyDecision,
}

impl ServiceManager {
    pub fn service_definition_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceDefinition> {
        let key = TenantServiceKey::new(tenant_id, service_name);
        if let Some(definition) = self
            .state
            .lock()
            .expect("manager lock should not be poisoned")
            .definitions
            .get(&key)
            .cloned()
        {
            return Some(definition);
        }
        self.service_definitions
            .service_backend_for_tenant(tenant_id, service_name)
            .map(|backend| {
                ServiceDefinition::static_catalog(tenant_id.clone(), service_name, backend)
            })
    }

    pub fn service_definitions_for_tenant(&self, tenant_id: &TenantId) -> Vec<ServiceDefinition> {
        let mut definitions = self
            .service_definitions
            .service_backends_for_tenant(tenant_id)
            .into_iter()
            .map(|(name, backend)| {
                (
                    name.clone(),
                    ServiceDefinition::static_catalog(tenant_id.clone(), name, backend),
                )
            })
            .collect::<BTreeMap<_, _>>();

        for definition in self
            .state
            .lock()
            .expect("manager lock should not be poisoned")
            .definitions
            .values()
            .filter(|definition| &definition.tenant_id == tenant_id)
        {
            definitions.insert(definition.name.clone(), definition.clone());
        }

        definitions.into_values().collect()
    }

    pub fn create_service_definition(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        backend: ServiceBackend,
        labels: BTreeMap<String, String>,
    ) -> Result<ServiceDefinition, Error> {
        validate_service_name(service_name)?;
        validate_service_backend(tenant_id, service_name, &backend)?;
        let key = TenantServiceKey::new(tenant_id, service_name);

        if self
            .service_definitions
            .service_backend_for_tenant(tenant_id, service_name)
            .is_some()
        {
            return Err(Error::AlreadyExists(format!(
                "service `{service_name}` for tenant `{tenant_id}` is already declared by the static service catalog"
            )));
        }

        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        if state.definitions.contains_key(&key) {
            return Err(Error::AlreadyExists(format!(
                "service `{service_name}` for tenant `{tenant_id}` already exists"
            )));
        }
        let version = next_version(&mut state.next_definition_version, "svcdef");
        let now = now_millis();
        let definition = ServiceDefinition::dynamic(
            tenant_id.clone(),
            service_name,
            backend,
            1,
            version,
            now,
            labels,
        );
        state.definitions.insert(key, definition.clone());
        Ok(definition)
    }

    pub fn update_service_definition(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        expected_generation: u64,
        backend: ServiceBackend,
        labels: BTreeMap<String, String>,
    ) -> Result<ServiceDefinition, Error> {
        validate_service_name(service_name)?;
        validate_service_backend(tenant_id, service_name, &backend)?;
        let key = TenantServiceKey::new(tenant_id, service_name);
        if self
            .service_definitions
            .service_backend_for_tenant(tenant_id, service_name)
            .is_some()
        {
            return Err(Error::Conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` is static and cannot be updated through dynamic service definition routes"
            )));
        }

        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let Some(current) = state.definitions.get(&key).cloned() else {
            return Err(Error::NotFound(format!(
                "service `{service_name}` for tenant `{tenant_id}` was not found"
            )));
        };
        if current.generation != expected_generation {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` for tenant `{tenant_id}` has generation {}, but update expected generation {expected_generation}",
                current.generation
            )));
        }
        if state.activations_in_progress.contains(&key) || state.handles.contains_key(&key) {
            return Err(Error::Conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` has an active backend; stop the service before updating its definition"
            )));
        }

        let mut updated = current;
        updated.backend = backend;
        updated.generation = updated.generation.saturating_add(1);
        updated.resource_version = next_version(&mut state.next_definition_version, "svcdef");
        updated.updated_at_millis = now_millis();
        updated.labels = labels;
        state.definitions.insert(key, updated.clone());
        Ok(updated)
    }

    pub async fn delete_service_definition_async(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        expected_generation: u64,
        force: bool,
    ) -> Result<ServiceDefinition, Error> {
        validate_service_name(service_name)?;
        let key = TenantServiceKey::new(tenant_id, service_name);
        if self
            .service_definitions
            .service_backend_for_tenant(tenant_id, service_name)
            .is_some()
        {
            return Err(Error::Conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` is static and cannot be deleted through dynamic service definition routes"
            )));
        }

        let current = {
            let state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            state.definitions.get(&key).cloned()
        }
        .ok_or_else(|| {
            Error::NotFound(format!(
                "service `{service_name}` for tenant `{tenant_id}` was not found"
            ))
        })?;

        if current.generation != expected_generation {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` for tenant `{tenant_id}` has generation {}, but delete expected generation {expected_generation}",
                current.generation
            )));
        }

        self.claim_service_definition_delete(&key, force).await?;

        let post_claim_precondition = {
            let state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            if let Some(current) = state.definitions.get(&key) {
                if current.source != ServiceDefinitionSource::Dynamic {
                    Err(Error::Conflict(format!(
                        "service `{service_name}` for tenant `{tenant_id}` is static and cannot be deleted through dynamic service definition routes"
                    )))
                } else if current.generation != expected_generation {
                    Err(Error::PreconditionFailed(format!(
                        "service `{service_name}` for tenant `{tenant_id}` has generation {}, but delete expected generation {expected_generation}",
                        current.generation
                    )))
                } else {
                    Ok(())
                }
            } else {
                Err(Error::NotFound(format!(
                    "service `{service_name}` for tenant `{tenant_id}` was not found"
                )))
            }
        };
        if let Err(error) = post_claim_precondition {
            self.release_activation(&key);
            return Err(error);
        }

        if !force {
            let live_session_count = {
                let state = self
                    .state
                    .lock()
                    .expect("manager lock should not be poisoned");
                open_service_session_ids(&state, tenant_id, service_name).len()
            };
            if live_session_count > 0 {
                self.release_activation(&key);
                return Err(Error::Conflict(format!(
                    "service `{service_name}` for tenant `{tenant_id}` has {live_session_count} open session(s); close sessions first or pass an authorized force delete policy",
                )));
            }
        }

        let refreshed = match self.refresh_handle_async(&key).await {
            Ok(handle) => handle,
            Err(error) => {
                self.release_activation(&key);
                return Err(error);
            }
        };
        if let Some(handle) = refreshed.as_ref() {
            let running = !matches!(
                handle.status,
                SandboxStatus::Stopped | SandboxStatus::Stopping | SandboxStatus::Failed
            );
            if running && !force {
                self.release_activation(&key);
                return Err(Error::Conflict(format!(
                    "service `{service_name}` for tenant `{tenant_id}` is running; stop it first or pass an authorized force delete policy"
                )));
            }
            if running {
                self.sandbox_backend
                    .stop(&handle.id)
                    .await
                    .map_err(|error| {
                        self.release_activation(&key);
                        sandbox_backend_error(&key, "force delete stop", &error)
                    })?;
                let mut stopped_handle = handle.clone();
                stopped_handle.status = SandboxStatus::Stopped;
                stopped_handle.published_endpoints.clear();
                if let Err(error) = self.record_service_handle(&key, &stopped_handle).await {
                    self.release_activation(&key);
                    return Err(error);
                }
            }
        }

        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let Some(current) = state.definitions.get(&key).cloned() else {
            state.activations_in_progress.remove(&key);
            self.activation_notify.notify_waiters();
            return Err(Error::NotFound(format!(
                "service `{service_name}` for tenant `{tenant_id}` was not found"
            )));
        };
        if current.source != ServiceDefinitionSource::Dynamic {
            state.activations_in_progress.remove(&key);
            self.activation_notify.notify_waiters();
            return Err(Error::Conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` is static and cannot be deleted through dynamic service definition routes"
            )));
        }
        if current.generation != expected_generation {
            state.activations_in_progress.remove(&key);
            self.activation_notify.notify_waiters();
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` for tenant `{tenant_id}` has generation {}, but delete expected generation {expected_generation}",
                current.generation
            )));
        }
        let live_session_ids = open_service_session_ids(&state, tenant_id, service_name);
        if !live_session_ids.is_empty() && !force {
            state.activations_in_progress.remove(&key);
            self.activation_notify.notify_waiters();
            return Err(Error::Conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` has {} open session(s); close sessions first or pass an authorized force delete policy",
                live_session_ids.len()
            )));
        }
        state.handles.remove(&key);
        state.activations_in_progress.remove(&key);
        let removed = state.definitions.remove(&key).ok_or_else(|| {
            Error::NotFound(format!(
                "service `{service_name}` for tenant `{tenant_id}` was not found"
            ))
        })?;
        if force {
            close_open_service_sessions(&mut state, &live_session_ids, "service_force_deleted");
        }
        self.activation_notify.notify_waiters();
        Ok(removed)
    }

    pub(super) fn service_backend_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceBackend> {
        self.service_activation_for_tenant(tenant_id, service_name)
            .map(|definition| definition.backend)
    }

    pub(super) fn service_activation_for_tenant(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
    ) -> Option<ServiceActivationPlan> {
        let key = TenantServiceKey::new(tenant_id, service_name);
        if let Some(definition) = self
            .state
            .lock()
            .expect("manager lock should not be poisoned")
            .definitions
            .get(&key)
            .cloned()
        {
            return Some(ServiceActivationPlan {
                backend: definition.backend,
                volume_policy: TenantVolumePolicyDecision::default(),
            });
        }

        let backend = self
            .service_definitions
            .service_backend_for_tenant(tenant_id, service_name)?;
        let volume_policy = self
            .service_definitions
            .service_volume_policy_for_tenant(tenant_id, service_name);
        Some(ServiceActivationPlan {
            backend,
            volume_policy,
        })
    }

    async fn claim_service_definition_delete(
        &self,
        key: &TenantServiceKey,
        force: bool,
    ) -> Result<(), Error> {
        let deadline = Instant::now() + self.activation_timeout;
        loop {
            let notified = self.activation_notify.notified();
            {
                let mut state = self
                    .state
                    .lock()
                    .expect("manager lock should not be poisoned");
                if state.activations_in_progress.insert(key.clone()) {
                    return Ok(());
                }
                if !force {
                    return Err(Error::Conflict(format!(
                        "service `{}` for tenant `{}` has an activation in progress; retry after the service reaches a stable lifecycle state",
                        key.service_name, key.tenant_id
                    )));
                }
            }
            self.notify_activation_wait_observer();

            let now = Instant::now();
            if now >= deadline {
                return Err(Error::ResourceExhausted(format!(
                    "force delete for service `{}` for tenant `{}` could not acquire the service lifecycle slot before {:?}",
                    key.service_name, key.tenant_id, self.activation_timeout
                )));
            }
            let remaining = deadline.saturating_duration_since(now);
            if tokio::time::timeout(remaining, notified).await.is_err() {
                return Err(Error::ResourceExhausted(format!(
                    "force delete for service `{}` for tenant `{}` timed out waiting for activation to settle",
                    key.service_name, key.tenant_id
                )));
            }
        }
    }
}

fn open_service_session_ids(
    state: &ServiceManagerState,
    tenant_id: &TenantId,
    service_name: &str,
) -> Vec<String> {
    let now = now_millis();
    state
        .sessions
        .values()
        .filter(|session| {
            &session.tenant_id == tenant_id
                && session.lifecycle_state == SessionLifecycleState::Open
                && now < session.expires_at_millis
                && matches!(&session.target, SessionTarget::Service { name } if name == service_name)
        })
        .map(|session| session.id.clone())
        .collect()
}

fn close_open_service_sessions(
    state: &mut ServiceManagerState,
    session_ids: &[String],
    reason: &str,
) {
    let now = now_millis();
    for session_id in session_ids {
        let should_close = state
            .sessions
            .get(session_id)
            .is_some_and(|session| session.lifecycle_state == SessionLifecycleState::Open);
        if !should_close {
            continue;
        }
        let next_resource_version = next_version(&mut state.next_session_version, "session");
        if let Some(session) = state.sessions.get_mut(session_id) {
            session.lifecycle_state = SessionLifecycleState::Closed;
            session.generation = session.generation.saturating_add(1);
            session.resource_version = next_resource_version;
            session.updated_at_millis = now;
            session.closed_at_millis = Some(now);
            session.close_reason = Some(reason.to_owned());
        }
        close_session_channels(state, session_id, reason);
    }
}

fn validate_service_name(service_name: &str) -> Result<(), Error> {
    if service_name.trim() != service_name || service_name.is_empty() {
        return Err(Error::InvalidInput(
            "service name must be non-empty and must not have leading or trailing whitespace"
                .to_owned(),
        ));
    }
    if service_name.contains('/') {
        return Err(Error::InvalidInput(
            "service name must be a single path segment and cannot contain `/`".to_owned(),
        ));
    }
    Ok(())
}

fn validate_service_backend(
    tenant_id: &TenantId,
    service_name: &str,
    backend: &ServiceBackend,
) -> Result<(), Error> {
    match backend {
        ServiceBackend::Sandbox(spec) => {
            if &spec.tenant_id != tenant_id {
                return Err(Error::InvalidInput(format!(
                    "sandbox backend tenant {} does not match service route tenant {tenant_id}",
                    spec.tenant_id
                )));
            }
            if spec.service_name() != Some(service_name) {
                return Err(Error::InvalidInput(format!(
                    "sandbox backend owner {:?} does not match service `{service_name}`",
                    spec.owner
                )));
            }
            if spec.root.is_unspecified_rootfs() {
                return Err(Error::InvalidInput(format!(
                    "sandbox backend for service `{service_name}` must declare a rootfs or OCI image root"
                )));
            }
            if spec
                .mounts
                .iter()
                .any(|mount| mount.tenant_volume_name().is_some())
            {
                return Err(Error::InvalidInput(format!(
                    "dynamic sandbox service definition `{service_name}` cannot declare tenant volume mounts without an admitted service volume policy"
                )));
            }
        }
        ServiceBackend::BuiltIn(spec) => {
            if !SUPPORTED_BUILT_IN_PROVIDERS.contains(&spec.provider()) {
                return Err(Error::InvalidInput(format!(
                    "unsupported built-in service provider `{}`; supported providers: {}",
                    spec.provider(),
                    SUPPORTED_BUILT_IN_PROVIDERS.join(", ")
                )));
            }
        }
        ServiceBackend::External(spec) => {
            let endpoint = spec.endpoint();
            if endpoint.contains('\n') || endpoint.contains('\r') {
                return Err(Error::InvalidInput(format!(
                    "external service `{service_name}` endpoint URL must be a single line"
                )));
            }
            let endpoint_url = Url::parse(endpoint).map_err(|error| {
                Error::InvalidInput(format!(
                    "external service `{service_name}` endpoint must be an absolute http(s) URL with a host: {error}"
                ))
            })?;
            if !matches!(endpoint_url.scheme(), "http" | "https") {
                return Err(Error::InvalidInput(format!(
                    "external service `{service_name}` endpoint must use http or https"
                )));
            }
            if endpoint_url.host_str().is_none() {
                return Err(Error::InvalidInput(format!(
                    "external service `{service_name}` endpoint must include a host"
                )));
            }
            if !endpoint_url.username().is_empty() || endpoint_url.password().is_some() {
                return Err(Error::InvalidInput(format!(
                    "external service `{service_name}` endpoint must not embed credentials"
                )));
            }
            let HealthCheckPolicy::Http { path } = spec.health();
            if !path.starts_with('/') {
                return Err(Error::InvalidInput(format!(
                    "external service `{service_name}` health.path must start with `/`"
                )));
            }
        }
    }
    Ok(())
}
