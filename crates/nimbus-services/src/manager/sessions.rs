use std::collections::BTreeSet;

use nimbus_core::{Error, TenantId};
use nimbus_sandbox::SandboxStatus;
use nimbus_workloads::{WorkloadChannelDescriptor, WorkloadExecutor};
use ulid::Ulid;

use crate::{
    BuiltInServiceSpec, ExternalServiceSpec, ServiceBackend, ServiceDefinition,
    ServiceDefinitionSource, SessionLifecycleState, SessionResource, SessionTarget,
    SessionTargetSnapshot,
};

use super::ServiceManager;
use super::clock::{next_version, now_millis};
use super::session_channels::{
    SessionBroker, SessionChannelKey, SessionChannelState, close_session_channels,
    initialize_session_channels,
};
use super::types::{ServiceManagerState, TenantSandboxResourceKey, TenantServiceKey};

const DEFAULT_SESSION_TTL_MILLIS: u64 = 15 * 60 * 1000;
const MAX_SESSION_TTL_MILLIS: u64 = 60 * 60 * 1000;
const MAX_SESSION_CHANNELS: usize = 8;

impl ServiceManager {
    pub async fn open_session_with_broker_async<E>(
        &self,
        tenant_id: &TenantId,
        target: SessionTarget,
        channels: Vec<String>,
        requested_ttl_millis: Option<u64>,
        executor: &mut E,
    ) -> Result<(SessionResource, Vec<WorkloadChannelDescriptor>), Error>
    where
        E: WorkloadExecutor + ?Sized,
    {
        let session = self
            .open_session_async(tenant_id, target, channels, requested_ttl_millis)
            .await?;
        let mut broker = SessionBroker::new(executor);
        match broker.open_session_channels(&session) {
            Ok(descriptors) => Ok((session, descriptors)),
            Err(error) => {
                let _ = self.close_session(tenant_id, &session.id, "channel_open_failed");
                Err(error)
            }
        }
    }

    pub async fn open_session_async(
        &self,
        tenant_id: &TenantId,
        target: SessionTarget,
        channels: Vec<String>,
        requested_ttl_millis: Option<u64>,
    ) -> Result<SessionResource, Error> {
        validate_channels(&channels)?;
        let (target_snapshot, commit_gate) = match &target {
            SessionTarget::Service { name } => {
                validate_path_segment("service name", name)?;
                let definition = self
                    .service_definition_for_tenant(tenant_id, name)
                    .ok_or_else(|| {
                        Error::NotFound(format!(
                            "service `{name}` was not found for tenant `{tenant_id}`"
                        ))
                    })?;
                validate_service_session_channels(&definition, &channels)?;
                let expected_observation = if matches!(
                    definition.backend,
                    ServiceBackend::Sandbox(_)
                ) {
                    let observation = self
                        .service_definition_observation_for_tenant(tenant_id, name)
                        .ok_or_else(|| {
                            Error::conflict(format!(
                                "sandbox-backed service `{name}` has no observed ready generation"
                            ))
                        })?;
                    if observation.observed_generation != definition.generation
                        || observation.handle.status != SandboxStatus::Ready
                    {
                        return Err(Error::conflict(format!(
                            "sandbox-backed service `{name}` observation is not ready for generation {}",
                            definition.generation
                        )));
                    }
                    Some(observation.handle)
                } else {
                    None
                };
                let service_gate = SessionCommitGate::Service {
                    name: name.clone(),
                    generation: definition.generation,
                    resource_version: definition.resource_version.clone(),
                    source: definition.source,
                    expected_observation,
                };
                (service_target_snapshot(definition), service_gate)
            }
            SessionTarget::Sandbox { id } => {
                validate_path_segment("sandbox id", id)?;
                let snapshot = self
                    .sandbox_resource_snapshot_for_tenant(tenant_id, id)?
                    .ok_or_else(|| {
                        Error::NotFound(format!(
                            "sandbox `{id}` was not found for tenant `{tenant_id}`"
                        ))
                    })?;
                let observation = snapshot.observation.as_ref().ok_or_else(|| {
                    Error::conflict(format!(
                        "sandbox `{id}` is pending; session open requires a ready sandbox target"
                    ))
                })?;
                if observation.handle.status != SandboxStatus::Ready {
                    return Err(Error::conflict(format!(
                        "sandbox `{id}` is {}; session open requires a ready sandbox target",
                        sandbox_lifecycle_state(observation.handle.status)
                    )));
                }
                validate_allowed_channels("sandbox", &["stdio", "files"], &channels)?;
                (
                    SessionTargetSnapshot::Sandbox {
                        id: snapshot.source.id.clone(),
                        generation: snapshot.source.generation,
                        profile: snapshot.source.profile.clone(),
                        backend: sandbox_backend_wire(observation.handle.backend),
                    },
                    SessionCommitGate::Sandbox {
                        id: snapshot.source.id,
                        generation: snapshot.source.generation,
                        resource_version: snapshot.source.resource_version,
                        expected_observation: observation.handle.clone(),
                    },
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
        validate_session_commit_gate(&state, tenant_id, &commit_gate)?;
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
        initialize_session_channels(&mut state, &session)?;
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
        let channel_close_reason = match action {
            Some(SessionCloseAction::Expire) => Some("expired".to_owned()),
            Some(SessionCloseAction::Close) => Some(reason.into()),
            None => None,
        };
        let session = {
            let session = state.sessions.get_mut(session_id)?;
            if &session.tenant_id != tenant_id {
                return None;
            }
            match (action, next_resource_version, channel_close_reason.as_ref()) {
                (Some(SessionCloseAction::Expire), Some(next_resource_version), Some(reason)) => {
                    session.lifecycle_state = SessionLifecycleState::Expired;
                    session.generation = session.generation.saturating_add(1);
                    session.resource_version = next_resource_version;
                    session.updated_at_millis = now;
                    session.closed_at_millis = Some(now);
                    session.close_reason = Some(reason.clone());
                }
                (Some(SessionCloseAction::Close), Some(next_resource_version), Some(reason)) => {
                    session.lifecycle_state = SessionLifecycleState::Closed;
                    session.generation = session.generation.saturating_add(1);
                    session.resource_version = next_resource_version;
                    session.updated_at_millis = now;
                    session.closed_at_millis = Some(now);
                    session.close_reason = Some(reason.clone());
                }
                _ => {}
            }
            session.clone()
        };
        if let Some(reason) = channel_close_reason {
            close_session_channels(&mut state, session_id, &reason);
        }
        Some(session)
    }

    pub fn ensure_session_channel_target_generation(
        &self,
        tenant_id: &TenantId,
        session_id: &str,
        channel: &str,
        current_generation: u64,
    ) -> Result<(), Error> {
        let state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let channel_state = state
            .session_channels
            .get(&SessionChannelKey::new(session_id, channel))
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "session channel `{channel}` for session `{session_id}` was not found"
                ))
            })?;
        if &channel_state.tenant_id != tenant_id {
            return Err(Error::NotFound(format!(
                "session channel `{channel}` for session `{session_id}` was not found"
            )));
        }
        channel_state.ensure_target_generation(current_generation)
    }

    pub fn half_close_session_channel_client_write(
        &self,
        tenant_id: &TenantId,
        session_id: &str,
        channel: &str,
        reason: impl Into<String>,
    ) -> Result<(), Error> {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let channel_state = mutable_session_channel(&mut state, tenant_id, session_id, channel)?;
        channel_state.half_close_client_write(reason);
        Ok(())
    }

    pub fn enqueue_session_channel_target_to_client_bytes(
        &self,
        tenant_id: &TenantId,
        session_id: &str,
        channel: &str,
        bytes: usize,
    ) -> Result<(), Error> {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let channel_state = mutable_session_channel(&mut state, tenant_id, session_id, channel)?;
        channel_state.enqueue_target_to_client_bytes(bytes)
    }

    pub fn drain_session_channel_target_to_client_bytes(
        &self,
        tenant_id: &TenantId,
        session_id: &str,
        channel: &str,
        bytes: usize,
    ) -> Result<(), Error> {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let channel_state = mutable_session_channel(&mut state, tenant_id, session_id, channel)?;
        channel_state.drain_target_to_client_bytes(bytes);
        Ok(())
    }

    pub fn disconnect_session_channel(
        &self,
        tenant_id: &TenantId,
        session_id: &str,
        channel: &str,
        reason: impl Into<String>,
    ) -> Result<(), Error> {
        let mut state = self
            .state
            .lock()
            .expect("manager lock should not be poisoned");
        let channel_state = mutable_session_channel(&mut state, tenant_id, session_id, channel)?;
        channel_state.disconnect(reason);
        Ok(())
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
        let expired = next_resource_version.is_some();
        let session = {
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
            session.clone()
        };
        if expired {
            close_session_channels(&mut state, session_id, "expired");
        }
        Some(session)
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

#[derive(Debug, Clone)]
pub(super) enum SessionCommitGate {
    Service {
        name: String,
        generation: u64,
        resource_version: String,
        source: ServiceDefinitionSource,
        expected_observation: Option<nimbus_sandbox::SandboxHandle>,
    },
    Sandbox {
        id: String,
        generation: u64,
        resource_version: String,
        expected_observation: nimbus_sandbox::SandboxHandle,
    },
}

pub(super) fn validate_session_commit_gate(
    state: &ServiceManagerState,
    tenant_id: &TenantId,
    gate: &SessionCommitGate,
) -> Result<(), Error> {
    match gate {
        SessionCommitGate::Service {
            name,
            generation,
            resource_version,
            source,
            expected_observation,
        } => {
            let key = TenantServiceKey::new(tenant_id, name);
            if state.definition_mutations_in_progress.contains(&key) {
                return Err(Error::conflict(format!(
                    "service `{name}` for tenant `{tenant_id}` has a definition mutation in progress; retry session open after it reaches a stable state"
                )));
            }
            if *source == ServiceDefinitionSource::Dynamic {
                let Some(current) = state.definitions.get(&key) else {
                    return Err(Error::NotFound(format!(
                        "service `{name}` was deleted before the session could be opened for tenant `{tenant_id}`"
                    )));
                };
                if current.generation != *generation
                    || current.resource_version != *resource_version
                {
                    return Err(Error::conflict(format!(
                        "service `{name}` changed while opening the session; retry against the latest service definition"
                    )));
                }
            }
            if let Some(expected) = expected_observation {
                let Some(current) = state.service_definition_observations.get(&key) else {
                    return Err(Error::conflict(format!(
                        "sandbox-backed service `{name}` lost its ready observation while opening the session"
                    )));
                };
                if current.observed_generation != *generation
                    || current.handle.status != SandboxStatus::Ready
                    || current.handle != *expected
                {
                    return Err(Error::conflict(format!(
                        "sandbox-backed service `{name}` observation changed while opening the session"
                    )));
                }
            }
        }
        SessionCommitGate::Sandbox {
            id,
            generation,
            resource_version,
            expected_observation,
        } => {
            let key = TenantSandboxResourceKey::new(tenant_id, id);
            let Some(source) = state.sandbox_resource_sources.get(&key) else {
                return Err(Error::NotFound(format!(
                    "sandbox `{id}` was deleted before the session could be opened for tenant `{tenant_id}`"
                )));
            };
            let Some(observation) = state.sandbox_resource_observations.get(&key) else {
                return Err(Error::conflict(format!(
                    "sandbox `{id}` lost its ready observation while opening the session"
                )));
            };
            if source.generation != *generation
                || source.resource_version != *resource_version
                || observation.observed_generation != *generation
                || observation.handle.status != SandboxStatus::Ready
                || observation.handle != *expected_observation
            {
                return Err(Error::conflict(format!(
                    "sandbox `{id}` source or observation changed while opening the session"
                )));
            }
        }
    }
    Ok(())
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

fn mutable_session_channel<'a>(
    state: &'a mut ServiceManagerState,
    tenant_id: &TenantId,
    session_id: &str,
    channel: &str,
) -> Result<&'a mut SessionChannelState, Error> {
    let channel_state = state
        .session_channels
        .get_mut(&SessionChannelKey::new(session_id, channel))
        .ok_or_else(|| {
            Error::NotFound(format!(
                "session channel `{channel}` for session `{session_id}` was not found"
            ))
        })?;
    if &channel_state.tenant_id != tenant_id {
        return Err(Error::NotFound(format!(
            "session channel `{channel}` for session `{session_id}` was not found"
        )));
    }
    Ok(channel_state)
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
