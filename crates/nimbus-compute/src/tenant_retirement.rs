//! Compute-owned, exact-key convergence for one live tenant deletion.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use nimbus_core::{TenantId, WorkloadId};
use nimbus_services::{
    ServiceManager, TenantSourceRetirementSnapshot, TenantWorkloadSourceSnapshot,
};
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, WorkloadProvisionSourceKind, WorkloadSagaKey,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStoreError, WorkloadSagaTenantCursor,
    WorkloadSagaTenantPageRequest,
};
use thiserror::Error;

use crate::resource_retirement::{ComputeResourceRetirementError, ComputeResourceRetirer};
use crate::workload_saga::WorkloadSagaCoordinator;

const TENANT_SAGA_PAGE_SIZE: u16 = nimbus_workloads::MAX_WORKLOAD_SAGA_PAGE_SIZE;

#[derive(Debug, Error)]
pub(crate) enum TenantRetirementError {
    #[error("tenant workload inventory failed: {0}")]
    Inventory(#[from] WorkloadSagaStoreError),
    #[error("tenant workload inventory is invalid: {0}")]
    InvalidInventory(&'static str),
    #[error("tenant workload teardown failed: {0}")]
    Teardown(#[from] ComputeResourceRetirementError),
    #[error("tenant services finalization failed: {0}")]
    Services(#[from] nimbus_core::Error),
}

pub(crate) struct TenantRetirementDriver {
    coordinator: Arc<WorkloadSagaCoordinator>,
    services: Arc<ServiceManager>,
    resource_retirer: ComputeResourceRetirer,
    page_size: u16,
}

impl TenantRetirementDriver {
    pub(crate) fn new(
        coordinator: Arc<WorkloadSagaCoordinator>,
        services: Arc<ServiceManager>,
        resource_retirer: ComputeResourceRetirer,
    ) -> Self {
        Self {
            coordinator,
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

    pub(crate) async fn drive_tenant_teardown(
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
        let terminal = final_records.into_values().collect::<Vec<_>>();
        self.services
            .finalize_tenant_sources_after_recorded(snapshot.claim(), &terminal)?;
        self.resource_retirer
            .release_tenant_source_fences(&source_keys);
        Ok(terminal)
    }

    async fn list_tenant_sagas(
        &self,
        tenant_id: &TenantId,
    ) -> Result<BTreeMap<WorkloadSagaKey, WorkloadSagaRecord>, WorkloadSagaStoreError> {
        let mut records = BTreeMap::new();
        let mut cursor: Option<WorkloadSagaTenantCursor> = None;
        loop {
            let request = WorkloadSagaTenantPageRequest::new(cursor.clone(), self.page_size)?;
            let page = self.coordinator.list_for_tenant(tenant_id, request).await?;
            if page.tenant_id() != tenant_id {
                return Err(WorkloadSagaStoreError::Corrupt);
            }
            let next = page.next_cursor().cloned();
            let mut previous = cursor.as_ref().map(WorkloadSagaTenantCursor::key);
            for record in page.records() {
                if record.key().tenant_id() != tenant_id
                    || previous.is_some_and(|previous| record.key() <= previous)
                {
                    return Err(WorkloadSagaStoreError::Corrupt);
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
                return Err(WorkloadSagaStoreError::Corrupt);
            }
            for record in page.into_records() {
                if records.insert(record.key().clone(), record).is_some() {
                    return Err(WorkloadSagaStoreError::Corrupt);
                }
            }
            match next {
                Some(next) => cursor = Some(next),
                None => return Ok(records),
            }
        }
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
    snapshot: &TenantWorkloadSourceSnapshot,
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
