use std::sync::Arc;

use nimbus_core::{Filter, FilterOp, OrderBy, OrderDirection, PrincipalContext, Query};
use nimbus_engine::Engine;
use nimbus_workloads::{WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaStoreError};
use serde_json::Value;

use super::codec::decode_workload_saga_record;
use super::schema::{workload_saga_table, workload_saga_tenant};

pub(super) async fn list_recoverable(
    engine: &Arc<Engine>,
    request: WorkloadSagaPageRequest,
) -> Result<WorkloadSagaPage, WorkloadSagaStoreError> {
    let tenant = workload_saga_tenant()?;
    let table = workload_saga_table()?;
    let mut filters = vec![Filter {
        field: "recoveryEligible".to_owned(),
        op: FilterOp::Eq,
        value: Value::Bool(true),
    }];
    if let Some(cursor) = request.after() {
        filters.push(Filter {
            field: "sagaId".to_owned(),
            op: FilterOp::Gt,
            value: Value::String(cursor.saga_id().as_str().to_owned()),
        });
    }

    let documents = engine
        .query_documents_async_with_principal(
            tenant,
            Query {
                table,
                filters,
                order: Some(OrderBy {
                    field: "sagaId".to_owned(),
                    direction: OrderDirection::Asc,
                }),
                limit: Some(usize::from(request.limit()).saturating_add(1)),
            },
            PrincipalContext::system(),
        )
        .await
        .map_err(|_| WorkloadSagaStoreError::Unavailable)?;
    let has_more = documents.len() > usize::from(request.limit());
    let mut records = Vec::with_capacity(usize::from(request.limit()));
    for document in documents.into_iter().take(usize::from(request.limit())) {
        let record = decode_workload_saga_record(&document)?;
        if !record.requires_recovery() {
            return Err(WorkloadSagaStoreError::Corrupt);
        }
        records.push(record);
    }

    WorkloadSagaPage::new(&request, records, has_more)
}
