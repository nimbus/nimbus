use std::collections::BTreeSet;

use nimbus_core::{Error, TenantId};
use nimbus_sandbox::SandboxStatus;
use ulid::Ulid;

use crate::{
    BuiltInServiceSpec, ExternalServiceSpec, ServiceBackend, ServiceDefinition,
    ServiceDefinitionSource, SessionLifecycleState, SessionResource, SessionTarget,
    SessionTargetSnapshot,
};

use super::ServiceManager;
use super::clock::{next_version, now_millis};
use super::types::TenantServiceKey;

const DEFAULT_SESSION_TTL_MILLIS: u64 = 15 * 60 * 1000;
const MAX_SESSION_TTL_MILLIS: u64 = 60 * 60 * 1000;
const MAX_SESSION_CHANNELS: usize = 8;

impl ServiceManager {
    pub async fn open_session_async(
        &self,
        tenant_id: &TenantId,
        target: SessionTarget,
        channels: Vec<String>,
        requested_ttl_millis: Option<u64>,
    ) -> Result<SessionResource, Error> {
        validate_channels(&channels)?;
        let (target_snapshot, service_gate) = match &target {
            SessionTarget::Service { name } => {
                validate_path_segment("service name", name)?;
                let definition = self
                    .service_definition_for_tenant(tenant_id, name)
                    .ok_or_else(|| {
                        Error::NotFound(format!(
                            "service `{name}` was not found for tenant `{tenant_id}`"
                        ))
                    })?;
                let service_gate = ServiceSessionGate {
                    name: name.clone(),
                    generation: definition.generation,
                    source: definition.source,
                };
                validate_service_session_channels(&definition, &channels)?;
                (service_target_snapshot(definition), Some(service_gate))
            }
            SessionTarget::Sandbox { id } => {
                validate_path_segment("sandbox id", id)?;
                let resource = self
                    .get_sandbox_resource_async(tenant_id, id)
                    .await?
                    .ok_or_else(|| {
                        Error::NotFound(format!(
                            "sandbox `{id}` was not found for tenant `{tenant_id}`"
                        ))
                    })?;
                if resource.handle.status != SandboxStatus::Ready {
                    return Err(Error::Conflict(format!(
                        "sandbox `{id}` is {}; session open requires a ready sandbox target",
                        sandbox_lifecycle_state(resource.handle.status)
                    )));
                }
                validate_allowed_channels("sandbox", &["stdio", "files"], &channels)?;
                (
                    SessionTargetSnapshot::Sandbox {
                        id: resource.id,
                        generation: resource.generation,
                        profile: resource.profile,
                        backend: sandbox_backend_wire(resource.handle.backend),
                    },
                    None,
                )
            }
        };

        let ttl = requested_ttl_millis.unwrap_or(DEFAULT_SESSION_TTL_MILLIS);
        if ttl == 0 {
            return Err(Error::InvalidInput(
                "session requestedTtlMs must be greater than zero".to_owned(),
            ));
        }
        let ttl = ttl.min(MAX_SESSION_TTL_MILLIS);
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        if let Some(gate) = service_gate.as_ref() {
            let key = TenantServiceKey::new(tenant_id, &gate.name);
            if state.activations_in_progress.contains(&key) {
                return Err(Error::Conflict(format!(
                    "service `{}` for tenant `{tenant_id}` has a lifecycle operation in progress; retry session open after it reaches a stable state",
                    gate.name
                )));
            }
            if gate.source == ServiceDefinitionSource::Dynamic {
                let Some(current) = state.definitions.get(&key) else {
                    return Err(Error::NotFound(format!(
                        "service `{}` was deleted before the session could be opened for tenant `{tenant_id}`",
                        gate.name
                    )));
                };
                if current.generation != gate.generation {
                    return Err(Error::Conflict(format!(
                        "service `{}` changed while opening the session; retry against the latest service definition",
                        gate.name
                    )));
                }
            }
        }
        let now = now_millis();
        let resource_version = next_version(&mut state.next_session_version, "session");
        let id = next_session_id();
        let session = SessionResource {
            tenant_id: tenant_id.clone(),
            id: id.clone(),
            target,
            target_snapshot,
            channels,
            lifecycle_state: SessionLifecycleState::Open,
            generation: 1,
            resource_version,
            created_at_millis: now,
            updated_at_millis: now,
            expires_at_millis: now.saturating_add(ttl),
            closed_at_millis: None,
            close_reason: None,
        };
        state.sessions.insert(id, session.clone());
        Ok(session)
    }

    pub fn get_session(&self, tenant_id: &TenantId, session_id: &str) -> Option<SessionResource> {
        self.refresh_session_expiration(tenant_id, session_id)
    }

    pub fn list_sessions_for_tenant(&self, tenant_id: &TenantId) -> Vec<SessionResource> {
        self.refresh_tenant_session_expirations(tenant_id);
        self.state
            .lock()
            .expect("manager lock should not be poisoned")
            .sessions
            .values()
            .filter(|session| &session.tenant_id == tenant_id)
            .cloned()
            .collect()
    }

    pub fn close_session(
        &self,
        tenant_id: &TenantId,
        session_id: &str,
        reason: impl Into<String>,
    ) -> Option<SessionResource> {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let now = now_millis();
        let action = state.sessions.get(session_id).and_then(|session| {
            if &session.tenant_id != tenant_id {
                return None;
            }
            match session.lifecycle_state {
                SessionLifecycleState::Open if now >= session.expires_at_millis => {
                    Some(SessionCloseAction::Expire)
                }
                SessionLifecycleState::Open => Some(SessionCloseAction::Close),
                SessionLifecycleState::Closed | SessionLifecycleState::Expired => None,
            }
        });
        let next_resource_version = if action.is_some() {
            Some(next_version(&mut state.next_session_version, "session"))
        } else {
            None
        };
        let session = state.sessions.get_mut(session_id)?;
        if &session.tenant_id != tenant_id {
            return None;
        }
        match (action, next_resource_version) {
            (Some(SessionCloseAction::Expire), Some(next_resource_version)) => {
                session.lifecycle_state = SessionLifecycleState::Expired;
                session.generation = session.generation.saturating_add(1);
                session.resource_version = next_resource_version;
                session.updated_at_millis = now;
                session.closed_at_millis = Some(now);
                session.close_reason = Some("expired".to_owned());
            }
            (Some(SessionCloseAction::Close), Some(next_resource_version)) => {
                session.lifecycle_state = SessionLifecycleState::Closed;
                session.generation = session.generation.saturating_add(1);
                session.resource_version = next_resource_version;
                session.updated_at_millis = now;
                session.closed_at_millis = Some(now);
                session.close_reason = Some(reason.into());
            }
            _ => {}
        }
        Some(session.clone())
    }

    fn refresh_session_expiration(
        &self,
        tenant_id: &TenantId,
        session_id: &str,
    ) -> Option<SessionResource> {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let now = now_millis();
        let should_expire = state.sessions.get(session_id).is_some_and(|session| {
            &session.tenant_id == tenant_id
                && session.lifecycle_state == SessionLifecycleState::Open
                && now >= session.expires_at_millis
        });
        let next_resource_version = if should_expire {
            Some(next_version(&mut state.next_session_version, "session"))
        } else {
            None
        };
        let session = state.sessions.get_mut(session_id)?;
        if &session.tenant_id != tenant_id {
            return None;
        }
        if let Some(next_resource_version) = next_resource_version {
            session.lifecycle_state = SessionLifecycleState::Expired;
            session.generation = session.generation.saturating_add(1);
            session.resource_version = next_resource_version;
            session.updated_at_millis = now;
            session.closed_at_millis = Some(now);
            session.close_reason = Some("expired".to_owned());
        }
        Some(session.clone())
    }

    fn refresh_tenant_session_expirations(&self, tenant_id: &TenantId) {
        let ids = self
            .state
            .lock()
            .expect("manager lock should not be poisoned")
            .sessions
            .values()
            .filter(|session| &session.tenant_id == tenant_id)
            .map(|session| session.id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            let _ = self.refresh_session_expiration(tenant_id, &id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionCloseAction {
    Close,
    Expire,
}

struct ServiceSessionGate {
    name: String,
    generation: u64,
    source: ServiceDefinitionSource,
}

fn validate_channels(channels: &[String]) -> Result<(), Error> {
    if channels.is_empty() {
        return Err(Error::InvalidInput(
            "session open requires at least one channel".to_owned(),
        ));
    }
    if channels.len() > MAX_SESSION_CHANNELS {
        return Err(Error::InvalidInput(format!(
            "session open supports at most {MAX_SESSION_CHANNELS} channels"
        )));
    }
    let mut seen = BTreeSet::new();
    for channel in channels {
        validate_path_segment("session channel", channel)?;
        if !seen.insert(channel) {
            return Err(Error::InvalidInput(format!(
                "session channel `{channel}` is duplicated"
            )));
        }
    }
    Ok(())
}

fn validate_service_session_channels(
    definition: &ServiceDefinition,
    channels: &[String],
) -> Result<(), Error> {
    match &definition.backend {
        ServiceBackend::BuiltIn(spec) => validate_built_in_channels(spec, channels),
        ServiceBackend::Sandbox(_) => {
            validate_allowed_channels("sandbox-backed service", &["stdio", "files"], channels)
        }
        ServiceBackend::External(spec) => reject_external_channels(spec, channels),
    }
}

fn validate_built_in_channels(spec: &BuiltInServiceSpec, channels: &[String]) -> Result<(), Error> {
    match spec.provider() {
        "browser" => {
            validate_allowed_channels("built-in browser service", &["cdp", "page"], channels)
        }
        provider => Err(Error::InvalidInput(format!(
            "built-in service provider `{provider}` does not expose session channels"
        ))),
    }
}

fn reject_external_channels(spec: &ExternalServiceSpec, _channels: &[String]) -> Result<(), Error> {
    Err(Error::InvalidInput(format!(
        "external service endpoint `{}` does not expose session channels",
        spec.endpoint()
    )))
}

fn validate_allowed_channels(
    target: &str,
    allowed: &[&str],
    channels: &[String],
) -> Result<(), Error> {
    for channel in channels {
        if !allowed.iter().any(|allowed| allowed == channel) {
            return Err(Error::InvalidInput(format!(
                "{target} does not support session channel `{channel}`"
            )));
        }
    }
    Ok(())
}

fn service_target_snapshot(definition: ServiceDefinition) -> SessionTargetSnapshot {
    match definition.backend {
        ServiceBackend::BuiltIn(spec) => SessionTargetSnapshot::Service {
            name: definition.name,
            generation: definition.generation,
            backend: "builtIn".to_owned(),
            provider: Some(spec.provider().to_owned()),
        },
        ServiceBackend::Sandbox(_) => SessionTargetSnapshot::Service {
            name: definition.name,
            generation: definition.generation,
            backend: "sandbox".to_owned(),
            provider: None,
        },
        ServiceBackend::External(_) => SessionTargetSnapshot::Service {
            name: definition.name,
            generation: definition.generation,
            backend: "external".to_owned(),
            provider: None,
        },
    }
}

fn validate_path_segment(label: &str, value: &str) -> Result<(), Error> {
    if value.trim() != value || value.is_empty() {
        return Err(Error::InvalidInput(format!(
            "{label} must be non-empty and must not have leading or trailing whitespace"
        )));
    }
    if value.contains('/') {
        return Err(Error::InvalidInput(format!(
            "{label} must be a single path segment and cannot contain `/`"
        )));
    }
    Ok(())
}

fn sandbox_lifecycle_state(status: SandboxStatus) -> &'static str {
    match status {
        SandboxStatus::Ready => "ready",
        SandboxStatus::Stopped => "stopped",
        SandboxStatus::Failed => "failed",
        SandboxStatus::Stopping => "stopping",
        SandboxStatus::Starting => "starting",
        SandboxStatus::NotReady => "not_ready",
    }
}

fn sandbox_backend_wire(backend: nimbus_sandbox::SandboxBackendKind) -> String {
    match backend {
        nimbus_sandbox::SandboxBackendKind::Container => "container",
        nimbus_sandbox::SandboxBackendKind::Krun => "krun",
    }
    .to_owned()
}

fn next_session_id() -> String {
    format!("session-{}", Ulid::new().to_string().to_ascii_lowercase())
}
