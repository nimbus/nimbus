use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nimbus_core::{Error, Result, StorageErrorKind, TenantEventRecord, TenantId};
use nimbus_storage::{FaultInjector, FaultPoint};

use super::PpscInjectedFault;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ArmedFaultState {
    active: bool,
    visits: u64,
    fires: u64,
}

/// Observable state for one tenant-scoped PPSC storage fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PpscStorageFaultSnapshot {
    pub active: bool,
    pub visits: u64,
    pub fires: u64,
}

/// Tenant-scoped storage-fault control for the production-interface PPSC runner.
///
/// Only faults at the persistence commit boundary belong here. Publisher and
/// commit-orchestration faults use Engine-owned seams instead. Acknowledgement
/// loss fires once after durable visibility; provider transient failures keep
/// firing at the journal transaction's pre-visibility boundary until the
/// scenario explicitly releases them. The journal-specific boundary is
/// intentional: an embedded publisher appends the journal and materializes it
/// in separate transactions, while a provider publisher performs both in one
/// fenced transaction.
#[derive(Default)]
pub struct PpscStorageFaultInjector {
    state: Mutex<BTreeMap<(TenantId, PpscInjectedFault), ArmedFaultState>>,
}

impl PpscStorageFaultInjector {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn arm(&self, tenant_id: TenantId, fault: PpscInjectedFault) -> Result<()> {
        storage_fault_point(fault)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Internal("PPSC storage fault lock poisoned".to_string()))?;
        let entry = state.entry((tenant_id, fault)).or_default();
        if entry.active {
            return Err(Error::InvalidInput(format!(
                "PPSC storage fault '{}' is already armed",
                fault.as_str()
            )));
        }
        entry.active = true;
        Ok(())
    }

    pub fn release(&self, tenant_id: &TenantId, fault: PpscInjectedFault) -> Result<()> {
        storage_fault_point(fault)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Internal("PPSC storage fault lock poisoned".to_string()))?;
        let entry = state.get_mut(&(tenant_id.clone(), fault)).ok_or_else(|| {
            Error::InvalidInput(format!(
                "PPSC storage fault '{}' was never armed for tenant '{tenant_id}'",
                fault.as_str()
            ))
        })?;
        entry.active = false;
        Ok(())
    }

    pub fn snapshot(
        &self,
        tenant_id: &TenantId,
        fault: PpscInjectedFault,
    ) -> Result<PpscStorageFaultSnapshot> {
        storage_fault_point(fault)?;
        let state = self
            .state
            .lock()
            .map_err(|_| Error::Internal("PPSC storage fault lock poisoned".to_string()))?;
        let current = state
            .get(&(tenant_id.clone(), fault))
            .copied()
            .unwrap_or_default();
        Ok(PpscStorageFaultSnapshot {
            active: current.active,
            visits: current.visits,
            fires: current.fires,
        })
    }

    fn check_tenant(&self, point: FaultPoint, tenant_id: &TenantId) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Internal("PPSC storage fault lock poisoned".to_string()))?;
        let Some((fault, current)) = state.iter_mut().find(|((candidate_tenant, fault), state)| {
            candidate_tenant == tenant_id
                && state.active
                && storage_fault_point_unchecked(*fault) == Some(point)
        }) else {
            return Ok(());
        };
        current.visits = current.visits.saturating_add(1);
        current.fires = current.fires.saturating_add(1);
        let fault = fault.1;
        if fault == PpscInjectedFault::AcknowledgementLoss {
            current.active = false;
        }
        Err(Error::storage(
            StorageErrorKind::Transient,
            format!(
                "injected PPSC {} for tenant '{}' on fire {}",
                fault.as_str(),
                tenant_id,
                current.fires
            ),
        ))
    }
}

impl FaultInjector for PpscStorageFaultInjector {
    fn check(&self, _point: FaultPoint) -> Result<()> {
        // The PPSC runner always supplies this injector through a tenant-owned
        // store/provider boundary. An unscoped check cannot safely choose a
        // tenant and therefore cannot consume an arm.
        Ok(())
    }

    fn check_for_tenant(&self, point: FaultPoint, tenant_id: &TenantId) -> Result<()> {
        self.check_tenant(point, tenant_id)
    }

    fn check_for_durable_records(
        &self,
        point: FaultPoint,
        tenant_id: &TenantId,
        _records: &[TenantEventRecord],
    ) -> Result<()> {
        self.check_tenant(point, tenant_id)
    }
}

fn storage_fault_point(fault: PpscInjectedFault) -> Result<FaultPoint> {
    storage_fault_point_unchecked(fault).ok_or_else(|| {
        Error::InvalidInput(format!(
            "PPSC fault '{}' is not owned by the storage fault interface",
            fault.as_str()
        ))
    })
}

const fn storage_fault_point_unchecked(fault: PpscInjectedFault) -> Option<FaultPoint> {
    match fault {
        PpscInjectedFault::AcknowledgementLoss => {
            Some(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)
        }
        PpscInjectedFault::ProviderTransient => Some(FaultPoint::JournalAppendBeforeDurableFlush),
        PpscInjectedFault::DurableBeforePublish
        | PpscInjectedFault::PublicationPredecessorHeld
        | PpscInjectedFault::PanicAfterDurable => None,
    }
}
