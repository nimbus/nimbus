use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nimbus_core::{DependencySet, PrincipalContext, Result, Schema, SequenceNumber, TableName};

use crate::persistence::TenantPersistenceSnapshot;
use crate::tenant::TenantRuntime;

use super::Engine;

mod batch;
mod commit;
mod pause;
mod reads;
mod staging;
mod state;

#[cfg(test)]
mod tests;

use self::state::MutationExecutionUnitState;
pub(crate) use pause::CommitFaultClient;
#[cfg(any(test, feature = "test-hooks"))]
pub use pause::{CommitFaultHandle, Fault, Label, labels};
#[cfg(not(any(test, feature = "test-hooks")))]
pub(crate) use pause::{Label, labels};

pub struct MutationExecutionUnit {
    engine: Arc<Engine>,
    runtime: Arc<TenantRuntime>,
    tenant_id: nimbus_core::TenantId,
    principal: PrincipalContext,
    schema_snapshot: Arc<Schema>,
    schema_epoch_snapshot: HashMap<TableName, SequenceNumber>,
    snapshot: TenantPersistenceSnapshot,
    snapshot_sequence: SequenceNumber,
    state: Mutex<MutationExecutionUnitState>,
}

impl Engine {
    pub fn begin_mutation_execution_unit(
        self: &Arc<Self>,
        tenant_id: nimbus_core::TenantId,
        principal: PrincipalContext,
    ) -> Result<Arc<MutationExecutionUnit>> {
        let runtime = self.get_existing_tenant(&tenant_id)?;
        let snapshot = runtime.store().read_snapshot()?;
        let snapshot_sequence = snapshot.applied_sequence()?;
        let schema_snapshot = runtime.schema();
        let schema_epoch_snapshot = runtime.published_schema_epoch_snapshot();
        Ok(Arc::new(MutationExecutionUnit {
            engine: self.clone(),
            runtime,
            tenant_id,
            principal,
            schema_snapshot,
            schema_epoch_snapshot,
            snapshot,
            snapshot_sequence,
            state: Mutex::new(MutationExecutionUnitState::default()),
        }))
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub fn commit_fault_handle_for_testing(&self) -> CommitFaultHandle {
        self.commit_faults.handle()
    }
}

impl MutationExecutionUnit {
    pub fn snapshot_sequence(&self) -> SequenceNumber {
        self.snapshot_sequence
    }

    pub fn read_dependencies(&self) -> DependencySet {
        self.state
            .lock()
            .expect("mutation execution unit lock should not be poisoned")
            .read_dependencies
            .clone()
    }

    pub fn write_dependencies(&self) -> DependencySet {
        self.state
            .lock()
            .expect("mutation execution unit lock should not be poisoned")
            .write_dependencies
            .clone()
    }
}
