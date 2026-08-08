//! Retained, keyed supervision for exact durable restart candidates.
//!
//! The supervisor owns task retention and duplicate suppression. The injected
//! coordinator owns durable restart coordination. Dropping a watch-facing
//! supervisor clone does not cancel work retained by another clone, and task
//! completion can remove only the exact token that started it.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_workloads::{WorkloadSagaKey, WorkloadSagaRecord};
use tokio::sync::{Notify, oneshot};
use tokio::task::JoinHandle;

use super::restart_watch::{RestartSupervisor, RestartTrack};

/// One asynchronous attempt to coordinate an exact durable restart record.
pub(super) type RestartCandidateFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

/// Compute-owned coordination capability invoked by retained tasks.
pub(super) trait RestartCandidateCoordinator: Send + Sync {
    fn coordinate(&self, record: WorkloadSagaRecord) -> RestartCandidateFuture<'_>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RestartCandidateFailure {
    key: WorkloadSagaKey,
    token: u64,
    message: String,
}

impl RestartCandidateFailure {
    pub(super) fn key(&self) -> &WorkloadSagaKey {
        &self.key
    }

    pub(super) fn message(&self) -> &str {
        &self.message
    }
}

enum RetainedRestartEntry {
    Active { token: u64, _task: JoinHandle<()> },
    Failed(RestartCandidateFailure),
}

struct RetainedRestartState {
    entries: Mutex<BTreeMap<WorkloadSagaKey, RetainedRestartEntry>>,
    next_token: AtomicU64,
    changed: Notify,
}

impl RetainedRestartState {
    fn lock_entries(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<WorkloadSagaKey, RetainedRestartEntry>>, String>
    {
        self.entries
            .lock()
            .map_err(|_| "retained restart supervisor state is poisoned".to_owned())
    }

    fn allocate_token(&self) -> Result<u64, String> {
        self.next_token
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| "retained restart supervisor exhausted task tokens".to_owned())
    }

    fn complete(&self, key: WorkloadSagaKey, token: u64, result: Result<(), String>) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        let owns_entry = matches!(
            entries.get(&key),
            Some(RetainedRestartEntry::Active {
                token: active_token,
                ..
            }) if *active_token == token
        );
        if !owns_entry {
            return;
        }
        match result {
            Ok(()) => {
                entries.remove(&key);
            }
            Err(message) => {
                entries.insert(
                    key.clone(),
                    RetainedRestartEntry::Failed(RestartCandidateFailure {
                        key,
                        token,
                        message,
                    }),
                );
            }
        }
        drop(entries);
        self.changed.notify_waiters();
    }

    fn remove_active(&self, key: &WorkloadSagaKey, token: u64) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        let owns_entry = matches!(
            entries.get(key),
            Some(RetainedRestartEntry::Active {
                token: active_token,
                ..
            }) if *active_token == token
        );
        if owns_entry {
            entries.remove(key);
            drop(entries);
            self.changed.notify_waiters();
        }
    }
}

/// Retains one restart task per stable tenant-qualified workload key.
#[derive(Clone)]
pub(super) struct RetainedRestartSupervisor {
    coordinator: Arc<dyn RestartCandidateCoordinator>,
    state: Arc<RetainedRestartState>,
}

impl RetainedRestartSupervisor {
    pub(super) fn new(coordinator: Arc<dyn RestartCandidateCoordinator>) -> Self {
        Self {
            coordinator,
            state: Arc::new(RetainedRestartState {
                entries: Mutex::new(BTreeMap::new()),
                next_token: AtomicU64::new(1),
                changed: Notify::new(),
            }),
        }
    }

    #[cfg(test)]
    pub(super) fn failure(
        &self,
        key: &WorkloadSagaKey,
    ) -> Result<Option<RestartCandidateFailure>, String> {
        Ok(match self.state.lock_entries()?.get(key) {
            Some(RetainedRestartEntry::Failed(failure)) => Some(failure.clone()),
            Some(RetainedRestartEntry::Active { .. }) | None => None,
        })
    }

    /// Permit a later durable sweep to retry one exact observed failure.
    #[cfg(test)]
    pub(super) fn clear_failure_for_retry(
        &self,
        failure: &RestartCandidateFailure,
    ) -> Result<bool, String> {
        let mut entries = self.state.lock_entries()?;
        let is_same_failure = matches!(
            entries.get(failure.key()),
            Some(RetainedRestartEntry::Failed(current)) if current.token == failure.token
        );
        if is_same_failure {
            entries.remove(failure.key());
        }
        drop(entries);
        if is_same_failure {
            self.state.changed.notify_waiters();
        }
        Ok(is_same_failure)
    }

    #[cfg(test)]
    pub(super) async fn wait_until_quiescent(&self) -> Result<(), String> {
        loop {
            let changed = self.state.changed.notified();
            let has_active = self
                .state
                .lock_entries()?
                .values()
                .any(|entry| matches!(entry, RetainedRestartEntry::Active { .. }));
            if !has_active {
                return Ok(());
            }
            changed.await;
        }
    }

    fn start(&self, record: WorkloadSagaRecord) -> Result<RestartTrack, String> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| "retained restart supervisor requires a Tokio runtime".to_owned())?;
        let key = record.key().clone();
        let mut entries = self.state.lock_entries()?;
        match entries.get(&key) {
            Some(RetainedRestartEntry::Active { .. }) => return Ok(RestartTrack::Joined),
            Some(RetainedRestartEntry::Failed(failure)) => {
                return Err(format!(
                    "retained restart candidate {} failed: {}",
                    failure.key().saga_id(),
                    failure.message()
                ));
            }
            None => {}
        }

        let token = self.state.allocate_token()?;
        let (start, started) = oneshot::channel();
        let state = Arc::clone(&self.state);
        let coordinator = Arc::clone(&self.coordinator);
        let task_key = key.clone();
        let task = runtime.spawn(async move {
            if started.await.is_err() {
                return;
            }
            let result = coordinator.coordinate(record).await;
            state.complete(task_key, token, result);
        });
        entries.insert(
            key.clone(),
            RetainedRestartEntry::Active { token, _task: task },
        );
        drop(entries);

        if start.send(()).is_err() {
            self.state.remove_active(&key, token);
            return Err("retained restart task stopped before coordination began".to_owned());
        }
        self.state.changed.notify_waiters();
        Ok(RestartTrack::Started)
    }
}

impl RestartSupervisor for RetainedRestartSupervisor {
    fn track(&self, record: WorkloadSagaRecord) -> Result<RestartTrack, String> {
        self.start(record)
    }
}

#[cfg(test)]
#[path = "restart_supervisor/tests.rs"]
mod tests;
