//! Best-effort publication of immutable connectivity observations.
//!
//! This runtime retains typed observations and retries only `_nimbus`
//! document writes. It cannot bind sockets, call providers, or mutate desired
//! state and lease authority.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use nimbus_engine::Engine;
use nimbus_machine::{MachineConfigRecord, MachineStateRecord};
use tokio::sync::Notify;
use tokio::time::Instant;
use tracing::warn;

use crate::records::replace_server_port_listener_observations_async;
use crate::{
    SystemPortListenerObservation, SystemServiceConnectivityObservation,
    delete_machine_state_async, record_machine_state_async, record_port_listener_observation_async,
    record_service_connectivity_observation_async,
};

const RETRY_BASE: Duration = Duration::from_millis(25);
const RETRY_MAX: Duration = Duration::from_secs(2);
const REBUILD_INTERVAL: Duration = Duration::from_secs(30);

/// Retained, coalescing publication of rebuildable `_nimbus` observations.
#[derive(Clone)]
pub struct SystemConnectivityProjectionRuntime {
    inner: Arc<ProjectionRuntimeInner>,
}

struct ProjectionRuntimeInner {
    engine: Weak<Engine>,
    rebuild_interval: Duration,
    wake: Notify,
    state: Mutex<ProjectionRuntimeState>,
}

#[derive(Default)]
struct ProjectionRuntimeState {
    next_revision: u64,
    running: bool,
    entries: BTreeMap<String, ProjectionEntry>,
}

struct ProjectionEntry {
    revision: u64,
    operation: ProjectionOperation,
    dirty: bool,
    failures: u32,
    retry_at: Option<Instant>,
    rebuild_at: Option<Instant>,
}

#[derive(Clone)]
enum ProjectionOperation {
    PortListener(SystemPortListenerObservation),
    ServerListeners {
        server_incarnation: String,
        observations: Vec<SystemPortListenerObservation>,
    },
    ServiceConnectivity(SystemServiceConnectivityObservation),
    MachineState {
        config: MachineConfigRecord,
        state: MachineStateRecord,
    },
    MachineDeletion {
        name: String,
    },
}

impl SystemConnectivityProjectionRuntime {
    pub fn new(engine: &Arc<Engine>) -> Self {
        Self::with_rebuild_interval(engine, REBUILD_INTERVAL)
    }

    fn with_rebuild_interval(engine: &Arc<Engine>, rebuild_interval: Duration) -> Self {
        Self {
            inner: Arc::new(ProjectionRuntimeInner {
                engine: Arc::downgrade(engine),
                rebuild_interval,
                wake: Notify::new(),
                state: Mutex::new(ProjectionRuntimeState::default()),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_rebuild_interval_for_testing(
        engine: &Arc<Engine>,
        rebuild_interval: Duration,
    ) -> Self {
        Self::with_rebuild_interval(engine, rebuild_interval)
    }

    /// Retain the latest observation for one stable listener identity.
    pub fn project_port_listener(&self, observation: SystemPortListenerObservation) {
        self.submit(
            format!("listener:{}", observation.listener_id().as_str()),
            ProjectionOperation::PortListener(observation),
        );
    }

    /// Replace all physical listeners owned by this server incarnation.
    pub fn project_server_listeners(
        &self,
        server_incarnation: impl Into<String>,
        observations: impl IntoIterator<Item = SystemPortListenerObservation>,
    ) {
        self.submit(
            "server-listeners".to_owned(),
            ProjectionOperation::ServerListeners {
                server_incarnation: server_incarnation.into(),
                observations: observations.into_iter().collect(),
            },
        );
    }

    /// Retain the latest observation for one tenant-qualified service.
    pub fn project_service_connectivity(&self, observation: SystemServiceConnectivityObservation) {
        self.submit(
            format!(
                "service:{}:{}",
                observation.tenant_id().as_str(),
                observation.service_name()
            ),
            ProjectionOperation::ServiceConnectivity(observation),
        );
    }

    /// Retain the latest machine snapshot after the machine authority succeeds.
    pub fn project_machine_state(&self, config: MachineConfigRecord, state: MachineStateRecord) {
        self.submit(
            format!("machine:{}", config.name),
            ProjectionOperation::MachineState { config, state },
        );
    }

    /// Replace a retained machine snapshot with its observed deletion.
    pub fn project_machine_deletion(&self, name: impl Into<String>) {
        let name = name.into();
        self.submit(
            format!("machine:{name}"),
            ProjectionOperation::MachineDeletion { name },
        );
    }

    /// Re-publish every retained source snapshot after projection loss.
    pub fn rebuild(&self) {
        {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("projection state is not poisoned");
            let keys = state.entries.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                state.next_revision = state.next_revision.wrapping_add(1);
                let revision = state.next_revision;
                let entry = state
                    .entries
                    .get_mut(&key)
                    .expect("retained projection key should remain present");
                // A publication that started before this rebuild must not
                // acknowledge the new repair request.
                entry.revision = revision;
                entry.dirty = true;
                entry.failures = 0;
                entry.retry_at = None;
                entry.rebuild_at = None;
            }
        }
        self.inner.wake.notify_one();
        self.ensure_driver();
    }

    #[cfg(test)]
    pub(crate) fn retained_entry_count_for_testing(&self) -> usize {
        self.inner
            .state
            .lock()
            .expect("projection state is not poisoned")
            .entries
            .len()
    }

    #[cfg(test)]
    pub(crate) fn driver_running_for_testing(&self) -> bool {
        self.inner
            .state
            .lock()
            .expect("projection state is not poisoned")
            .running
    }

    fn submit(&self, key: String, operation: ProjectionOperation) {
        {
            let mut state = self
                .inner
                .state
                .lock()
                .expect("projection state is not poisoned");
            if state
                .entries
                .get(&key)
                .is_some_and(|current| !operation.supersedes(&current.operation))
            {
                return;
            }
            state.next_revision = state.next_revision.wrapping_add(1);
            let revision = state.next_revision;
            state.entries.insert(
                key,
                ProjectionEntry {
                    revision,
                    operation,
                    dirty: true,
                    failures: 0,
                    retry_at: None,
                    rebuild_at: None,
                },
            );
        }
        self.inner.wake.notify_one();
        self.ensure_driver();
    }

    fn ensure_driver(&self) {
        schedule_driver(&self.inner);
    }
}

fn schedule_driver(inner: &Arc<ProjectionRuntimeInner>) {
    let Some(engine) = inner.engine.upgrade() else {
        return;
    };
    {
        let mut state = inner
            .state
            .lock()
            .expect("projection state is not poisoned");
        if state.running || !state.entries.values().any(|entry| entry.dirty) {
            return;
        }
        state.running = true;
    }
    if let Err(error) = engine.try_spawn_observer_work(run_projection_driver(Arc::clone(inner))) {
        inner
            .state
            .lock()
            .expect("projection state is not poisoned")
            .running = false;
        warn!(%error, "could not schedule retained system connectivity projection");
    }
}

struct DriverGuard {
    inner: Arc<ProjectionRuntimeInner>,
    clear_on_drop: bool,
}

impl Drop for DriverGuard {
    fn drop(&mut self) {
        if !self.clear_on_drop {
            return;
        }
        self.inner
            .state
            .lock()
            .expect("projection state is not poisoned")
            .running = false;
    }
}

async fn run_projection_driver(inner: Arc<ProjectionRuntimeInner>) {
    let mut guard = DriverGuard {
        inner: Arc::clone(&inner),
        clear_on_drop: true,
    };
    loop {
        let Some(step) = next_driver_step(&inner) else {
            // `next_driver_step` cleared `running` under the submit lock.
            // Do not let this old driver clobber a successor scheduled after
            // that handoff.
            guard.clear_on_drop = false;
            return;
        };
        let candidate = match step {
            ProjectionDriverStep::Publish(candidate) => *candidate,
            ProjectionDriverStep::Wait(delay) => {
                tokio::select! {
                    () = tokio::time::sleep(delay) => {}
                    () = inner.wake.notified() => {}
                }
                continue;
            }
        };
        let Some(engine) = inner.engine.upgrade() else {
            return;
        };
        match candidate.operation.publish(&engine).await {
            Ok(()) => mark_success(&inner, &candidate.key, candidate.revision),
            Err(error) => {
                let delay = mark_failure(&inner, &candidate.key, candidate.revision);
                warn!(
                    %error,
                    projection_key = %candidate.key,
                    retry_millis = delay.as_millis(),
                    "system connectivity projection failed; retained observation will retry"
                );
            }
        }
    }
}

struct ProjectionCandidate {
    key: String,
    revision: u64,
    operation: ProjectionOperation,
}

enum ProjectionDriverStep {
    Publish(Box<ProjectionCandidate>),
    Wait(Duration),
}

fn next_driver_step(inner: &ProjectionRuntimeInner) -> Option<ProjectionDriverStep> {
    let mut state = inner
        .state
        .lock()
        .expect("projection state is not poisoned");
    let now = Instant::now();
    let ready = state.entries.iter().find(|(_, entry)| {
        if entry.dirty {
            entry.retry_at.is_none_or(|retry_at| retry_at <= now)
        } else {
            entry.rebuild_at.is_some_and(|rebuild_at| rebuild_at <= now)
        }
    });
    if let Some((key, entry)) = ready {
        return Some(ProjectionDriverStep::Publish(Box::new(
            ProjectionCandidate {
                key: key.clone(),
                revision: entry.revision,
                operation: entry.operation.clone(),
            },
        )));
    }
    let Some(next_at) = state
        .entries
        .values()
        .filter_map(|entry| {
            if entry.dirty {
                entry.retry_at
            } else {
                entry.rebuild_at
            }
        })
        .min()
    else {
        // Clear the scheduling guard while holding the same lock used by
        // submit. A concurrent submission therefore either becomes visible
        // above or observes `running = false` and starts the next driver.
        state.running = false;
        return None;
    };
    Some(ProjectionDriverStep::Wait(
        next_at.saturating_duration_since(now),
    ))
}

fn mark_success(inner: &ProjectionRuntimeInner, key: &str, revision: u64) {
    let mut state = inner
        .state
        .lock()
        .expect("projection state is not poisoned");
    if state.entries.get(key).is_some_and(|entry| {
        entry.revision == revision
            && matches!(entry.operation, ProjectionOperation::MachineDeletion { .. })
    }) {
        state.entries.remove(key);
        return;
    }
    if let Some(entry) = state.entries.get_mut(key)
        && entry.revision == revision
    {
        entry.dirty = false;
        entry.failures = 0;
        entry.retry_at = None;
        entry.rebuild_at = Some(Instant::now() + inner.rebuild_interval);
    }
}

fn mark_failure(inner: &ProjectionRuntimeInner, key: &str, revision: u64) -> Duration {
    let mut state = inner
        .state
        .lock()
        .expect("projection state is not poisoned");
    let Some(entry) = state.entries.get_mut(key) else {
        return RETRY_BASE;
    };
    if entry.revision != revision {
        return RETRY_BASE;
    }
    entry.failures = entry.failures.saturating_add(1);
    entry.dirty = true;
    entry.rebuild_at = None;
    let shift = entry.failures.saturating_sub(1).min(6);
    let delay = RETRY_BASE.saturating_mul(1_u32 << shift).min(RETRY_MAX);
    entry.retry_at = Some(Instant::now() + delay);
    delay
}

impl ProjectionOperation {
    fn supersedes(&self, current: &Self) -> bool {
        match (self, current) {
            (Self::PortListener(incoming), Self::PortListener(current)) => {
                (incoming.generation(), incoming.lease_epoch())
                    >= (current.generation(), current.lease_epoch())
            }
            (Self::ServerListeners { .. }, Self::ServerListeners { .. }) => true,
            (Self::ServiceConnectivity(incoming), Self::ServiceConnectivity(current)) => {
                let incoming_fence = (
                    incoming.source_generation(),
                    incoming.attachment().generation(),
                );
                let current_fence = (
                    current.source_generation(),
                    current.attachment().generation(),
                );
                match incoming_fence.cmp(&current_fence) {
                    std::cmp::Ordering::Greater => true,
                    std::cmp::Ordering::Less => false,
                    std::cmp::Ordering::Equal => {
                        service_listener_fences_supersede(incoming, current)
                    }
                }
            }
            _ => true,
        }
    }

    async fn publish(&self, engine: &Arc<Engine>) -> nimbus_core::Result<()> {
        match self {
            Self::PortListener(observation) => {
                record_port_listener_observation_async(engine, observation).await
            }
            Self::ServerListeners {
                server_incarnation,
                observations,
            } => {
                replace_server_port_listener_observations_async(
                    engine,
                    server_incarnation,
                    observations,
                )
                .await
            }
            Self::ServiceConnectivity(observation) => {
                record_service_connectivity_observation_async(engine, observation).await
            }
            Self::MachineState { config, state } => {
                record_machine_state_async(engine, config, state).await
            }
            Self::MachineDeletion { name } => delete_machine_state_async(engine, name).await,
        }
    }
}

fn service_listener_fences_supersede(
    incoming: &SystemServiceConnectivityObservation,
    current: &SystemServiceConnectivityObservation,
) -> bool {
    let listener_fences = |observation: &SystemServiceConnectivityObservation| {
        observation
            .endpoints()
            .iter()
            .map(|endpoint| {
                (
                    endpoint.listener().listener_id().clone(),
                    (
                        endpoint.listener().generation(),
                        endpoint.listener().lease_epoch(),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    let incoming = listener_fences(incoming);
    let current = listener_fences(current);
    if incoming.keys().ne(current.keys()) {
        return false;
    }
    incoming
        .iter()
        .all(|(listener_id, fence)| current.get(listener_id).is_some_and(|old| fence >= old))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebuild_revision_rejects_an_older_in_flight_success() {
        let key = "machine:rebuild-race".to_owned();
        let runtime = SystemConnectivityProjectionRuntime {
            inner: Arc::new(ProjectionRuntimeInner {
                engine: Weak::new(),
                rebuild_interval: REBUILD_INTERVAL,
                wake: Notify::new(),
                state: Mutex::new(ProjectionRuntimeState {
                    next_revision: 1,
                    running: false,
                    entries: BTreeMap::from([(
                        key.clone(),
                        ProjectionEntry {
                            revision: 1,
                            operation: ProjectionOperation::MachineDeletion {
                                name: "rebuild-race".to_owned(),
                            },
                            dirty: false,
                            failures: 0,
                            retry_at: None,
                            rebuild_at: Some(Instant::now() + REBUILD_INTERVAL),
                        },
                    )]),
                }),
            }),
        };

        runtime.rebuild();
        mark_success(&runtime.inner, &key, 1);

        let state = runtime
            .inner
            .state
            .lock()
            .expect("projection state is not poisoned");
        let entry = state
            .entries
            .get(&key)
            .expect("an older completion must not remove rebuilt work");
        assert_eq!(entry.revision, 2);
        assert!(entry.dirty, "the rebuild request must remain pending");
    }
}
