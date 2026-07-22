use std::sync::Arc;

use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, AtomicWriteBatchOutcome, Document, DocumentPath, Error,
    PrincipalContext, ResourcePathBinding, Result, StructuredAggregationQuery, StructuredQuery,
    SystemWallClock, TenantId, TransactionSession, TransactionSessionMode, TransactionSessionToken,
    WallClock, WriteKey,
};
use nimbus_core::{locator_for_document_path, storage_table_for_collection_path};
use nimbus_engine::Engine;
use nimbus_tenant::TenantIsolationContext;

use crate::project_tenant_registry::{
    ProjectTenantRegistry, firebase_project_from_verified_principal,
};

use super::batch_get_request;
use super::errors::{firestore_request_error_to_core, resource_name_error_to_core};
use super::list_collection_ids_request;
use super::request_error::{FirestoreRequestError, FirestoreRpc};
use super::resource_names;
use super::response::firestore_parent_name;
use super::{
    BatchGetDocumentEntry, BatchGetDocumentsOutcome, BatchWriteEntryOutcome, BatchWriteOutcome,
    RunAggregationQueryOutcome, RunQueryDocument, RunQueryOutcome,
};

pub fn resolve_write_key(
    document_path: &DocumentPath,
) -> std::result::Result<WriteKey, FirestoreRequestError> {
    let binding = ResourcePathBinding::new(
        locator_for_document_path(document_path).map_err(|error| {
            FirestoreRequestError::invalid_request(FirestoreRpc::Commit, error.to_string())
        })?,
        document_path.clone(),
    );
    Ok(WriteKey::from(binding))
}

pub fn commit_batch_for_database(
    registry: &ProjectTenantRegistry,
    engine: &Arc<Engine>,
    isolation: &TenantIsolationContext,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    batch: AtomicWriteBatch,
    transaction: Option<&[u8]>,
) -> Result<AtomicWriteBatchOutcome> {
    let tenant_id =
        tenant_id_for_context_database(registry, isolation, database, "Firestore commit database")?;
    match transaction {
        Some(transaction_bytes) => {
            let transaction_token = decode_transaction_token(transaction_bytes)?;
            engine.commit_transaction_session(tenant_id, &transaction_token, principal, Some(batch))
        }
        None => engine
            .begin_mutation_execution_unit(tenant_id.clone(), principal.clone())
            .and_then(|execution_unit| execution_unit.execute_atomic_write_batch(batch)),
    }
}

pub fn begin_transaction_session_for_database(
    registry: &ProjectTenantRegistry,
    engine: &Arc<Engine>,
    isolation: &TenantIsolationContext,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    mode: TransactionSessionMode,
) -> Result<TransactionSession> {
    let tenant_id = tenant_id_for_context_database(
        registry,
        isolation,
        database,
        "Firestore transaction database",
    )?;
    engine.begin_transaction_session(tenant_id.clone(), principal.clone(), mode)
}

pub fn rollback_transaction_session_for_database(
    registry: &ProjectTenantRegistry,
    engine: &Arc<Engine>,
    isolation: &TenantIsolationContext,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    transaction: &[u8],
) -> Result<()> {
    let tenant_id = tenant_id_for_context_database(
        registry,
        isolation,
        database,
        "Firestore rollback database",
    )?;
    let token = decode_transaction_token(transaction)?;
    engine.rollback_transaction_session(tenant_id, &token, principal)
}

/// Bind a Firestore request to a tenant — the #24 admission gate.
///
/// This is the single place a Firestore request's tenant is decided, and the
/// **only** project→tenant mapping in the adapter. The pre-#24 verbatim
/// `TenantId::new(project_id)` path is gone: a request is admitted only if
///
/// 1. the principal carries a **verified** Firebase project (its token issuer,
///    `securetoken.google.com/<project>`) — an anonymous or unverified caller
///    has none and is refused;
/// 2. that verified project resolves through the [`ProjectTenantRegistry`] to a
///    tenant; and
/// 3. the URL `project_id` resolves through the same registry to the **same**
///    tenant — so a token minted for project X may reach project Y when both
///    belong to one tenant (many-projects-per-tenant), but never a project that
///    belongs to a different tenant.
///
/// The returned context's tenant is the registry-resolved tenant, never the URL
/// project verbatim. There is no path here that yields a context from the URL
/// project without a verified token and two registry resolutions, so an
/// unverified request can never fall through to a URL-derived tenant (#24
/// defense-in-depth: the fail-open is structurally absent, not merely unused).
pub fn tenant_context_for_database(
    registry: &ProjectTenantRegistry,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    surface: &'static str,
) -> Result<TenantIsolationContext> {
    let verified_project = firebase_project_from_verified_principal(principal).ok_or_else(|| {
        Error::PermissionDenied(format!(
            "Firestore request to project `{}` has no verified Firebase project; an anonymous or \
             unverified principal cannot select a project",
            database.project_id
        ))
    })?;
    let token_tenant = registry.resolve(&verified_project)?;
    let url_tenant = registry.resolve(&database.project_id)?;
    if token_tenant != url_tenant {
        return Err(Error::PermissionDenied(format!(
            "verified Firebase project `{verified_project}` (tenant `{token_tenant}`) is not \
             authorized for project `{}` (tenant `{url_tenant}`)",
            database.project_id
        )));
    }
    Ok(TenantIsolationContext::application(
        url_tenant,
        principal.clone(),
        surface,
    ))
}

/// Re-resolve the tenant for one operation's database and confirm it matches the
/// bound context — the per-op defense-in-depth check every data op runs.
///
/// Like the gate, this maps the project to a tenant **only** through the registry
/// (strict; no verbatim `TenantId::new`), so an unregistered project can never
/// resolve here either.
fn tenant_id_for_context_database<'a>(
    registry: &ProjectTenantRegistry,
    isolation: &'a TenantIsolationContext,
    database: &resource_names::FirestoreDatabaseName,
    context: &str,
) -> Result<&'a TenantId> {
    let database_tenant_id = registry.resolve(&database.project_id)?;
    isolation.ensure_tenant_matches(&database_tenant_id, context)?;
    Ok(isolation.tenant_id())
}

pub fn decode_transaction_token(bytes: &[u8]) -> Result<TransactionSessionToken> {
    let token = String::from_utf8(bytes.to_vec()).map_err(|error| {
        Error::InvalidInput(format!(
            "transaction bytes must decode to a UTF-8 token string: {error}"
        ))
    })?;
    TransactionSessionToken::new(token)
}

pub fn batch_get_documents_for_database(
    registry: &ProjectTenantRegistry,
    engine: &Arc<Engine>,
    isolation: &TenantIsolationContext,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    request: &batch_get_request::ParsedBatchGetRequest,
) -> Result<BatchGetDocumentsOutcome> {
    let tenant_id = tenant_id_for_context_database(
        registry,
        isolation,
        database,
        "Firestore batch-get database",
    )?;
    let read_time = SystemWallClock.now();
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
                engine,
                tenant_id,
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

pub fn batch_write_for_database(
    registry: &ProjectTenantRegistry,
    engine: &Arc<Engine>,
    isolation: &TenantIsolationContext,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    writes: Vec<AtomicWrite>,
) -> Result<BatchWriteOutcome> {
    let tenant_id = tenant_id_for_context_database(
        registry,
        isolation,
        database,
        "Firestore batch-write database",
    )?;
    let mut entries = Vec::with_capacity(writes.len());

    for write in writes {
        let outcome = engine
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

pub fn list_collection_ids_for_database(
    registry: &ProjectTenantRegistry,
    engine: &Arc<Engine>,
    isolation: &TenantIsolationContext,
    database: &resource_names::FirestoreDatabaseName,
    parent_document_path: Option<&DocumentPath>,
    request: &list_collection_ids_request::ParsedListCollectionIdsRequest,
) -> Result<list_collection_ids_request::PaginatedCollectionIds> {
    let tenant_id = tenant_id_for_context_database(
        registry,
        isolation,
        database,
        "Firestore list-collections database",
    )?;
    let collection_ids = engine
        .list_collection_ids_for_parent(tenant_id, parent_document_path)?
        .into_iter()
        .map(|collection_id| collection_id.to_string())
        .collect::<Vec<_>>();
    list_collection_ids_request::paginate_collection_ids(collection_ids, request)
        .map_err(firestore_request_error_to_core)
}

pub fn get_document_for_database(
    registry: &ProjectTenantRegistry,
    engine: &Arc<Engine>,
    isolation: &TenantIsolationContext,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    document_path: &DocumentPath,
    transaction: Option<&[u8]>,
) -> Result<Option<Document>> {
    let tenant_id =
        tenant_id_for_context_database(registry, isolation, database, "Firestore get database")?;
    let transaction_token = transaction.map(decode_transaction_token).transpose()?;
    read_batch_get_document(
        engine,
        tenant_id,
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
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    principal: &PrincipalContext,
    transaction_token: Option<&TransactionSessionToken>,
    document_path: &DocumentPath,
) -> Result<Option<Document>> {
    let locator = locator_for_document_path(document_path)?;
    match transaction_token {
        Some(transaction_token) => engine.get_document_in_transaction(
            tenant_id,
            transaction_token,
            principal,
            &locator.table,
            locator.id,
        ),
        None => match engine.get_document_with_principal(
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

// `registry` is a cross-cutting tenant-binding parameter threaded uniformly
// through every Firestore data op (#24); it pushes this query path to eight
// arguments. Bundling the rest into a struct would diverge from the sibling ops
// for no clarity gain, so the registry threading is allowed past the lint here.
#[allow(clippy::too_many_arguments)]
pub fn run_query_documents_for_database(
    registry: &ProjectTenantRegistry,
    engine: &Arc<Engine>,
    isolation: &TenantIsolationContext,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    parent_document_path: Option<&DocumentPath>,
    mut structured_query: StructuredQuery,
    transaction: Option<&[u8]>,
) -> Result<RunQueryOutcome> {
    let tenant_id = tenant_id_for_context_database(
        registry,
        isolation,
        database,
        "Firestore run-query database",
    )?;
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
            Some(transaction_token) => engine
                .query_collection_group_documents_structured_in_transaction(
                    tenant_id,
                    transaction_token,
                    principal,
                    &collection_target.collection_group,
                    parent_document_path,
                    &structured_query,
                )?,
            None => engine.query_collection_group_documents_structured_with_principal_cancellable(
                tenant_id,
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
            Some(transaction_token) => engine.query_documents_structured_in_transaction(
                tenant_id,
                transaction_token,
                principal,
                &table,
                &structured_query,
            )?,
            None => engine.query_documents_structured_with_principal(
                tenant_id,
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
        read_time: SystemWallClock.now(),
        skipped_results,
    })
}

pub fn run_aggregation_query_for_database(
    registry: &ProjectTenantRegistry,
    engine: &Arc<Engine>,
    isolation: &TenantIsolationContext,
    database: &resource_names::FirestoreDatabaseName,
    principal: &PrincipalContext,
    parent_document_path: Option<&DocumentPath>,
    mut aggregation_query: StructuredAggregationQuery,
) -> Result<RunAggregationQueryOutcome> {
    let tenant_id = tenant_id_for_context_database(
        registry,
        isolation,
        database,
        "Firestore run-aggregation database",
    )?;
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
        engine.aggregate_collection_group_documents_structured_with_principal_cancellable(
            tenant_id,
            &collection_target.collection_group,
            parent_document_path,
            &aggregation_query,
            principal,
            &mut || Ok(()),
        )?
    } else {
        let table = storage_table_for_collection_path(&collection_target.collection_path)?;
        engine.aggregate_documents_structured_with_principal_cancellable(
            tenant_id,
            &table,
            &aggregation_query,
            principal,
            &mut || Ok(()),
        )?
    };

    Ok(RunAggregationQueryOutcome {
        result,
        read_time: SystemWallClock.now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_tenant_registry::ProjectTenantRegistry;
    use nimbus_core::parse_document_path;
    use serde_json::json;

    fn database(project_id: &str) -> resource_names::FirestoreDatabaseName {
        resource_names::FirestoreDatabaseName {
            project_id: project_id.to_string(),
        }
    }

    /// A principal *verified* to hold a Firebase project: the issuer
    /// (`securetoken.google.com/<project>`) is recorded under `verified_claims`,
    /// exactly as `nimbus_auth::normalize_principal_context` records a verified
    /// `VerifiedUserIdentity::issuer`.
    fn verified_principal_for_project(project_id: &str) -> PrincipalContext {
        PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::new(),
            verified_claims: serde_json::Map::from_iter([(
                "issuer".to_string(),
                json!(format!("https://securetoken.google.com/{project_id}")),
            )]),
        }
    }

    /// proj-x and proj-y both belong to tenant-1 (many-projects-per-tenant);
    /// proj-z belongs to tenant-2.
    fn registry() -> ProjectTenantRegistry {
        ProjectTenantRegistry::new()
            .bind("proj-x", TenantId::new("tenant-1").expect("tenant id"))
            .bind("proj-y", TenantId::new("tenant-1").expect("tenant id"))
            .bind("proj-z", TenantId::new("tenant-2").expect("tenant id"))
    }

    // ---- Acceptance case 1: anonymous / no-token -> REFUSED (was admitted to any project) ----
    #[test]
    fn anonymous_principal_is_refused_admission_to_any_project() {
        let registry = registry();
        let error = tenant_context_for_database(
            &registry,
            &database("proj-x"),
            &PrincipalContext::anonymous(),
            "test",
        )
        .expect_err("anonymous principal must be refused admission");
        assert!(matches!(error, Error::PermissionDenied(_)), "got {error:?}");
    }

    /// An authenticated principal with no *verified* Firebase project (e.g. a
    /// non-Firebase / Auth0 token) is refused too — the issuer must verify to a
    /// Firebase project, and an issuer stuffed into unverified claims is ignored.
    #[test]
    fn authenticated_without_verified_firebase_project_is_refused() {
        let registry = registry();
        let non_firebase = PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::new(),
            verified_claims: serde_json::Map::from_iter([(
                "issuer".to_string(),
                json!("https://acme.auth0.com/"),
            )]),
        };
        assert!(matches!(
            tenant_context_for_database(&registry, &database("proj-x"), &non_firebase, "test"),
            Err(Error::PermissionDenied(_))
        ));

        let spoofed_unverified = PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::from_iter([(
                "issuer".to_string(),
                json!("https://securetoken.google.com/proj-x"),
            )]),
            verified_claims: serde_json::Map::new(),
        };
        assert!(
            matches!(
                tenant_context_for_database(
                    &registry,
                    &database("proj-x"),
                    &spoofed_unverified,
                    "test"
                ),
                Err(Error::PermissionDenied(_))
            ),
            "an issuer in unverified claims must not select a project"
        );
    }

    // ---- Acceptance case 2: token for X, URL Y in a DIFFERENT tenant -> REFUSED ----
    #[test]
    fn verified_token_cross_tenant_project_is_refused() {
        let registry = registry();
        let principal = verified_principal_for_project("proj-x"); // tenant-1
        let error = tenant_context_for_database(&registry, &database("proj-z"), &principal, "test")
            .expect_err("cross-tenant project selection must be refused");
        match error {
            Error::PermissionDenied(message) => {
                assert!(message.contains("proj-x"), "names token project: {message}");
                assert!(
                    message.contains("proj-z"),
                    "names target project: {message}"
                );
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }

    /// An unregistered project is refused (strict registry; no verbatim
    /// project→tenant fallthrough) whether named by the URL or by the token.
    #[test]
    fn unregistered_project_is_refused() {
        let registry = registry();
        assert!(matches!(
            tenant_context_for_database(
                &registry,
                &database("proj-unknown"),
                &verified_principal_for_project("proj-x"),
                "test"
            ),
            Err(Error::PermissionDenied(_))
        ));
        assert!(matches!(
            tenant_context_for_database(
                &registry,
                &database("proj-x"),
                &verified_principal_for_project("proj-unknown"),
                "test"
            ),
            Err(Error::PermissionDenied(_))
        ));
    }

    // ---- Acceptance case 4: token for X, URL X -> ADMITTED ----
    #[test]
    fn verified_token_same_project_is_admitted() {
        let registry = registry();
        let principal = verified_principal_for_project("proj-x");
        let context =
            tenant_context_for_database(&registry, &database("proj-x"), &principal, "test")
                .expect("matching project must be admitted");
        assert_eq!(context.tenant_id().as_str(), "tenant-1");
    }

    // ---- Acceptance case 3: token for X, URL Y in the SAME tenant -> ADMITTED + sees data ----
    #[test]
    fn verified_token_same_tenant_other_project_is_admitted_and_sees_data() {
        let temp = tempfile::tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
        let registry = registry();
        let tenant_1 = TenantId::new("tenant-1").expect("tenant id");
        engine
            .create_tenant(tenant_1.clone())
            .expect("create tenant");

        // Seed a document under tenant-1 (data that, in the 1:1 world, "belongs
        // to" proj-x). The storage partition is the tenant, not the project.
        let doc_path = parse_document_path("cities/sf", "test path").expect("doc path");
        let locator = locator_for_document_path(&doc_path).expect("locator");
        engine
            .insert_document_with_id(
                &tenant_1,
                locator.table.clone(),
                locator.id.clone(),
                serde_json::Map::from_iter([("name".to_string(), json!("San Francisco"))]),
            )
            .expect("seed insert should succeed");

        // A token verified for proj-x, addressing proj-y's URL (same tenant), is
        // admitted and reaches tenant-1's data — proving many-projects-per-tenant
        // is non-vacuous (not a blanket refuse).
        let principal = verified_principal_for_project("proj-x");
        let isolation =
            tenant_context_for_database(&registry, &database("proj-y"), &principal, "test")
                .expect("same-tenant cross-project access must be admitted");
        assert_eq!(isolation.tenant_id().as_str(), "tenant-1");

        let document = get_document_for_database(
            &registry,
            &engine,
            &isolation,
            &database("proj-y"),
            &principal,
            &doc_path,
            None,
        )
        .expect("read through the admitted path must succeed")
        .expect("document seeded under the shared tenant must be visible");
        assert_eq!(document.get_field("name"), Some(&json!("San Francisco")));
    }

    /// Defense-in-depth: the per-op re-check refuses a data op whose database
    /// resolves to a different tenant than the bound context — and it maps the
    /// project only through the registry, never a verbatim `TenantId::new`.
    #[test]
    fn per_op_recheck_refuses_database_in_other_tenant() {
        let registry = registry();
        let principal = verified_principal_for_project("proj-x");
        let isolation =
            tenant_context_for_database(&registry, &database("proj-x"), &principal, "test")
                .expect("binds tenant-1");
        // A *registered* project in a different tenant (proj-z -> tenant-2) is
        // refused by the context match: the registry resolves it, then
        // `ensure_tenant_matches` rejects tenant-2 against the bound tenant-1.
        let error = tenant_id_for_context_database(
            &registry,
            &isolation,
            &database("proj-z"),
            "Firestore test database",
        )
        .expect_err("a per-op database in another tenant must be refused");
        assert!(
            error.to_string().contains("tenant-1") && error.to_string().contains("tenant-2"),
            "the refusal should name both the bound and the referenced tenant: {error}"
        );

        // An *unregistered* per-op database is refused by the registry itself
        // (PermissionDenied) — there is no verbatim project->tenant fallthrough.
        assert!(matches!(
            tenant_id_for_context_database(
                &registry,
                &isolation,
                &database("proj-unknown"),
                "Firestore test database",
            ),
            Err(Error::PermissionDenied(_))
        ));
    }
}
