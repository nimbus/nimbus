use std::sync::Arc;

use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, AtomicWriteBatchOutcome, Document, DocumentPath, Error,
    PrincipalContext, ResourcePathBinding, Result, StructuredAggregationQuery, StructuredQuery,
    TenantId, Timestamp, TransactionSession, TransactionSessionMode, TransactionSessionToken,
    WriteKey,
};

use super::batch_get_request;
use super::commit_request;
use super::errors::{list_collection_ids_request_error_to_core, resource_name_error_to_core};
use super::list_collection_ids_request;
use super::resource_names;
use super::response::firestore_parent_name;
use super::{
    BatchGetDocumentEntry, BatchGetDocumentsOutcome, BatchWriteEntryOutcome, BatchWriteOutcome,
    RunAggregationQueryOutcome, RunQueryDocument, RunQueryOutcome,
};
use crate::provider_family::firestore::{
    locator_for_document_path, storage_table_for_collection_path,
};
use crate::state::AppState;

pub(crate) fn resolve_write_key(
    document_path: &DocumentPath,
) -> std::result::Result<WriteKey, commit_request::FirestoreCommitRequestError> {
    let binding = ResourcePathBinding::new(
        locator_for_document_path(document_path).map_err(|error| {
            commit_request::FirestoreCommitRequestError::InvalidRequest(error.to_string())
        })?,
        document_path.clone(),
    );
    Ok(WriteKey::from(binding))
}

pub(crate) fn commit_batch_for_database(
    state: &Arc<AppState>,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    batch: AtomicWriteBatch,
    transaction: Option<&[u8]>,
) -> Result<AtomicWriteBatchOutcome> {
    let tenant_id = tenant_id_for_database(database)?;
    match transaction {
        Some(transaction_bytes) => {
            let transaction_token = decode_transaction_token(transaction_bytes)?;
            state.service.commit_transaction_session(
                &tenant_id,
                &transaction_token,
                principal,
                Some(batch),
            )
        }
        None => state
            .service
            .begin_mutation_execution_unit(tenant_id, principal.clone())
            .and_then(|execution_unit| execution_unit.execute_atomic_write_batch(batch)),
    }
}

pub(crate) fn begin_transaction_session_for_database(
    state: &Arc<AppState>,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    mode: TransactionSessionMode,
) -> Result<TransactionSession> {
    state.service.begin_transaction_session(
        tenant_id_for_database(database)?,
        principal.clone(),
        mode,
    )
}

pub(crate) fn rollback_transaction_session_for_database(
    state: &Arc<AppState>,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    transaction: &[u8],
) -> Result<()> {
    let tenant_id = tenant_id_for_database(database)?;
    let token = decode_transaction_token(transaction)?;
    state
        .service
        .rollback_transaction_session(&tenant_id, &token, principal)
}

pub(crate) fn tenant_id_for_database(
    database: &resource_names::FirestoreDatabaseName,
) -> Result<TenantId> {
    TenantId::new(database.project_id.clone())
}

pub(crate) fn decode_transaction_token(bytes: &[u8]) -> Result<TransactionSessionToken> {
    let token = String::from_utf8(bytes.to_vec()).map_err(|error| {
        Error::InvalidInput(format!(
            "transaction bytes must decode to a UTF-8 token string: {error}"
        ))
    })?;
    TransactionSessionToken::new(token)
}

pub(crate) fn batch_get_documents_for_database(
    state: &Arc<AppState>,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    request: &batch_get_request::ParsedBatchGetRequest,
) -> Result<BatchGetDocumentsOutcome> {
    let tenant_id = tenant_id_for_database(database)?;
    let read_time = Timestamp::now();
    let transaction_token = request
        .transaction
        .as_deref()
        .map(decode_transaction_token)
        .transpose()?;
    request
        .documents
        .iter()
        .map(|requested_document| {
            let document = read_batch_get_document(
                state,
                &tenant_id,
                principal,
                transaction_token.as_ref(),
                &requested_document.document_path,
            )?;
            Ok(BatchGetDocumentEntry {
                document_name: requested_document.document_name.clone(),
                document,
            })
        })
        .collect::<Result<Vec<_>>>()
        .map(|entries| BatchGetDocumentsOutcome { entries, read_time })
}

pub(crate) fn batch_write_for_database(
    state: &Arc<AppState>,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    writes: Vec<AtomicWrite>,
) -> Result<BatchWriteOutcome> {
    let tenant_id = tenant_id_for_database(database)?;
    let mut entries = Vec::with_capacity(writes.len());

    for write in writes {
        let outcome = state
            .service
            .begin_mutation_execution_unit(tenant_id.clone(), principal.clone())
            .and_then(|execution_unit| {
                execution_unit.execute_atomic_write_batch(
                    AtomicWriteBatch::new(vec![write])
                        .expect("single-write batch construction should succeed"),
                )
            });
        match outcome {
            Ok(outcome) => entries.push(BatchWriteEntryOutcome {
                write_result: outcome.write_results.into_iter().next(),
                error: None,
            }),
            Err(error) => entries.push(BatchWriteEntryOutcome {
                write_result: None,
                error: Some(error),
            }),
        }
    }

    Ok(BatchWriteOutcome { entries })
}

pub(crate) fn list_collection_ids_for_database(
    state: &Arc<AppState>,
    database: &resource_names::FirestoreDatabaseName,
    parent_document_path: Option<&DocumentPath>,
    request: &list_collection_ids_request::ParsedListCollectionIdsRequest,
) -> Result<list_collection_ids_request::PaginatedCollectionIds> {
    let tenant_id = tenant_id_for_database(database)?;
    let collection_ids = state
        .service
        .list_collection_ids_for_parent(&tenant_id, parent_document_path)?
        .into_iter()
        .map(|collection_id| collection_id.to_string())
        .collect::<Vec<_>>();
    list_collection_ids_request::paginate_collection_ids(collection_ids, request)
        .map_err(list_collection_ids_request_error_to_core)
}

pub(crate) fn get_document_for_database(
    state: &Arc<AppState>,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    document_path: &DocumentPath,
    transaction: Option<&[u8]>,
) -> Result<Option<Document>> {
    let tenant_id = tenant_id_for_database(database)?;
    let transaction_token = transaction.map(decode_transaction_token).transpose()?;
    read_batch_get_document(
        state,
        &tenant_id,
        principal,
        transaction_token.as_ref(),
        document_path,
    )
}

fn resolve_run_query_collection_target(
    database: &resource_names::FirestoreDatabaseName,
    parent_document_path: Option<&DocumentPath>,
    query: &StructuredQuery,
) -> Result<resource_names::FirestoreCollectionTarget> {
    match query.from.as_slice() {
        [] => Err(Error::InvalidInput(
            "RunQuery `structuredQuery.from` must contain exactly one collection selector"
                .to_string(),
        )),
        [selector] => {
            let parent_resource = firestore_parent_name(database, parent_document_path);
            resource_names::parse_collection_target(
                &parent_resource,
                selector.collection_id.as_str(),
            )
            .map_err(resource_name_error_to_core)
        }
        _ => Err(Error::InvalidInput(
            "structured query feature not yet supported: multiple query sources".to_string(),
        )),
    }
}

fn read_batch_get_document(
    state: &Arc<AppState>,
    tenant_id: &TenantId,
    principal: &PrincipalContext,
    transaction_token: Option<&TransactionSessionToken>,
    document_path: &DocumentPath,
) -> Result<Option<Document>> {
    let locator = locator_for_document_path(document_path)?;
    match transaction_token {
        Some(transaction_token) => state.service.get_document_in_transaction(
            tenant_id,
            transaction_token,
            principal,
            &locator.table,
            locator.id,
        ),
        None => match state.service.get_document_with_principal(
            tenant_id,
            &locator.table,
            locator.id,
            principal,
        ) {
            Ok(document) => Ok(Some(document)),
            Err(Error::DocumentNotFound(_)) => Ok(None),
            Err(error) => Err(error),
        },
    }
}

pub(crate) fn run_query_documents_for_database(
    state: &Arc<AppState>,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    parent_document_path: Option<&DocumentPath>,
    mut structured_query: StructuredQuery,
    transaction: Option<&[u8]>,
) -> Result<RunQueryOutcome> {
    let tenant_id = tenant_id_for_database(database)?;
    let transaction_token = transaction.map(decode_transaction_token).transpose()?;
    let collection_target =
        resolve_run_query_collection_target(database, parent_document_path, &structured_query)?;
    let is_collection_group = structured_query
        .from
        .first()
        .is_some_and(nimbus_core::CollectionSelector::is_collection_group);
    let skipped_results = structured_query.offset.unwrap_or(0) as usize;
    structured_query.from.clear();
    let documents = if is_collection_group {
        match transaction_token.as_ref() {
            Some(transaction_token) => state
                .service
                .query_collection_group_documents_structured_in_transaction(
                    &tenant_id,
                    transaction_token,
                    principal,
                    &collection_target.collection_group,
                    parent_document_path,
                    &structured_query,
                )?,
            None => state
                .service
                .query_collection_group_documents_structured_with_principal_cancellable(
                    &tenant_id,
                    &collection_target.collection_group,
                    parent_document_path,
                    &structured_query,
                    principal,
                    &mut || Ok(()),
                )?,
        }
        .into_iter()
        .map(|(document_path, document)| RunQueryDocument {
            document_path,
            document,
        })
        .collect::<Vec<_>>()
    } else {
        let collection_path = collection_target.collection_path.clone();
        let table = storage_table_for_collection_path(&collection_path)?;
        match transaction_token.as_ref() {
            Some(transaction_token) => state.service.query_documents_structured_in_transaction(
                &tenant_id,
                transaction_token,
                principal,
                &table,
                &structured_query,
            )?,
            None => state.service.query_documents_structured_with_principal(
                &tenant_id,
                &table,
                &structured_query,
                principal,
            )?,
        }
        .into_iter()
        .map(|document| RunQueryDocument {
            document_path: DocumentPath::new(collection_path.clone(), document.id.clone()),
            document,
        })
        .collect::<Vec<_>>()
    };
    Ok(RunQueryOutcome {
        documents,
        read_time: Timestamp::now(),
        skipped_results,
    })
}

pub(crate) fn run_aggregation_query_for_database(
    state: &Arc<AppState>,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    parent_document_path: Option<&DocumentPath>,
    mut aggregation_query: StructuredAggregationQuery,
) -> Result<RunAggregationQueryOutcome> {
    let tenant_id = tenant_id_for_database(database)?;
    let collection_target = resolve_run_query_collection_target(
        database,
        parent_document_path,
        &aggregation_query.structured_query,
    )?;
    let is_collection_group = aggregation_query
        .structured_query
        .from
        .first()
        .is_some_and(nimbus_core::CollectionSelector::is_collection_group);
    aggregation_query.structured_query.from.clear();
    let result = if is_collection_group {
        state
            .service
            .aggregate_collection_group_documents_structured_with_principal_cancellable(
                &tenant_id,
                &collection_target.collection_group,
                parent_document_path,
                &aggregation_query,
                principal,
                &mut || Ok(()),
            )?
    } else {
        let table = storage_table_for_collection_path(&collection_target.collection_path)?;
        state
            .service
            .aggregate_documents_structured_with_principal_cancellable(
                &tenant_id,
                &table,
                &aggregation_query,
                principal,
                &mut || Ok(()),
            )?
    };

    Ok(RunAggregationQueryOutcome {
        result,
        read_time: Timestamp::now(),
    })
}
