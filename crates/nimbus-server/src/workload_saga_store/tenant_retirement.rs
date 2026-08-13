use std::collections::BTreeSet;
use std::sync::Arc;

use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, Document, DocumentId, DocumentLocator, Error, Filter, FilterOp,
    OrderBy, OrderDirection, PrincipalContext, Query, TenantId, WriteKey, WritePrecondition,
    WriteSetMode,
};
use nimbus_engine::Engine;
use nimbus_workloads::{
    TenantRetirementCommit, TenantRetirementExpected, TenantRetirementPage,
    TenantRetirementPageRequest, TenantRetirementRecord, TenantRetirementRevision,
    TenantRetirementStoreError, TenantWorkloadMutationEpoch,
};
use serde_json::{Map, Value, json};

use super::schema::{
    tenant_retirement_table, workload_saga_tenant, workload_saga_tenant_epoch_table,
};

const TENANT_RETIREMENT_FIELDS: [&str; 8] = [
    "formatVersion",
    "retirementId",
    "tenantId",
    "tenantIncarnation",
    "revision",
    "phase",
    "active",
    "sources",
];
const TENANT_EPOCH_FIELDS: [&str; 3] = ["formatVersion", "tenantId", "mutationEpoch"];
const TENANT_EPOCH_FORMAT_VERSION: u32 = 1;

pub(super) fn load_retirement_blocking(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> Result<Option<TenantRetirementRecord>, TenantRetirementStoreError> {
    let tenant = workload_saga_tenant().map_err(map_workload_store_error)?;
    let table = tenant_retirement_table().map_err(map_workload_store_error)?;
    let document_id = tenant_document_id(tenant_id)?;
    let unit = engine
        .begin_mutation_execution_unit(tenant, PrincipalContext::system())
        .map_err(|_| TenantRetirementStoreError::Unavailable)?;
    unit.get_document(&table, document_id)
        .map_err(|_| TenantRetirementStoreError::Unavailable)?
        .as_ref()
        .map(decode_retirement)
        .transpose()
}

pub(super) fn compare_and_swap_retirement_blocking(
    engine: &Arc<Engine>,
    expected: TenantRetirementExpected,
    next: TenantRetirementRecord,
) -> Result<TenantRetirementCommit, TenantRetirementStoreError> {
    next.validate()?;
    let tenant = workload_saga_tenant().map_err(map_workload_store_error)?;
    let table = tenant_retirement_table().map_err(map_workload_store_error)?;
    let document_id = tenant_document_id(next.tenant_id())?;
    let unit = engine
        .begin_mutation_execution_unit(tenant, PrincipalContext::system())
        .map_err(|_| TenantRetirementStoreError::Unavailable)?;
    let loaded_document = unit
        .get_document(&table, document_id.clone())
        .map_err(|_| TenantRetirementStoreError::Unavailable)?;
    let loaded = loaded_document
        .as_ref()
        .map(decode_retirement)
        .transpose()?;

    if loaded.as_ref() == Some(&next) {
        return Ok(TenantRetirementCommit::Unchanged);
    }
    let precondition = match (expected, loaded.as_ref(), loaded_document.as_ref()) {
        (TenantRetirementExpected::Missing, None, None) => {
            if next.revision().as_u64() != 0
                || next.phase() != nimbus_workloads::TenantRetirementPhase::IntentCommitted
            {
                return Err(TenantRetirementStoreError::Corrupt);
            }
            WritePrecondition::exists(false)
        }
        (TenantRetirementExpected::Missing, Some(current), _) => {
            return Err(conflict(expected, Some(current.revision())));
        }
        (TenantRetirementExpected::Revision(revision), Some(current), Some(document))
            if current.revision() == revision =>
        {
            if current.tenant_id() != next.tenant_id()
                || current.retirement_id() != next.retirement_id()
                || current.tenant_incarnation() != next.tenant_incarnation()
                || current.advance(next.phase()).as_ref() != Ok(&next)
            {
                return Err(TenantRetirementStoreError::Corrupt);
            }
            WritePrecondition::update_time(document.update_time)
        }
        (TenantRetirementExpected::Revision(_), Some(current), _) => {
            return Err(conflict(expected, Some(current.revision())));
        }
        (TenantRetirementExpected::Revision(_), None, _) => {
            return Err(conflict(expected, None));
        }
        _ => return Err(TenantRetirementStoreError::Corrupt),
    };

    unit.stage_atomic_write_batch(
        AtomicWriteBatch::new(vec![set_write(
            table,
            document_id,
            encode_retirement(&next)?,
            precondition,
        )])
        .map_err(|_| TenantRetirementStoreError::Unavailable)?,
    )
    .map_err(|_| TenantRetirementStoreError::Unavailable)?;
    commit_retirement(engine, expected, &next, &unit)
}

pub(super) fn delete_retirement_blocking(
    engine: &Arc<Engine>,
    expected: TenantRetirementRecord,
) -> Result<TenantRetirementCommit, TenantRetirementStoreError> {
    expected.validate()?;
    if !expected.phase().is_terminal() {
        return Err(TenantRetirementStoreError::Corrupt);
    }
    let tenant = workload_saga_tenant().map_err(map_workload_store_error)?;
    let table = tenant_retirement_table().map_err(map_workload_store_error)?;
    let document_id = tenant_document_id(expected.tenant_id())?;
    let unit = engine
        .begin_mutation_execution_unit(tenant, PrincipalContext::system())
        .map_err(|_| TenantRetirementStoreError::Unavailable)?;
    let loaded_document = unit
        .get_document(&table, document_id.clone())
        .map_err(|_| TenantRetirementStoreError::Unavailable)?;
    let Some(document) = loaded_document else {
        return Ok(TenantRetirementCommit::Unchanged);
    };
    let loaded = decode_retirement(&document)?;
    if loaded != expected {
        return Err(conflict(
            TenantRetirementExpected::Revision(expected.revision()),
            Some(loaded.revision()),
        ));
    }
    unit.stage_atomic_write_batch(
        AtomicWriteBatch::new(vec![AtomicWrite::Delete {
            key: WriteKey::from(DocumentLocator::new(table, document_id)),
            precondition: WritePrecondition::update_time(document.update_time),
            missing_ok: false,
        }])
        .map_err(|_| TenantRetirementStoreError::Unavailable)?,
    )
    .map_err(|_| TenantRetirementStoreError::Unavailable)?;
    match unit.commit() {
        Ok(Some(_)) => Ok(TenantRetirementCommit::Applied),
        Ok(None) => Err(TenantRetirementStoreError::Unavailable),
        Err(Error::Conflict { .. }) => Err(TenantRetirementStoreError::Conflict {
            expected: TenantRetirementExpected::Revision(expected.revision()),
            observed: load_retirement_blocking(engine, expected.tenant_id())?
                .map(|record| record.revision()),
        }),
        Err(_) => match load_retirement_blocking(engine, expected.tenant_id()) {
            Ok(None) => Ok(TenantRetirementCommit::Applied),
            Ok(Some(current)) if current == expected => Err(TenantRetirementStoreError::Ambiguous),
            Ok(Some(_)) => Err(TenantRetirementStoreError::Corrupt),
            Err(_) => Err(TenantRetirementStoreError::Ambiguous),
        },
    }
}

pub(super) async fn list_active_retirements(
    engine: &Arc<Engine>,
    request: TenantRetirementPageRequest,
) -> Result<TenantRetirementPage, TenantRetirementStoreError> {
    let tenant = workload_saga_tenant().map_err(map_workload_store_error)?;
    let table = tenant_retirement_table().map_err(map_workload_store_error)?;
    let mut filters = vec![Filter {
        field: "active".to_owned(),
        op: FilterOp::Eq,
        value: Value::Bool(true),
    }];
    if let Some(cursor) = request.after() {
        filters.push(Filter {
            field: "retirementId".to_owned(),
            op: FilterOp::Gt,
            value: Value::String(cursor.retirement_id().as_str().to_owned()),
        });
    }
    let documents = engine
        .query_documents_async_with_principal(
            tenant,
            Query {
                table,
                filters,
                order: Some(OrderBy {
                    field: "retirementId".to_owned(),
                    direction: OrderDirection::Asc,
                }),
                limit: Some(usize::from(request.limit()).saturating_add(1)),
            },
            PrincipalContext::system(),
        )
        .await
        .map_err(|_| TenantRetirementStoreError::Unavailable)?;
    if documents.len() > usize::from(request.limit()).saturating_add(1) {
        return Err(TenantRetirementStoreError::Corrupt);
    }
    let has_more = documents.len() > usize::from(request.limit());
    let records = documents
        .into_iter()
        .take(usize::from(request.limit()))
        .map(|document| decode_retirement(&document))
        .collect::<Result<Vec<_>, _>>()?;
    TenantRetirementPage::active(&request, records, has_more)
}

pub(super) async fn list_retirements(
    engine: &Arc<Engine>,
    request: TenantRetirementPageRequest,
) -> Result<TenantRetirementPage, TenantRetirementStoreError> {
    let tenant = workload_saga_tenant().map_err(map_workload_store_error)?;
    let table = tenant_retirement_table().map_err(map_workload_store_error)?;
    let mut filters = Vec::new();
    if let Some(cursor) = request.after() {
        filters.push(Filter {
            field: "retirementId".to_owned(),
            op: FilterOp::Gt,
            value: Value::String(cursor.retirement_id().as_str().to_owned()),
        });
    }
    let documents = engine
        .query_documents_async_with_principal(
            tenant,
            Query {
                table,
                filters,
                order: Some(OrderBy {
                    field: "retirementId".to_owned(),
                    direction: OrderDirection::Asc,
                }),
                limit: Some(usize::from(request.limit()).saturating_add(1)),
            },
            PrincipalContext::system(),
        )
        .await
        .map_err(|_| TenantRetirementStoreError::Unavailable)?;
    if documents.len() > usize::from(request.limit()).saturating_add(1) {
        return Err(TenantRetirementStoreError::Corrupt);
    }
    let has_more = documents.len() > usize::from(request.limit());
    let records = documents
        .into_iter()
        .take(usize::from(request.limit()))
        .map(|document| decode_retirement(&document))
        .collect::<Result<Vec<_>, _>>()?;
    TenantRetirementPage::retained(&request, records, has_more)
}

pub(super) fn load_workload_mutation_epoch_blocking(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> Result<TenantWorkloadMutationEpoch, TenantRetirementStoreError> {
    let tenant = workload_saga_tenant().map_err(map_workload_store_error)?;
    let table = workload_saga_tenant_epoch_table().map_err(map_workload_store_error)?;
    let document_id = tenant_document_id(tenant_id)?;
    let unit = engine
        .begin_mutation_execution_unit(tenant, PrincipalContext::system())
        .map_err(|_| TenantRetirementStoreError::Unavailable)?;
    unit.get_document(&table, document_id)
        .map_err(|_| TenantRetirementStoreError::Unavailable)?
        .as_ref()
        .map(|document| decode_epoch(document, tenant_id))
        .transpose()
        .map(|epoch| epoch.unwrap_or(TenantWorkloadMutationEpoch::new(0)))
}

pub(super) struct EpochWrite {
    pub(super) write: AtomicWrite,
}

pub(super) fn next_epoch_write(
    unit: &nimbus_engine::MutationExecutionUnit,
    tenant_id: &TenantId,
) -> Result<EpochWrite, nimbus_workloads::WorkloadSagaStoreError> {
    let table = workload_saga_tenant_epoch_table()?;
    let document_id = DocumentId::from_key(tenant_id.as_str())
        .map_err(|_| nimbus_workloads::WorkloadSagaStoreError::Corrupt)?;
    let document = unit
        .get_document(&table, document_id.clone())
        .map_err(|_| nimbus_workloads::WorkloadSagaStoreError::Unavailable)?;
    let current = document
        .as_ref()
        .map(|document| {
            decode_epoch(document, tenant_id)
                .map_err(|_| nimbus_workloads::WorkloadSagaStoreError::Corrupt)
        })
        .transpose()?
        .unwrap_or(TenantWorkloadMutationEpoch::new(0));
    let next = current
        .checked_next()
        .ok_or(nimbus_workloads::WorkloadSagaStoreError::Corrupt)?;
    let precondition = document.as_ref().map_or_else(
        || WritePrecondition::exists(false),
        |document| WritePrecondition::update_time(document.update_time),
    );
    Ok(EpochWrite {
        write: set_write(
            table,
            document_id,
            json!({
                "formatVersion": TENANT_EPOCH_FORMAT_VERSION,
                "tenantId": tenant_id.as_str(),
                "mutationEpoch": next.to_string(),
            })
            .as_object()
            .expect("tenant epoch encoding should be an object")
            .clone(),
            precondition,
        ),
    })
}

fn encode_retirement(
    record: &TenantRetirementRecord,
) -> Result<Map<String, Value>, TenantRetirementStoreError> {
    record.validate()?;
    let portable = serde_json::to_value(record).map_err(|_| TenantRetirementStoreError::Corrupt)?;
    let mut fields = portable
        .as_object()
        .ok_or(TenantRetirementStoreError::Corrupt)?
        .clone();
    fields.insert(
        "active".to_owned(),
        Value::Bool(!record.phase().is_terminal()),
    );
    Ok(fields)
}

fn decode_retirement(
    document: &Document,
) -> Result<TenantRetirementRecord, TenantRetirementStoreError> {
    let allowed = TENANT_RETIREMENT_FIELDS
        .into_iter()
        .collect::<BTreeSet<_>>();
    if document
        .fields
        .keys()
        .any(|field| !allowed.contains(field.as_str()))
        || allowed
            .iter()
            .any(|field| !document.fields.contains_key(*field))
    {
        return Err(TenantRetirementStoreError::Corrupt);
    }
    let mut portable = document.fields.clone();
    let active = portable
        .remove("active")
        .and_then(|value| value.as_bool())
        .ok_or(TenantRetirementStoreError::Corrupt)?;
    let record: TenantRetirementRecord = serde_json::from_value(Value::Object(portable))
        .map_err(|_| TenantRetirementStoreError::Corrupt)?;
    record.validate()?;
    if active == record.phase().is_terminal()
        || document.id != tenant_document_id(record.tenant_id())?
    {
        return Err(TenantRetirementStoreError::Corrupt);
    }
    Ok(record)
}

fn decode_epoch(
    document: &Document,
    expected_tenant: &TenantId,
) -> Result<TenantWorkloadMutationEpoch, TenantRetirementStoreError> {
    let allowed = TENANT_EPOCH_FIELDS.into_iter().collect::<BTreeSet<_>>();
    if document
        .fields
        .keys()
        .any(|field| !allowed.contains(field.as_str()))
        || allowed
            .iter()
            .any(|field| !document.fields.contains_key(*field))
        || document.fields.get("formatVersion") != Some(&json!(TENANT_EPOCH_FORMAT_VERSION))
        || document.fields.get("tenantId") != Some(&json!(expected_tenant.as_str()))
        || document.id != tenant_document_id(expected_tenant)?
    {
        return Err(TenantRetirementStoreError::Corrupt);
    }
    serde_json::from_value(
        document
            .fields
            .get("mutationEpoch")
            .cloned()
            .ok_or(TenantRetirementStoreError::Corrupt)?,
    )
    .map_err(|_| TenantRetirementStoreError::Corrupt)
}

fn commit_retirement(
    engine: &Arc<Engine>,
    expected: TenantRetirementExpected,
    next: &TenantRetirementRecord,
    unit: &nimbus_engine::MutationExecutionUnit,
) -> Result<TenantRetirementCommit, TenantRetirementStoreError> {
    match unit.commit() {
        Ok(Some(_)) => Ok(TenantRetirementCommit::Applied),
        Ok(None) => Err(TenantRetirementStoreError::Unavailable),
        Err(Error::Conflict { .. }) => Err(conflict(
            expected,
            load_retirement_blocking(engine, next.tenant_id())?.map(|record| record.revision()),
        )),
        Err(_) => match load_retirement_blocking(engine, next.tenant_id()) {
            Ok(Some(current)) if current == *next => Ok(TenantRetirementCommit::Applied),
            Ok(Some(current)) if Some(current.revision()) != expected_revision(expected) => {
                Err(conflict(expected, Some(current.revision())))
            }
            Ok(None) if matches!(expected, TenantRetirementExpected::Missing) => {
                Err(TenantRetirementStoreError::Ambiguous)
            }
            Ok(_) | Err(_) => Err(TenantRetirementStoreError::Ambiguous),
        },
    }
}

fn set_write(
    table: nimbus_core::TableName,
    document_id: DocumentId,
    document: Map<String, Value>,
    precondition: WritePrecondition,
) -> AtomicWrite {
    AtomicWrite::Set {
        key: WriteKey::from(DocumentLocator::new(table, document_id)),
        document,
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition,
        transforms: Vec::new(),
    }
}

fn tenant_document_id(tenant_id: &TenantId) -> Result<DocumentId, TenantRetirementStoreError> {
    DocumentId::from_key(tenant_id.as_str()).map_err(|_| TenantRetirementStoreError::Corrupt)
}

fn conflict(
    expected: TenantRetirementExpected,
    observed: Option<TenantRetirementRevision>,
) -> TenantRetirementStoreError {
    TenantRetirementStoreError::Conflict { expected, observed }
}

fn expected_revision(expected: TenantRetirementExpected) -> Option<TenantRetirementRevision> {
    match expected {
        TenantRetirementExpected::Missing => None,
        TenantRetirementExpected::Revision(revision) => Some(revision),
    }
}

fn map_workload_store_error(
    error: nimbus_workloads::WorkloadSagaStoreError,
) -> TenantRetirementStoreError {
    match error {
        nimbus_workloads::WorkloadSagaStoreError::Corrupt
        | nimbus_workloads::WorkloadSagaStoreError::InvalidTransition(_) => {
            TenantRetirementStoreError::Corrupt
        }
        nimbus_workloads::WorkloadSagaStoreError::Ambiguous => {
            TenantRetirementStoreError::Ambiguous
        }
        nimbus_workloads::WorkloadSagaStoreError::Unavailable
        | nimbus_workloads::WorkloadSagaStoreError::Conflict { .. } => {
            TenantRetirementStoreError::Unavailable
        }
    }
}
