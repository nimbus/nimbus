use std::sync::{Arc, Mutex};

use neovex_core::{DependencySet, PrincipalContext, Result, Schema, SequenceNumber};

use crate::persistence::TenantPersistenceSnapshot;
use crate::tenant::TenantRuntime;

use super::Service;

mod batch;
mod commit;
mod reads;
mod staging;
mod state;

#[cfg(test)]
mod tests;

use self::state::MutationExecutionUnitState;

pub struct MutationExecutionUnit {
    service: Arc<Service>,
    runtime: Arc<TenantRuntime>,
    tenant_id: neovex_core::TenantId,
    principal: PrincipalContext,
    schema_snapshot: Arc<Schema>,
    snapshot: TenantPersistenceSnapshot,
    snapshot_sequence: SequenceNumber,
    state: Mutex<MutationExecutionUnitState>,
}

impl Service {
    pub fn begin_mutation_execution_unit(
        self: &Arc<Self>,
        tenant_id: neovex_core::TenantId,
        principal: PrincipalContext,
    ) -> Result<Arc<MutationExecutionUnit>> {
        let runtime = self.get_existing_tenant(&tenant_id)?;
        let snapshot = runtime.store().read_snapshot()?;
        let snapshot_sequence = snapshot.applied_sequence()?;
        let schema_snapshot = runtime.schema();
        Ok(Arc::new(MutationExecutionUnit {
            service: self.clone(),
            runtime,
            tenant_id,
            principal,
            schema_snapshot,
            snapshot,
            snapshot_sequence,
            state: Mutex::new(MutationExecutionUnitState::default()),
        }))
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
