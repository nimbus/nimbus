use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use nimbus_core::{CommitEntry, TableName, TenantId};
use tokio::sync::mpsc;
#[cfg(any(test, feature = "test-hooks"))]
use tokio::sync::oneshot;

use crate::{Engine, tenant::TenantRuntime};

/// A durable mutation that has been applied to the tenant's serving state.
#[derive(Clone, Debug)]
pub struct CommittedMutationEvent {
    pub tenant_id: TenantId,
    pub commit: CommitEntry,
}

/// Observer for committed mutation events.
pub trait CommittedMutationObserver: Send + Sync {
    fn committed_mutation_applied(&self, event: CommittedMutationEvent);

    /// Waits for work that the callback spawned after accepting an event.
    ///
    /// The default is appropriate for callbacks that finish their work before
    /// returning. This hidden hook only extends the test flush seam; production
    /// dispatch never waits on it.
    #[doc(hidden)]
    fn flush_spawned_work_for_testing(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }
}

/// A table schema or collection metadata change applied to a tenant.
#[derive(Clone, Debug)]
pub struct TableSchemaChangeEvent {
    pub tenant_id: TenantId,
    pub table: TableName,
}

/// Observer for table schema or collection metadata changes.
pub trait TableSchemaChangeObserver: Send + Sync {
    fn table_schema_changed(&self, event: TableSchemaChangeEvent);
}

pub(super) type CommittedMutationObserverRegistry =
    HashMap<&'static str, Arc<dyn CommittedMutationObserver>>;
pub(super) type TableSchemaChangeObserverRegistry =
    HashMap<&'static str, Arc<dyn TableSchemaChangeObserver>>;

pub(crate) struct CommittedMutationObserverDispatch {
    observers: Vec<Arc<dyn CommittedMutationObserver>>,
    events: Vec<CommittedMutationEvent>,
}

pub(crate) enum CommittedMutationObserverMessage {
    Dispatch(CommittedMutationObserverDispatch),
    #[cfg(any(test, feature = "test-hooks"))]
    Fence(oneshot::Sender<()>),
    Close,
}

impl CommittedMutationObserverDispatch {
    pub(crate) fn event_count(&self) -> usize {
        self.events.len()
    }

    fn run(self) {
        for event in self.events {
            for observer in &self.observers {
                observer.committed_mutation_applied(event.clone());
            }
        }
    }
}

struct DispatchCompletion {
    runtime: std::sync::Weak<TenantRuntime>,
    event_count: usize,
}

impl Drop for DispatchCompletion {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime.complete_committed_mutation_observer_dispatch(self.event_count);
        }
    }
}

fn run_dispatch(
    dispatch: CommittedMutationObserverDispatch,
    runtime: &std::sync::Weak<TenantRuntime>,
) {
    let _completion = DispatchCompletion {
        runtime: runtime.clone(),
        event_count: dispatch.event_count(),
    };
    dispatch.run();
}

pub(crate) async fn run_committed_mutation_observer_dispatcher(
    mut receiver: mpsc::UnboundedReceiver<CommittedMutationObserverMessage>,
    runtime: std::sync::Weak<TenantRuntime>,
) {
    while let Some(message) = receiver.recv().await {
        match message {
            CommittedMutationObserverMessage::Dispatch(dispatch) => {
                run_dispatch(dispatch, &runtime)
            }
            #[cfg(any(test, feature = "test-hooks"))]
            CommittedMutationObserverMessage::Fence(completed) => {
                let _ = completed.send(());
            }
            CommittedMutationObserverMessage::Close => {
                receiver.close();
                while let Some(message) = receiver.recv().await {
                    match message {
                        CommittedMutationObserverMessage::Dispatch(dispatch) => {
                            run_dispatch(dispatch, &runtime)
                        }
                        #[cfg(any(test, feature = "test-hooks"))]
                        CommittedMutationObserverMessage::Fence(completed) => {
                            let _ = completed.send(());
                        }
                        CommittedMutationObserverMessage::Close => {}
                    }
                }
                break;
            }
        }
    }
    if let Some(runtime) = runtime.upgrade() {
        runtime.mark_committed_mutation_observers_drained();
    }
}

impl Engine {
    /// Installs a named committed-mutation observer.
    ///
    /// Calling this more than once with the same name is idempotent. The first
    /// observer wins so repeated router construction does not duplicate
    /// projection work for the same engine instance.
    ///
    /// Callbacks run serially on a per-tenant dispatcher and may synchronously
    /// perform nested writes. Publishing therefore never blocks on observer
    /// backlog. Installations must budget callback throughput so the queue
    /// stays below `NIMBUS_COMMITTED_OBSERVER_QUEUE_HIGH_WATERMARK` (3,072
    /// events by default) and its hard
    /// `NIMBUS_COMMITTED_OBSERVER_QUEUE_CAPACITY` (4,096 by default). A
    /// high-water crossing emits one warning until the queue recovers. A hard
    /// cap breach cannot safely block the publisher, so it loudly refuses the
    /// breaching dispatch, poisons and closes that tenant's dispatcher after
    /// accepted work drains, and exposes the failure in `MutationJournalStats`.
    /// Treat a poisoned dispatcher as a fatal health condition requiring
    /// operator intervention; already-durable observer events are not retried.
    pub fn install_committed_mutation_observer(
        &self,
        name: &'static str,
        observer: Arc<dyn CommittedMutationObserver>,
    ) {
        self.committed_mutation_observers
            .write()
            .expect("committed mutation observer registry lock should not be poisoned")
            .entry(name)
            .or_insert(observer);
    }

    /// Waits until every observer dispatch already accepted for `tenant_id`
    /// has completed, including work spawned by observers that implement the
    /// drain hook. This is a test seam for isolating process-wide commit faults
    /// from ordered observer work left by fixture setup.
    #[cfg(any(test, feature = "test-hooks"))]
    pub async fn flush_committed_mutation_observers_for_testing(
        &self,
        tenant_id: &TenantId,
    ) -> nimbus_core::Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime
            .flush_committed_mutation_observers_for_testing()
            .await?;
        let observers = self
            .committed_mutation_observers
            .read()
            .expect("committed mutation observer registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for observer in observers {
            observer.flush_spawned_work_for_testing().await;
        }
        Ok(())
    }

    pub(crate) fn enqueue_applied_commit_batch_observers(
        &self,
        runtime: Arc<TenantRuntime>,
        applied: &[CommitEntry],
    ) {
        let observers = self
            .committed_mutation_observers
            .read()
            .expect("committed mutation observer registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if observers.is_empty() {
            return;
        }
        let events = applied
            .iter()
            .filter(|commit| !commit.writes.is_empty())
            .cloned()
            .map(|commit| CommittedMutationEvent {
                tenant_id: runtime.tenant_id().clone(),
                commit,
            })
            .collect::<Vec<_>>();
        if events.is_empty() {
            return;
        }
        if let Err(error) = runtime.enqueue_committed_mutation_observer_dispatch(
            CommittedMutationObserverDispatch { observers, events },
        ) {
            tracing::error!(
                tenant = %runtime.tenant_id(),
                %error,
                "committed mutation observer dispatch was refused"
            );
        }
    }

    /// Installs a named table-schema observer.
    ///
    /// Calling this more than once with the same name is idempotent. The first
    /// observer wins so repeated router construction does not duplicate
    /// projection work for the same engine instance.
    pub fn install_table_schema_change_observer(
        &self,
        name: &'static str,
        observer: Arc<dyn TableSchemaChangeObserver>,
    ) {
        self.table_schema_change_observers
            .write()
            .expect("table schema change observer registry lock should not be poisoned")
            .entry(name)
            .or_insert(observer);
    }

    pub(crate) fn notify_table_schema_change_observers(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
    ) {
        let observers = self
            .table_schema_change_observers
            .read()
            .expect("table schema change observer registry lock should not be poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        if observers.is_empty() {
            return;
        }

        let event = TableSchemaChangeEvent {
            tenant_id: tenant_id.clone(),
            table: table.clone(),
        };
        for observer in observers {
            observer.table_schema_changed(event.clone());
        }
    }
}
