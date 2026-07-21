use std::sync::Arc;

use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, Document, DocumentId, DocumentLocator, Error, Filter, FilterOp,
    PrincipalContext, Query, Result, TableName, TenantId, WriteKey, WritePrecondition,
    WriteSetMode,
};
use nimbus_engine::{Engine, ProjectionToken};
use serde_json::{Map, Value, json};

use crate::identity::system_tenant_id;
use crate::keys::table_document_id;
use crate::schema::{PROJECTION_FENCE_TABLE, SystemTable};

const MAX_CONFLICT_ATTEMPTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProjectionPublicationOutcome {
    Applied,
    StaleNoOp,
}

pub(crate) struct ProjectionPublication {
    pub tenant_id: TenantId,
    pub table: TableName,
    pub token: ProjectionToken,
    pub visible_fields: Map<String, Value>,
    pub delete_visible: bool,
}

/// Returns previously projected table scopes, including deleted rows whose
/// only remaining identity is the private durable tombstone.
pub(crate) async fn projection_fence_tables_for_tenant_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> Result<Vec<TableName>> {
    let rows = match engine
        .query_documents_async(
            system_tenant_id()?,
            Query {
                table: TableName::new(PROJECTION_FENCE_TABLE)?,
                filters: vec![Filter {
                    field: "tenantId".to_string(),
                    op: FilterOp::Eq,
                    value: json!(tenant_id.as_str()),
                }],
                order: None,
                limit: None,
            },
        )
        .await
    {
        Ok(rows) => rows,
        Err(Error::TenantNotFound(_)) | Err(Error::SchemaNotFound(_)) => Vec::new(),
        Err(error) => return Err(error),
    };
    let mut tables = rows
        .into_iter()
        .map(|row| {
            let name = row
                .fields
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Error::Serialization(format!(
                        "private projection fence {} is missing string field name",
                        row.id
                    ))
                })?;
            TableName::new(name.to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    tables.sort();
    tables.dedup();
    Ok(tables)
}

/// Publishes one sampled table state behind its durable source token.
///
/// The visible row, its indexes, the private deletion-surviving fence, and the
/// system tenant's commit log are staged through one `MutationExecutionUnit`.
/// Only a fresh-snapshot OCC conflict is retried here. Every other error is
/// returned to the Drop-owned scheduler, which retains the table scope and
/// retries later with backoff; this is essential for ambiguous outcomes.
pub(crate) async fn publish_table_projection_async(
    engine: &Arc<Engine>,
    publication: ProjectionPublication,
) -> Result<ProjectionPublicationOutcome> {
    for attempt in 1..=MAX_CONFLICT_ATTEMPTS {
        let engine = engine.clone();
        let tenant_id = publication.tenant_id.clone();
        let table = publication.table.clone();
        let visible_fields = publication.visible_fields.clone();
        let token = publication.token;
        let delete_visible = publication.delete_visible;
        let result = tokio::task::spawn_blocking(move || {
            publish_table_projection_once(
                &engine,
                &tenant_id,
                &table,
                token,
                visible_fields,
                delete_visible,
            )
        })
        .await
        .map_err(|error| Error::Internal(format!("projection publication task failed: {error}")))?;

        match result {
            Err(error @ Error::Conflict { .. }) if attempt < MAX_CONFLICT_ATTEMPTS => {
                tracing::debug!(
                    tenant = %publication.tenant_id,
                    table = %publication.table,
                    attempt,
                    "retrying projection publication from a fresh snapshot after OCC conflict"
                );
                drop(error);
            }
            Err(error @ Error::Conflict { .. }) => {
                return Err(error.with_conflict_attempts(attempt));
            }
            other => return other,
        }
    }
    unreachable!("the bounded projection conflict loop always returns")
}

fn publish_table_projection_once(
    engine: &Arc<Engine>,
    source_tenant_id: &TenantId,
    source_table: &TableName,
    token: ProjectionToken,
    visible_fields: Map<String, Value>,
    delete_visible: bool,
) -> Result<ProjectionPublicationOutcome> {
    let system_tenant = system_tenant_id()?;
    let visible_table = SystemTable::Tables.table_name()?;
    let fence_table = TableName::new(PROJECTION_FENCE_TABLE)?;
    let document_id = DocumentId::from_key(table_document_id(source_tenant_id, source_table))?;
    let unit = engine.begin_mutation_execution_unit(system_tenant, PrincipalContext::system())?;

    let fence = unit.get_document(&fence_table, document_id.clone())?;
    if fence
        .as_ref()
        .map(projection_token_from_fence)
        .transpose()?
        .is_some_and(|current| current >= token)
    {
        return Ok(ProjectionPublicationOutcome::StaleNoOp);
    }

    let fence_fields = object_fields(json!({
        "tenantId": source_tenant_id.as_str(),
        "name": source_table.as_str(),
        "tenantIncarnation": token.tenant_incarnation,
        "leaseEpoch": token.lease_epoch,
        "durableSequence": token.durable_sequence.0,
        "deleted": delete_visible,
    }));
    let mut writes = Vec::with_capacity(2);
    writes.push(overwrite(fence_table, document_id.clone(), fence_fields));
    if delete_visible {
        writes.push(AtomicWrite::Delete {
            key: write_key(visible_table, document_id),
            precondition: WritePrecondition::default(),
            missing_ok: true,
        });
    } else {
        writes.push(overwrite(visible_table, document_id, visible_fields));
    }
    unit.stage_atomic_write_batch(AtomicWriteBatch::new(writes)?)?;
    let commit = unit.commit()?;
    if commit.is_none() {
        return Err(Error::Internal(
            "higher projection token produced no durable execution-unit commit".to_string(),
        ));
    }
    Ok(ProjectionPublicationOutcome::Applied)
}

fn projection_token_from_fence(document: &Document) -> Result<ProjectionToken> {
    let tenant_incarnation = required_u64(document, "tenantIncarnation")?;
    let lease_epoch = required_u64(document, "leaseEpoch")?;
    let durable_sequence = required_u64(document, "durableSequence")?;
    Ok(ProjectionToken {
        tenant_incarnation,
        lease_epoch,
        durable_sequence: nimbus_core::SequenceNumber(durable_sequence),
    })
}

fn required_u64(document: &Document, field: &str) -> Result<u64> {
    document
        .fields
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            Error::Serialization(format!(
                "private projection fence {} is missing numeric field {field}",
                document.id
            ))
        })
}

fn overwrite(table: TableName, id: DocumentId, document: Map<String, Value>) -> AtomicWrite {
    AtomicWrite::Set {
        key: write_key(table, id),
        document,
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    }
}

fn write_key(table: TableName, id: DocumentId) -> WriteKey {
    WriteKey::from(DocumentLocator::new(table, id))
}

fn object_fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        _ => unreachable!("projection publication payload must be an object"),
    }
}

#[cfg(test)]
#[path = "publication/tests.rs"]
mod tests;
