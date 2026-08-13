//! Compute-owned, exact-key convergence for one live tenant deletion.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;
use std::sync::Arc;

use nimbus_core::{TenantId, WorkloadId};
use nimbus_engine::Engine;
use nimbus_services::{ServiceManager, TenantSourceRetirementSnapshot};
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, TenantRetirementCommit, TenantRetirementExpected,
    TenantRetirementPageRequest, TenantRetirementPhase, TenantRetirementRecord,
    TenantRetirementSource, TenantRetirementStore, TenantRetirementStoreError,
    WorkloadProvisionSourceKind, WorkloadSagaKey, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaStoreError, WorkloadSagaTenantCursor, WorkloadSagaTenantPageRequest,
};
use thiserror::Error;

use crate::resource_retirement::{ComputeResourceRetirementError, ComputeResourceRetirer};
use crate::runtime_manager::RuntimeManager;
use crate::workload_saga::WorkloadSagaCoordinator;

const TENANT_SAGA_PAGE_SIZE: u16 = nimbus_workloads::MAX_WORKLOAD_SAGA_PAGE_SIZE;
const MAX_TENANT_RETIREMENT_PAGES: usize = 64;

#[derive(Debug, Error)]
pub(crate) enum TenantRetirementError {
    #[error("tenant workload inventory failed: {0}")]
    Inventory(#[from] WorkloadSagaStoreError),
    #[error("tenant workload inventory fence failed: {0}")]
    InventoryFence(#[from] TenantRetirementStoreError),
    #[error("tenant workload inventory is invalid: {0}")]
    InvalidInventory(&'static str),
    #[error("tenant workload teardown failed: {0}")]
    Teardown(#[from] ComputeResourceRetirementError),
    #[error("tenant retirement lifecycle failed: {0}")]
    Lifecycle(#[from] nimbus_core::Error),
    #[error("tenant retirement durable truth is crossed for {0}")]
    Crossed(TenantId),
    #[error(
        "tenant {tenant_id} disappeared before retirement phase {phase:?} could delete its Engine incarnation {tenant_incarnation}"
    )]
    MissingEngineTenant {
        tenant_id: TenantId,
        tenant_incarnation: NonZeroU64,
        phase: TenantRetirementPhase,
    },
    #[error("tenant retirement portable state is invalid: {0}")]
    Portable(#[from] nimbus_workloads::TenantRetirementError),
    #[error("tenant retirement exceeded {MAX_TENANT_RETIREMENT_PAGES} retained-record pages")]
    PageLimit,
}

/// One compute-owned durable tenant-retirement coordinator.
///
/// It composes the Engine deletion fence, runtime-owner retirement, durable
/// progress record, services barrier, and exact workload teardown owner. It
/// performs no provider effect directly.
pub(crate) struct TenantRetirementRuntime {
    engine: Arc<Engine>,
    runtime_manager: Arc<RuntimeManager>,
    driver: TenantRetirementDriver,
}

impl TenantRetirementRuntime {
    pub(crate) fn new(
        engine: Arc<Engine>,
        runtime_manager: Arc<RuntimeManager>,
        driver: TenantRetirementDriver,
    ) -> Self {
        Self {
            engine,
            runtime_manager,
            driver,
        }
    }

    /// Capture and durably persist exact source truth before the Engine
    /// deletion fence or any workload/provider effect can begin.
    pub(crate) async fn retire(&self, tenant_id: TenantId) -> Result<(), TenantRetirementError> {
        if let Some(retained) = self.driver.load_retained(&tenant_id).await? {
            let snapshot = self
                .driver
                .services
                .restore_tenant_source_retirement(&retained)?;
            return self.resume(retained, snapshot).await;
        }
        let identity = self
            .engine
            .enter_tenant_runtime_async(tenant_id.clone())
            .await?;
        let snapshot = self
            .driver
            .services
            .claim_tenant_source_retirement(&tenant_id, identity.tenant_incarnation())?;
        let record = self.driver.persist_intent(&snapshot).await?;
        drop(identity);
        self.resume(record, snapshot).await
    }

    /// Restore every durable source barrier before resuming any retirement.
    /// This ordering keeps workload admission closed for the complete retained
    /// set even when an earlier record needs provider-backed teardown work.
    pub(crate) async fn recover_retained(&self) -> Result<usize, TenantRetirementError> {
        let records = self.driver.list_retained_retirements().await?;
        let mut retirements = Vec::with_capacity(records.len());
        for record in records {
            let snapshot = self
                .driver
                .services
                .restore_tenant_source_retirement(&record)?;
            retirements.push((record, snapshot));
        }
        let count = retirements.len();
        for (record, snapshot) in retirements {
            self.resume(record, snapshot).await?;
        }
        Ok(count)
    }

    async fn resume(
        &self,
        retained: TenantRetirementRecord,
        snapshot: TenantSourceRetirementSnapshot,
    ) -> Result<(), TenantRetirementError> {
        let mut current = self.driver.adopt_exact_or_later(&retained).await?;
        let mut deletion = None;
        let mut retired_owner = None;

        if matches!(
            current.phase(),
            TenantRetirementPhase::IntentCommitted
                | TenantRetirementPhase::ChildrenRecorded
                | TenantRetirementPhase::SourcesFinalized
        ) {
            match self
                .engine
                .begin_tenant_incarnation_delete_async(
                    current.tenant_id().clone(),
                    current.tenant_incarnation(),
                )
                .await
            {
                Ok(exact_deletion) => {
                    let (owner_id, _) = self
                        .runtime_manager
                        .retire_tenant_deletion(&exact_deletion)
                        .await?;
                    retired_owner = Some(owner_id);
                    deletion = Some(exact_deletion);
                }
                Err(nimbus_core::Error::TenantNotFound(_))
                    if current.phase() == TenantRetirementPhase::SourcesFinalized => {}
                Err(nimbus_core::Error::TenantNotFound(_)) => {
                    return Err(TenantRetirementError::MissingEngineTenant {
                        tenant_id: current.tenant_id().clone(),
                        tenant_incarnation: current.tenant_incarnation(),
                        phase: current.phase(),
                    });
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            self.require_engine_tenant_absent(&current).await?;
        }

        let terminal = if current.phase() == TenantRetirementPhase::IntentCommitted {
            let terminal = self.driver.drive_children_to_recorded(&snapshot).await?;
            current = self
                .driver
                .advance_progress(&current, TenantRetirementPhase::ChildrenRecorded)
                .await?;
            terminal
        } else {
            self.driver.load_recorded_children(&snapshot).await?
        };

        if current.phase() == TenantRetirementPhase::ChildrenRecorded {
            self.driver
                .finalize_recorded_sources(&snapshot, &terminal)?;
            current = self
                .driver
                .advance_progress(&current, TenantRetirementPhase::SourcesFinalized)
                .await?;
        } else {
            self.driver
                .finalize_recorded_sources(&snapshot, &terminal)?;
        }

        if current.phase() == TenantRetirementPhase::SourcesFinalized {
            if let Some(exact_deletion) = deletion.take() {
                self.engine
                    .finish_tenant_delete_async(exact_deletion)
                    .await?;
            }
            current = self
                .driver
                .advance_progress(&current, TenantRetirementPhase::EngineDeleted)
                .await?;
        }

        if let Some(owner_id) = retired_owner.as_ref() {
            self.runtime_manager.forget_retired_owner(owner_id);
        }

        if current.phase() == TenantRetirementPhase::EngineDeleted {
            current = self
                .driver
                .advance_progress(&current, TenantRetirementPhase::Recorded)
                .await?;
        }

        self.driver.release_source_fences(&snapshot)?;
        self.driver
            .services
            .release_tenant_source_retirement(snapshot.claim())?;
        self.driver.delete_terminal(&current).await
    }

    async fn require_engine_tenant_absent(
        &self,
        record: &TenantRetirementRecord,
    ) -> Result<(), TenantRetirementError> {
        match self
            .engine
            .enter_tenant_runtime_async(record.tenant_id().clone())
            .await
        {
            Err(nimbus_core::Error::TenantNotFound(_)) => Ok(()),
            Ok(live) => {
                let tenant_id = live.tenant_id().clone();
                drop(live);
                Err(TenantRetirementError::Crossed(tenant_id))
            }
            Err(error) => Err(error.into()),
        }
    }
}

pub(crate) struct TenantRetirementDriver {
    coordinator: Arc<WorkloadSagaCoordinator>,
    retirement_store: Arc<dyn TenantRetirementStore>,
    services: Arc<ServiceManager>,
    resource_retirer: ComputeResourceRetirer,
    page_size: u16,
}

impl TenantRetirementDriver {
    pub(crate) fn new(
        coordinator: Arc<WorkloadSagaCoordinator>,
        retirement_store: Arc<dyn TenantRetirementStore>,
        services: Arc<ServiceManager>,
        resource_retirer: ComputeResourceRetirer,
    ) -> Self {
        Self {
            coordinator,
            retirement_store,
            services,
            resource_retirer,
            page_size: TENANT_SAGA_PAGE_SIZE,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_page_size_for_test(mut self, page_size: u16) -> Self {
        WorkloadSagaTenantPageRequest::new(None, page_size)
            .expect("tenant-retirement test page size must be valid");
        self.page_size = page_size;
        self
    }

    #[cfg(test)]
    pub(crate) async fn drive_tenant_teardown(
        &self,
        snapshot: &TenantSourceRetirementSnapshot,
    ) -> Result<Vec<WorkloadSagaRecord>, TenantRetirementError> {
        let terminal = self.drive_children_to_recorded(snapshot).await?;
        self.finalize_recorded_sources(snapshot, &terminal)?;
        self.release_source_fences(snapshot)?;
        Ok(terminal)
    }

    pub(crate) async fn list_retained_retirements(
        &self,
    ) -> Result<Vec<TenantRetirementRecord>, TenantRetirementError> {
        let mut cursor = None;
        let mut pages = 0;
        let mut records = Vec::new();
        loop {
            if pages == MAX_TENANT_RETIREMENT_PAGES {
                return Err(TenantRetirementError::PageLimit);
            }
            let request = TenantRetirementPageRequest::new(
                cursor,
                nimbus_workloads::MAX_TENANT_RETIREMENT_PAGE_SIZE,
            )?;
            let page = self.retirement_store.list_retirements(request).await?;
            pages += 1;
            records.extend_from_slice(page.records());
            let Some(next) = page.next_cursor().cloned() else {
                break;
            };
            cursor = Some(next);
        }
        Ok(records)
    }

    pub(crate) async fn load_retained(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<TenantRetirementRecord>, TenantRetirementError> {
        self.retirement_store
            .load_retirement(tenant_id)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn persist_intent(
        &self,
        snapshot: &TenantSourceRetirementSnapshot,
    ) -> Result<TenantRetirementRecord, TenantRetirementError> {
        let intended = TenantRetirementRecord::new(
            snapshot.claim().tenant_id().clone(),
            snapshot.claim().tenant_incarnation(),
            snapshot.sources().to_vec(),
        )?;
        match self
            .retirement_store
            .compare_and_swap_retirement(TenantRetirementExpected::Missing, intended.clone())
            .await
        {
            Ok(TenantRetirementCommit::Applied | TenantRetirementCommit::Unchanged) => Ok(intended),
            Err(
                TenantRetirementStoreError::Conflict { .. } | TenantRetirementStoreError::Ambiguous,
            ) => self.adopt_exact_or_later(&intended).await,
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) async fn advance_progress(
        &self,
        current: &TenantRetirementRecord,
        target: TenantRetirementPhase,
    ) -> Result<TenantRetirementRecord, TenantRetirementError> {
        if current.revision().as_u64() >= target_revision(target) {
            return Ok(current.clone());
        }
        let next = current.advance(target)?;
        match self
            .retirement_store
            .compare_and_swap_retirement(
                TenantRetirementExpected::Revision(current.revision()),
                next.clone(),
            )
            .await
        {
            Ok(TenantRetirementCommit::Applied | TenantRetirementCommit::Unchanged) => Ok(next),
            Err(
                TenantRetirementStoreError::Conflict { .. } | TenantRetirementStoreError::Ambiguous,
            ) => self.adopt_exact_or_later(&next).await,
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) async fn delete_terminal(
        &self,
        terminal: &TenantRetirementRecord,
    ) -> Result<(), TenantRetirementError> {
        for _ in 0..2 {
            match self
                .retirement_store
                .delete_retirement(terminal.clone())
                .await
            {
                Ok(TenantRetirementCommit::Applied | TenantRetirementCommit::Unchanged) => {
                    return Ok(());
                }
                Err(TenantRetirementStoreError::Conflict { .. }) => {
                    return Err(TenantRetirementError::Crossed(terminal.tenant_id().clone()));
                }
                Err(TenantRetirementStoreError::Ambiguous) => {
                    match self
                        .retirement_store
                        .load_retirement(terminal.tenant_id())
                        .await?
                    {
                        None => return Ok(()),
                        Some(current) if current == *terminal => continue,
                        Some(_) => {
                            return Err(TenantRetirementError::Crossed(
                                terminal.tenant_id().clone(),
                            ));
                        }
                    }
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(TenantRetirementStoreError::Ambiguous.into())
    }

    async fn adopt_exact_or_later(
        &self,
        expected: &TenantRetirementRecord,
    ) -> Result<TenantRetirementRecord, TenantRetirementError> {
        let current = self
            .retirement_store
            .load_retirement(expected.tenant_id())
            .await?
            .ok_or_else(|| TenantRetirementError::Crossed(expected.tenant_id().clone()))?;
        if current.retirement_id() == expected.retirement_id()
            && current.tenant_incarnation() == expected.tenant_incarnation()
            && current.sources() == expected.sources()
            && current.revision().as_u64() >= expected.revision().as_u64()
        {
            return Ok(current);
        }
        Err(TenantRetirementError::Crossed(expected.tenant_id().clone()))
    }

    pub(crate) async fn drive_children_to_recorded(
        &self,
        snapshot: &TenantSourceRetirementSnapshot,
    ) -> Result<Vec<WorkloadSagaRecord>, TenantRetirementError> {
        let source_keys = snapshot_source_keys(snapshot)?;
        self.resource_retirer
            .fence_tenant_sources_and_join(&source_keys)
            .await?;
        let initial = self.list_tenant_sagas(snapshot.claim().tenant_id()).await?;
        authenticate_snapshot_inventory(snapshot, &initial)?;

        for record in initial.values() {
            self.resource_retirer
                .submit_tenant_record_teardown(record.clone())
                .await?;
        }

        let final_records = self.list_tenant_sagas(snapshot.claim().tenant_id()).await?;
        require_all_recorded_before_finish_tenant_delete(&initial, &final_records)?;
        authenticate_snapshot_inventory(snapshot, &final_records)?;
        Ok(final_records.into_values().collect())
    }

    pub(crate) async fn load_recorded_children(
        &self,
        snapshot: &TenantSourceRetirementSnapshot,
    ) -> Result<Vec<WorkloadSagaRecord>, TenantRetirementError> {
        let source_keys = snapshot_source_keys(snapshot)?;
        self.resource_retirer
            .fence_tenant_sources_and_join(&source_keys)
            .await?;
        let records = self.list_tenant_sagas(snapshot.claim().tenant_id()).await?;
        authenticate_snapshot_inventory(snapshot, &records)?;
        if records.values().any(|record| {
            record.phase() != WorkloadSagaPhase::Recorded
                || record.active_intent().desired_state() != DesiredWorkloadState::Stopped
                || record.successor_intent().is_some()
        }) {
            return Err(TenantRetirementError::InvalidInventory(
                "durable tenant-retirement progress requires every child to remain Recorded and stopped",
            ));
        }
        Ok(records.into_values().collect())
    }

    pub(crate) fn finalize_recorded_sources(
        &self,
        snapshot: &TenantSourceRetirementSnapshot,
        terminal: &[WorkloadSagaRecord],
    ) -> Result<(), TenantRetirementError> {
        self.services
            .finalize_tenant_sources_after_recorded(snapshot.claim(), terminal)?;
        Ok(())
    }

    pub(crate) fn release_source_fences(
        &self,
        snapshot: &TenantSourceRetirementSnapshot,
    ) -> Result<(), TenantRetirementError> {
        let source_keys = snapshot_source_keys(snapshot)?;
        self.resource_retirer
            .release_tenant_source_fences(&source_keys);
        Ok(())
    }

    async fn list_tenant_sagas(
        &self,
        tenant_id: &TenantId,
    ) -> Result<BTreeMap<WorkloadSagaKey, WorkloadSagaRecord>, TenantRetirementError> {
        let epoch_before = self
            .retirement_store
            .load_workload_mutation_epoch(tenant_id)
            .await?;
        let mut records = BTreeMap::new();
        let mut cursor: Option<WorkloadSagaTenantCursor> = None;
        loop {
            let request = WorkloadSagaTenantPageRequest::new(cursor.clone(), self.page_size)?;
            let page = self.coordinator.list_for_tenant(tenant_id, request).await?;
            if page.tenant_id() != tenant_id {
                return Err(WorkloadSagaStoreError::Corrupt.into());
            }
            let next = page.next_cursor().cloned();
            let mut previous = cursor.as_ref().map(WorkloadSagaTenantCursor::key);
            for record in page.records() {
                if record.key().tenant_id() != tenant_id
                    || previous.is_some_and(|previous| record.key() <= previous)
                {
                    return Err(WorkloadSagaStoreError::Corrupt.into());
                }
                previous = Some(record.key());
            }
            if let Some(next) = next.as_ref()
                && (next.tenant_id() != tenant_id
                    || page.records().last().map(WorkloadSagaRecord::key) != Some(next.key())
                    || cursor
                        .as_ref()
                        .is_some_and(|cursor| next.key() <= cursor.key()))
            {
                return Err(WorkloadSagaStoreError::Corrupt.into());
            }
            for record in page.into_records() {
                if records.insert(record.key().clone(), record).is_some() {
                    return Err(WorkloadSagaStoreError::Corrupt.into());
                }
            }
            match next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        let epoch_after = self
            .retirement_store
            .load_workload_mutation_epoch(tenant_id)
            .await?;
        if epoch_before != epoch_after {
            return Err(TenantRetirementError::InvalidInventory(
                "durable workload inventory changed during paged enumeration",
            ));
        }
        Ok(records)
    }
}

const fn target_revision(phase: TenantRetirementPhase) -> u64 {
    match phase {
        TenantRetirementPhase::IntentCommitted => 0,
        TenantRetirementPhase::ChildrenRecorded => 1,
        TenantRetirementPhase::SourcesFinalized => 2,
        TenantRetirementPhase::EngineDeleted => 3,
        TenantRetirementPhase::Recorded => 4,
    }
}

fn snapshot_source_keys(
    snapshot: &TenantSourceRetirementSnapshot,
) -> Result<Vec<WorkloadSagaKey>, TenantRetirementError> {
    snapshot
        .sources()
        .iter()
        .map(|source| {
            WorkloadId::new(source.identity().stable_name())
                .map(|workload_id| {
                    WorkloadSagaKey::new(snapshot.claim().tenant_id().clone(), workload_id)
                })
                .map_err(|_| {
                    TenantRetirementError::InvalidInventory(
                        "frozen source snapshot contains an invalid workload identity",
                    )
                })
        })
        .collect()
}

fn authenticate_snapshot_inventory(
    snapshot: &TenantSourceRetirementSnapshot,
    records: &BTreeMap<WorkloadSagaKey, WorkloadSagaRecord>,
) -> Result<(), TenantRetirementError> {
    let sources = snapshot
        .sources()
        .iter()
        .map(|source| (source.identity().stable_name(), source))
        .collect::<BTreeMap<_, _>>();
    let mut covered = BTreeSet::new();
    for record in records.values() {
        let stable_name = record.key().workload_id().as_str();
        let source = sources
            .get(stable_name)
            .ok_or(TenantRetirementError::InvalidInventory(
                "durable record does not belong to the frozen source snapshot",
            ))?;
        authenticate_record_source(source, record)?;
        if !covered.insert(stable_name) {
            return Err(TenantRetirementError::InvalidInventory(
                "durable inventory contains duplicate workload identity",
            ));
        }
    }
    if snapshot.sources().iter().any(|source| {
        source.has_observation() && !covered.contains(source.identity().stable_name())
    }) {
        return Err(TenantRetirementError::InvalidInventory(
            "observed source has no durable workload saga",
        ));
    }
    Ok(())
}

fn authenticate_record_source(
    snapshot: &TenantRetirementSource,
    record: &WorkloadSagaRecord,
) -> Result<(), TenantRetirementError> {
    let intent = record.successor_intent().unwrap_or(record.active_intent());
    let source = intent.source();
    let expected_kind = match snapshot.identity().kind() {
        WorkloadProvisionSourceKind::StandaloneSandbox => DesiredWorkloadKind::Sandbox,
        WorkloadProvisionSourceKind::SandboxBackedService => DesiredWorkloadKind::Service,
    };
    if intent.kind() != expected_kind
        || source.source_identity() != snapshot.identity()
        || source.source_generation() != snapshot.source_generation()
        || source.resource_version() != snapshot.resource_version()
    {
        return Err(TenantRetirementError::InvalidInventory(
            "durable workload source is crossed with the frozen source snapshot",
        ));
    }
    Ok(())
}

fn require_all_recorded_before_finish_tenant_delete(
    initial: &BTreeMap<WorkloadSagaKey, WorkloadSagaRecord>,
    final_records: &BTreeMap<WorkloadSagaKey, WorkloadSagaRecord>,
) -> Result<(), TenantRetirementError> {
    if initial.keys().ne(final_records.keys()) {
        return Err(TenantRetirementError::InvalidInventory(
            "durable workload key set changed during tenant retirement",
        ));
    }
    if final_records.values().any(|record| {
        record.phase() != WorkloadSagaPhase::Recorded
            || record.active_intent().desired_state() != DesiredWorkloadState::Stopped
            || record.successor_intent().is_some()
    }) {
        return Err(TenantRetirementError::InvalidInventory(
            "tenant deletion requires every durable workload to be Recorded and stopped",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "tenant_retirement/tests.rs"]
mod tests;
