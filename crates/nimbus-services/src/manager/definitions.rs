use std::collections::BTreeMap;

use nimbus_core::{Error, TenantId};
use url::Url;

use crate::{
    HealthCheckPolicy, ServiceBackend, ServiceDefinition, ServiceDefinitionSource,
    SessionLifecycleState, SessionTarget,
};

use super::ServiceManager;
use super::clock::{next_version, now_millis};
use super::session_channels::close_session_channels;
use super::types::{ServiceManagerState, TenantServiceKey};

const SUPPORTED_BUILT_IN_PROVIDERS: &[&str] = &[
    "loadBalancer",
    "serviceDiscovery",
    "browser",
    "modelGateway",
];

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
            .service_definition_for_tenant(tenant_id, service_name)
    }

    pub fn service_definitions_for_tenant(&self, tenant_id: &TenantId) -> Vec<ServiceDefinition> {
        let mut definitions = self
            .service_definitions
            .service_definitions_for_tenant(tenant_id);

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
            .service_definition_for_tenant(tenant_id, service_name)
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
            .service_definition_for_tenant(tenant_id, service_name)
            .is_some()
        {
            return Err(Error::conflict(format!(
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
        if state.definition_mutations_in_progress.contains(&key)
            || state.service_definition_observations.contains_key(&key)
        {
            return Err(Error::conflict(format!(
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
            .service_definition_for_tenant(tenant_id, service_name)
            .is_some()
        {
            return Err(Error::conflict(format!(
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

        let _claim = self.claim_definition_mutation_guard(&key, force).await?;
        self.delete_service_definition_claimed_async(
            tenant_id,
            service_name,
            expected_generation,
            force,
            &key,
        )
        .await
    }

    async fn delete_service_definition_claimed_async(
        &self,
        tenant_id: &TenantId,
        service_name: &str,
        expected_generation: u64,
        force: bool,
        key: &TenantServiceKey,
    ) -> Result<ServiceDefinition, Error> {
        let post_claim_precondition = {
            let state = self
                .state
                .lock()
                .expect("manager lock should not be poisoned");
            if let Some(current) = state.definitions.get(key) {
                if current.source != ServiceDefinitionSource::Dynamic {
                    Err(Error::conflict(format!(
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
        post_claim_precondition?;

        if !force {
            let live_session_count = {
                let state = self
                    .state
                    .lock()
                    .expect("manager lock should not be poisoned");
                open_service_session_ids(&state, tenant_id, service_name).len()
            };
            if live_session_count > 0 {
                return Err(Error::conflict(format!(
                    "service `{service_name}` for tenant `{tenant_id}` has {live_session_count} open session(s); close sessions first or pass an authorized force delete policy",
                )));
            }
        }

        self.retire_service_for_definition_delete(key, force)
            .await?;

        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let Some(current) = state.definitions.get(key).cloned() else {
            return Err(Error::NotFound(format!(
                "service `{service_name}` for tenant `{tenant_id}` was not found"
            )));
        };
        if current.source != ServiceDefinitionSource::Dynamic {
            return Err(Error::conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` is static and cannot be deleted through dynamic service definition routes"
            )));
        }
        if current.generation != expected_generation {
            return Err(Error::PreconditionFailed(format!(
                "service `{service_name}` for tenant `{tenant_id}` has generation {}, but delete expected generation {expected_generation}",
                current.generation
            )));
        }
        let live_session_ids = open_service_session_ids(&state, tenant_id, service_name);
        if !live_session_ids.is_empty() && !force {
            return Err(Error::conflict(format!(
                "service `{service_name}` for tenant `{tenant_id}` has {} open session(s); close sessions first or pass an authorized force delete policy",
                live_session_ids.len()
            )));
        }
        state.service_definition_observations.remove(key);
        let removed = state.definitions.remove(key).ok_or_else(|| {
            Error::NotFound(format!(
                "service `{service_name}` for tenant `{tenant_id}` was not found"
            ))
        })?;
        if force {
            close_open_service_sessions(&mut state, &live_session_ids, "service_force_deleted");
        }
        Ok(removed)
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
