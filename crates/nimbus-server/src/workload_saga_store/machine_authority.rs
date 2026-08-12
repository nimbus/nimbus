//! Complete Engine authority scan for physical-machine admission fencing.
//!
//! The workload-saga schema indexes `recoveryEligible` and `sagaId`, while
//! execution-provider identity remains inside the strict portable intent.
//! Scan both Boolean partitions and filter only after decode. A stable tenant
//! sequence before and after the complete scan prevents a concurrent record
//! transition from appearing as false absence.

use std::collections::BTreeSet;
use std::sync::Arc;

use nimbus_compute::machine_stop_authority::{
    MachineWorkloadAuthorityStoreError, MachineWorkloadSagaAuthority,
};
use nimbus_core::{Filter, FilterOp, OrderBy, OrderDirection, PrincipalContext, Query};
use nimbus_engine::Engine;
use nimbus_workloads::{WorkloadExecutionProviderId, WorkloadSagaStoreError};
use serde_json::Value;

use super::codec::decode_workload_saga_record;
use super::schema::{workload_saga_table, workload_saga_tenant};

const SCAN_PAGE_SIZE: usize = 128;
const STABLE_SCAN_ATTEMPTS: usize = 3;

pub(super) fn map_store_error(error: WorkloadSagaStoreError) -> MachineWorkloadAuthorityStoreError {
    match error {
        WorkloadSagaStoreError::Ambiguous => MachineWorkloadAuthorityStoreError::Ambiguous,
        WorkloadSagaStoreError::Corrupt | WorkloadSagaStoreError::InvalidTransition(_) => {
            MachineWorkloadAuthorityStoreError::Corrupt
        }
        WorkloadSagaStoreError::Conflict { .. } => MachineWorkloadAuthorityStoreError::Ambiguous,
        WorkloadSagaStoreError::Unavailable => MachineWorkloadAuthorityStoreError::Unavailable,
    }
}

pub(super) async fn list_for_execution_provider(
    engine: &Arc<Engine>,
    execution_provider_id: &WorkloadExecutionProviderId,
) -> Result<Vec<MachineWorkloadSagaAuthority>, MachineWorkloadAuthorityStoreError> {
    let tenant = workload_saga_tenant().map_err(map_store_error)?;
    for _ in 0..STABLE_SCAN_ATTEMPTS {
        let before = engine
            .latest_sequence_async(tenant.clone())
            .await
            .map_err(|_| MachineWorkloadAuthorityStoreError::Unavailable)?;
        let mut authorities = Vec::new();
        let mut seen_sagas = BTreeSet::new();
        for recovery_eligible in [false, true] {
            scan_partition(
                engine,
                recovery_eligible,
                execution_provider_id,
                &mut seen_sagas,
                &mut authorities,
            )
            .await?;
        }
        let after = engine
            .latest_sequence_async(tenant.clone())
            .await
            .map_err(|_| MachineWorkloadAuthorityStoreError::Unavailable)?;
        if before == after {
            authorities.sort_by(|left, right| {
                (left.key(), left.generation()).cmp(&(right.key(), right.generation()))
            });
            if authorities
                .iter()
                .any(|authority| authority.execution_provider_id() != execution_provider_id)
                || authorities.windows(2).any(|pair| {
                    pair[0].key() == pair[1].key() && pair[0].generation() == pair[1].generation()
                })
            {
                return Err(MachineWorkloadAuthorityStoreError::Corrupt);
            }
            return Ok(authorities);
        }
    }
    Err(MachineWorkloadAuthorityStoreError::Ambiguous)
}

async fn scan_partition(
    engine: &Arc<Engine>,
    recovery_eligible: bool,
    execution_provider_id: &WorkloadExecutionProviderId,
    seen_sagas: &mut BTreeSet<nimbus_workloads::WorkloadSagaId>,
    authorities: &mut Vec<MachineWorkloadSagaAuthority>,
) -> Result<(), MachineWorkloadAuthorityStoreError> {
    let tenant = workload_saga_tenant().map_err(map_store_error)?;
    let table = workload_saga_table().map_err(map_store_error)?;
    let mut after = None::<String>;
    loop {
        let mut filters = vec![Filter {
            field: "recoveryEligible".to_owned(),
            op: FilterOp::Eq,
            value: Value::Bool(recovery_eligible),
        }];
        if let Some(after) = after.as_ref() {
            filters.push(Filter {
                field: "sagaId".to_owned(),
                op: FilterOp::Gt,
                value: Value::String(after.clone()),
            });
        }
        let documents = engine
            .query_documents_async_with_principal(
                tenant.clone(),
                Query {
                    table: table.clone(),
                    filters,
                    order: Some(OrderBy {
                        field: "sagaId".to_owned(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: Some(SCAN_PAGE_SIZE.saturating_add(1)),
                },
                PrincipalContext::system(),
            )
            .await
            .map_err(|_| MachineWorkloadAuthorityStoreError::Unavailable)?;
        if documents.len() > SCAN_PAGE_SIZE.saturating_add(1) {
            return Err(MachineWorkloadAuthorityStoreError::Corrupt);
        }
        let page_len = documents.len();
        let mut previous = after.clone();
        let mut records = Vec::with_capacity(page_len);
        for document in documents {
            let record = decode_workload_saga_record(&document).map_err(map_store_error)?;
            if record.requires_recovery() != recovery_eligible
                || previous
                    .as_ref()
                    .is_some_and(|previous| record.saga_id().as_str() <= previous.as_str())
            {
                return Err(MachineWorkloadAuthorityStoreError::Corrupt);
            }
            previous = Some(record.saga_id().as_str().to_owned());
            records.push(record);
        }
        for record in records.iter().take(SCAN_PAGE_SIZE) {
            if !seen_sagas.insert(record.saga_id().clone()) {
                return Err(MachineWorkloadAuthorityStoreError::Corrupt);
            }
            authorities.extend(
                MachineWorkloadSagaAuthority::from_record_for_provider(
                    record,
                    execution_provider_id,
                )
                .map_err(|_| MachineWorkloadAuthorityStoreError::Corrupt)?,
            );
        }
        if page_len <= SCAN_PAGE_SIZE {
            return Ok(());
        }
        after = records
            .get(SCAN_PAGE_SIZE.saturating_sub(1))
            .map(|record| record.saga_id().as_str().to_owned());
    }
}
