use std::collections::HashMap;
use std::sync::Arc;

use nimbus_core::{CommitEntry, TableName, TenantId};

use crate::{Service, tenant::TenantRuntime};

/// A durable mutation that has been applied to the tenant's serving state.
#[derive(Clone, Debug)]
pub struct CommittedMutationEvent {
    pub tenant_id: TenantId,
    pub commit: CommitEntry,
}

/// Observer for committed mutation events.
pub trait CommittedMutationObserver: Send + Sync {
    fn committed_mutation_applied(&self, event: CommittedMutationEvent);
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

impl Service {
    /// Installs a named committed-mutation observer.
    ///
    /// Calling this more than once with the same name is idempotent. The first
    /// observer wins so repeated router construction does not duplicate
    /// projection work for the same service instance.
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

    pub(crate) fn notify_committed_mutation_observers(
        &self,
        runtime: &TenantRuntime,
        commit: &CommitEntry,
    ) {
        if commit.writes.is_empty() {
            return;
        }
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

        let event = CommittedMutationEvent {
            tenant_id: runtime.tenant_id().clone(),
            commit: commit.clone(),
        };
        for observer in observers {
            observer.committed_mutation_applied(event.clone());
        }
    }

    /// Installs a named table-schema observer.
    ///
    /// Calling this more than once with the same name is idempotent. The first
    /// observer wins so repeated router construction does not duplicate
    /// projection work for the same service instance.
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
