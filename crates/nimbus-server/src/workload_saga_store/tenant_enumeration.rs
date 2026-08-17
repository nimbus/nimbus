use std::sync::Arc;

use nimbus_core::{
    Document, Filter, FilterOp, OrderBy, OrderDirection, PrincipalContext, Query, TenantId,
};
use nimbus_engine::Engine;
use nimbus_workloads::{
    WorkloadSagaStoreError, WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest,
};
use serde_json::Value;

use super::codec::decode_workload_saga_record;
use super::schema::{workload_saga_table, workload_saga_tenant};

pub(super) async fn list_for_tenant(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    request: WorkloadSagaTenantPageRequest,
) -> Result<WorkloadSagaTenantPage, WorkloadSagaStoreError> {
    request.validate_for_tenant(tenant_id)?;
    let storage_tenant = workload_saga_tenant()?;
    let table = workload_saga_table()?;
    let mut filters = vec![Filter {
        field: "tenantId".to_owned(),
        op: FilterOp::Eq,
        value: Value::String(tenant_id.as_str().to_owned()),
    }];
    if let Some(cursor) = request.after() {
        filters.push(Filter {
            field: "workloadId".to_owned(),
            op: FilterOp::Gt,
            value: Value::String(cursor.key().workload_id().as_str().to_owned()),
        });
    }

    let documents = engine
        .query_documents_async_with_principal(
            storage_tenant,
            Query {
                table,
                filters,
                order: Some(OrderBy {
                    field: "workloadId".to_owned(),
                    direction: OrderDirection::Asc,
                }),
                limit: Some(usize::from(request.limit()).saturating_add(1)),
            },
            PrincipalContext::system(),
        )
        .await
        .map_err(|_| WorkloadSagaStoreError::Unavailable)?;

    decode_tenant_page(tenant_id, &request, documents)
}

pub(super) fn decode_tenant_page(
    tenant_id: &TenantId,
    request: &WorkloadSagaTenantPageRequest,
    documents: Vec<Document>,
) -> Result<WorkloadSagaTenantPage, WorkloadSagaStoreError> {
    if documents.len() > usize::from(request.limit()).saturating_add(1) {
        return Err(WorkloadSagaStoreError::Corrupt);
    }
    let mut previous = request.after().map(|cursor| cursor.key().clone());
    let mut decoded = Vec::with_capacity(documents.len());
    for document in documents {
        let record = decode_workload_saga_record(&document)?;
        if record.key().tenant_id() != tenant_id
            || previous
                .as_ref()
                .is_some_and(|previous| record.key() <= previous)
        {
            return Err(WorkloadSagaStoreError::Corrupt);
        }
        previous = Some(record.key().clone());
        decoded.push(record);
    }

    let has_more = decoded.len() > usize::from(request.limit());
    decoded.truncate(usize::from(request.limit()));

    WorkloadSagaTenantPage::new(tenant_id, request, decoded, has_more)
        .map_err(|_| WorkloadSagaStoreError::Corrupt)
}
