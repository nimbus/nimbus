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

#[derive(Default)]
struct InjectorState {
    faults: BTreeMap<(TenantId, PpscInjectedFault), ArmedFaultState>,
}

/// Observable state for one tenant-scoped PPSC storage fault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PpscStorageFaultSnapshot {
    pub active: bool,
    /// Every armed check that reached this fault's tenant and point, whether or
    /// not it fired. `visits` above `fires` is the deflection count: concurrent
    /// same-tenant boundaries that made no journal record durable — record-less
    /// commits and journal replays alike — and so were not allowed to consume
    /// the arm.
    pub visits: u64,
    /// Checks that actually failed, each of which was making journal records
    /// durable.
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
///
/// Both faults are additionally scoped to record identity, not just to the
/// tenant; see [`PpscStorageFaultInjector::check_tenant`].
#[derive(Default)]
pub struct PpscStorageFaultInjector {
    state: Mutex<InjectorState>,
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
        let entry = state.faults.entry((tenant_id, fault)).or_default();
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
        let entry = state
            .faults
            .get_mut(&(tenant_id.clone(), fault))
            .ok_or_else(|| {
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
            .faults
            .get(&(tenant_id.clone(), fault))
            .copied()
            .unwrap_or_default();
        Ok(PpscStorageFaultSnapshot {
            active: current.active,
            visits: current.visits,
            fires: current.fires,
        })
    }

    /// Fires an armed fault only for a transaction that is making durable
    /// journal records visible.
    ///
    /// Tenant identity alone does not identify a transaction. Every
    /// same-tenant commit reaches the same commit-sequence fault points at the
    /// same time — schedule-only execution units, trigger outcomes, and the
    /// fenced durable batch itself all arrive with no commit entry — so an arm
    /// keyed on tenant alone is consumed by whichever of them happens to
    /// commit first. The durable records are the discriminator, in two steps.
    ///
    /// A transaction carrying no records is not the durable batch the scenario
    /// armed, so it records a visit and passes.
    ///
    /// This rests on the product's side of the contract: `records` names what
    /// *this* boundary makes durable, not merely what passes through it.
    /// Journal recovery re-applies records that were already durable, through
    /// the very same commit-sequence boundary, and it therefore presents none
    /// — see the replay paths in `nimbus-storage`. Before that was true, those
    /// replays stole the arm. Nothing here can recover the distinction if a
    /// boundary reports records it did not make durable, so a new fault-checked
    /// boundary must decide which of the two it is.
    ///
    /// Retries keep firing: a batch retried after a transient failure is still
    /// making its records durable, so it still presents them. That is what
    /// `ProviderTransient` needs in order to keep failing until the scenario
    /// releases it.
    ///
    /// This is not the refuted `commit.is_some()` gate. That one moved the
    /// discrimination into the product's commit sequence, where it deleted the
    /// crash-and-replay coverage the fault exists to test — the fenced durable
    /// batch commits with no commit entry, so gating on one silenced the fault
    /// on its own target. Here the product still checks unconditionally; only
    /// this harness decides, from identity the product now carries, whether
    /// this is the transaction it armed.
    fn check_tenant(
        &self,
        point: FaultPoint,
        tenant_id: &TenantId,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::Internal("PPSC storage fault lock poisoned".to_string()))?;
        let Some((fault, current)) =
            state
                .faults
                .iter_mut()
                .find(|((candidate_tenant, fault), state)| {
                    candidate_tenant == tenant_id
                        && state.active
                        && storage_fault_point_unchecked(*fault) == Some(point)
                })
        else {
            return Ok(());
        };
        current.visits = current.visits.saturating_add(1);
        if records.is_empty() {
            return Ok(());
        }
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

    fn check_for_tenant(
        &self,
        point: FaultPoint,
        tenant_id: &TenantId,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        self.check_tenant(point, tenant_id, records)
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
        PpscInjectedFault::DurableBeforePublish | PpscInjectedFault::PanicAfterDurable => None,
    }
}
