//! Structured ownership for sibling wire-protocol listener tasks.
//!
//! The server composition root prepares sockets, guards, and projections. This
//! module takes ownership only after one adapter is ready to serve. It keeps
//! each task with its active lease so setup unwind, normal shutdown, and
//! unexpected task completion use the same abort, join, and settlement path.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;

use nimbus_engine::Engine;
use tokio::task::{Id, JoinError, JoinSet};

use crate::adapters::wire::{WireProtocolAdapter, WireProtocolTask};
use crate::listener_lease::ActiveServerListenerLease;

#[derive(Clone, Copy)]
struct TaskIdentity {
    ordinal: usize,
    adapter: &'static str,
    task: &'static str,
}

struct ListenerMember {
    adapter: &'static str,
    lease: ActiveServerListenerLease,
    prepared_tasks: Option<Vec<WireProtocolTask>>,
}

/// All live sibling wire listeners for one server invocation.
pub(crate) struct WireListenerGroup {
    tasks: JoinSet<io::Result<()>>,
    task_identities: HashMap<Id, TaskIdentity>,
    listeners: Vec<ListenerMember>,
    next_task_ordinal: usize,
    #[cfg(test)]
    test_task_handles: Vec<(TaskIdentity, tokio::task::AbortHandle)>,
}

impl WireListenerGroup {
    pub(crate) fn new() -> Self {
        Self {
            tasks: JoinSet::new(),
            task_identities: HashMap::new(),
            listeners: Vec::new(),
            next_task_ordinal: 0,
            #[cfg(test)]
            test_task_handles: Vec::new(),
        }
    }

    /// Construct one adapter's complete unspawned task set.
    ///
    /// Task construction happens before the concrete listener leaves this
    /// method's fail-closed ownership. A construction error or panic closes
    /// that socket and settles its lease before it returns.
    pub(crate) fn prepare(
        &mut self,
        adapter: Box<dyn WireProtocolAdapter>,
        listener: tokio::net::TcpListener,
        lease: ActiveServerListenerLease,
        engine: Arc<Engine>,
    ) -> io::Result<()> {
        let adapter_name = adapter.name();
        let tasks = match catch_unwind(AssertUnwindSafe(|| adapter.build_tasks(engine))) {
            Ok(Ok(tasks)) => tasks,
            Ok(Err(error)) => {
                return settle_rejected_listener(
                    listener,
                    lease,
                    io::Error::new(
                        error.kind(),
                        format!("{adapter_name} task construction failed: {error}"),
                    ),
                    adapter_name,
                );
            }
            Err(_) => {
                return settle_rejected_listener(
                    listener,
                    lease,
                    io::Error::other(format!("{adapter_name} task construction panicked")),
                    adapter_name,
                );
            }
        };
        let tasks = match catch_unwind(AssertUnwindSafe(|| tasks.bind_listener(listener))) {
            Ok(tasks) => tasks,
            Err(_) => {
                return settle_lease_after_confirmed_close(
                    lease,
                    io::Error::other(format!(
                        "{adapter_name} listener task construction panicked"
                    )),
                    adapter_name,
                );
            }
        };

        self.listeners.push(ListenerMember {
            adapter: adapter_name,
            lease,
            prepared_tasks: Some(tasks),
        });
        Ok(())
    }

    /// Spawn every prepared adapter only after the complete setup succeeds.
    pub(crate) fn activate(&mut self) {
        for member_index in 0..self.listeners.len() {
            let adapter = self.listeners[member_index].adapter;
            let tasks = self.listeners[member_index]
                .prepared_tasks
                .take()
                .expect("a listener group must activate each prepared member once");
            for task in tasks {
                self.spawn_task(adapter, task);
            }
        }
    }

    fn spawn_task(&mut self, adapter: &'static str, task: WireProtocolTask) {
        let ordinal = self.next_task_ordinal;
        self.next_task_ordinal += 1;
        let task_name = task.name;
        let abort = self.tasks.spawn(task.future);
        let task_id = abort.id();
        let identity = TaskIdentity {
            ordinal,
            adapter,
            task: task_name,
        };
        #[cfg(test)]
        self.test_task_handles.push((identity, abort));
        self.task_identities.insert(task_id, identity);
    }

    #[cfg(test)]
    async fn wait_for_all_tasks_finished(&self, deadline: Duration) {
        let finished = async {
            loop {
                if self
                    .test_task_handles
                    .iter()
                    .all(|(_, handle)| handle.is_finished())
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        };
        if tokio::time::timeout(deadline, finished).await.is_err() {
            let unfinished = self
                .test_task_handles
                .iter()
                .filter(|(_, handle)| !handle.is_finished())
                .map(|(identity, _)| format!("{}:{}", identity.adapter, identity.task))
                .collect::<Vec<_>>()
                .join(", ");
            panic!("listener-group tasks did not finish within {deadline:?}: {unfinished}");
        }
    }

    /// Run the main server until it returns or any sibling task terminates.
    pub(crate) async fn supervise<F>(&mut self, main_server: F) -> io::Result<()>
    where
        F: Future<Output = io::Result<()>>,
    {
        if self.tasks.is_empty() {
            return main_server.await;
        }
        tokio::pin!(main_server);
        tokio::select! {
            biased;
            result = &mut main_server => result,
            failure = self.wait_for_unexpected_exit() => Err(failure),
        }
    }

    async fn wait_for_unexpected_exit(&mut self) -> io::Error {
        let outcome = self
            .tasks
            .join_next_with_id()
            .await
            .expect("a supervised listener group must contain at least one task");
        task_outcome_error(outcome, &mut self.task_identities)
    }

    /// Stop every remaining child, observe every join, and settle every lease.
    ///
    /// All tasks receive cancellation before any join is awaited. Cleanup
    /// failures are appended in registration order and never hide the primary
    /// server or setup failure.
    pub(crate) async fn shutdown(mut self, mut result: io::Result<()>) -> io::Result<()> {
        // A setup failure reaches this path before activation. Dropping every
        // unpolled listener future closes all concrete sockets before any
        // durable lease settlement begins.
        for member in &mut self.listeners {
            drop(member.prepared_tasks.take());
        }
        self.tasks.abort_all();

        let mut task_failures = Vec::new();
        while let Some(outcome) = self.tasks.join_next_with_id().await {
            if let Some(failure) = cleanup_task_failure(outcome, &mut self.task_identities) {
                task_failures.push(failure);
            }
        }
        task_failures.sort_by_key(|failure| failure.ordinal);
        for failure in task_failures {
            result = append_cleanup_error(
                result,
                &format!(
                    "failed to stop sibling task {}:{}",
                    failure.adapter, failure.task
                ),
                failure.error,
            );
        }

        for member in self.listeners {
            if let Err(error) = member.lease.settle_after_confirmed_local_close() {
                result = append_cleanup_error(
                    result,
                    &format!(
                        "failed to settle {adapter} sibling listener lease after confirmed task closure",
                        adapter = member.adapter
                    ),
                    error,
                );
            }
        }
        result
    }
}

struct OrderedTaskFailure {
    ordinal: usize,
    adapter: &'static str,
    task: &'static str,
    error: io::Error,
}

fn task_outcome_error(
    outcome: Result<(Id, io::Result<()>), JoinError>,
    identities: &mut HashMap<Id, TaskIdentity>,
) -> io::Error {
    let (identity, outcome) = split_task_outcome(outcome, identities);
    match outcome {
        TaskOutcome::Returned(Ok(())) => io::Error::other(format!(
            "sibling task {}:{} exited unexpectedly without an error",
            identity.adapter, identity.task
        )),
        TaskOutcome::Returned(Err(error)) => io::Error::new(
            error.kind(),
            format!(
                "sibling task {}:{} failed: {error}",
                identity.adapter, identity.task
            ),
        ),
        TaskOutcome::Cancelled(error) => io::Error::other(format!(
            "sibling task {}:{} was cancelled unexpectedly: {error}",
            identity.adapter, identity.task
        )),
        TaskOutcome::Panicked(error) => io::Error::other(format!(
            "sibling task {}:{} panicked: {error}",
            identity.adapter, identity.task
        )),
    }
}

fn cleanup_task_failure(
    outcome: Result<(Id, io::Result<()>), JoinError>,
    identities: &mut HashMap<Id, TaskIdentity>,
) -> Option<OrderedTaskFailure> {
    let (identity, outcome) = split_task_outcome(outcome, identities);
    let error = match outcome {
        TaskOutcome::Cancelled(_) => return None,
        TaskOutcome::Returned(Ok(())) => {
            io::Error::other("task exited before listener-group shutdown")
        }
        TaskOutcome::Returned(Err(error)) | TaskOutcome::Panicked(error) => error,
    };
    Some(OrderedTaskFailure {
        ordinal: identity.ordinal,
        adapter: identity.adapter,
        task: identity.task,
        error,
    })
}

fn split_task_outcome(
    outcome: Result<(Id, io::Result<()>), JoinError>,
    identities: &mut HashMap<Id, TaskIdentity>,
) -> (TaskIdentity, TaskOutcome) {
    match outcome {
        Ok((id, result)) => (
            remove_task_identity(id, identities),
            TaskOutcome::Returned(result),
        ),
        Err(error) => {
            let identity = remove_task_identity(error.id(), identities);
            let result = if error.is_cancelled() {
                TaskOutcome::Cancelled(error)
            } else {
                TaskOutcome::Panicked(io::Error::other(error))
            };
            (identity, result)
        }
    }
}

enum TaskOutcome {
    Returned(io::Result<()>),
    Cancelled(JoinError),
    Panicked(io::Error),
}

fn remove_task_identity(id: Id, identities: &mut HashMap<Id, TaskIdentity>) -> TaskIdentity {
    identities.remove(&id).unwrap_or(TaskIdentity {
        ordinal: usize::MAX,
        adapter: "unknown-adapter",
        task: "unknown-task",
    })
}

fn settle_rejected_listener(
    listener: tokio::net::TcpListener,
    lease: ActiveServerListenerLease,
    primary: io::Error,
    adapter: &str,
) -> io::Result<()> {
    drop(listener);
    settle_lease_after_confirmed_close(lease, primary, adapter)
}

fn settle_lease_after_confirmed_close(
    lease: ActiveServerListenerLease,
    primary: io::Error,
    adapter: &str,
) -> io::Result<()> {
    match lease.settle_after_confirmed_local_close() {
        Ok(()) => Err(primary),
        Err(cleanup_error) => append_cleanup_error(
            Err(primary),
            &format!("failed to settle rejected {adapter} sibling listener"),
            cleanup_error,
        ),
    }
}

pub(crate) fn append_cleanup_error(
    result: io::Result<()>,
    context: &str,
    cleanup_error: io::Error,
) -> io::Result<()> {
    match result {
        Ok(()) => Err(io::Error::new(
            cleanup_error.kind(),
            format!("{context}: {cleanup_error}"),
        )),
        Err(primary) => Err(io::Error::new(
            primary.kind(),
            format!("{primary}; {context}: {cleanup_error}"),
        )),
    }
}

#[cfg(test)]
#[path = "listener_group/tests.rs"]
mod tests;
