pub mod batch_get_request;
pub mod batch_write_request;
pub mod commit_request;
pub mod errors;
pub mod grpc;
pub mod list_collection_ids_request;
pub mod operations;
pub mod project_tenant_registry;
pub mod resource_names;
pub mod response;
pub mod run_aggregation_query_request;
pub mod run_query_request;
pub mod serializer;
pub mod transaction_request;
mod transaction_token;

pub use errors::{
    batch_get_request_error_to_core, batch_write_request_error_to_core,
    commit_request_error_to_core, firebase_error_response_json, firestore_google_rpc_status_json,
    firestore_grpc_code, list_collection_ids_request_error_to_core, resource_name_error_to_core,
    run_aggregation_query_request_error_to_core, run_query_request_error_to_core,
    transaction_request_error_to_core,
};
use nimbus_core::{
    AtomicWriteResult, Document, DocumentPath, Error, StructuredAggregationResult, Timestamp,
};
pub use operations::{
    batch_get_documents_for_database, batch_write_for_database,
    begin_transaction_session_for_database, commit_batch_for_database, get_document_for_database,
    list_collection_ids_for_database, resolve_write_key, rollback_transaction_session_for_database,
    run_aggregation_query_for_database, run_query_documents_for_database,
    tenant_context_for_database,
};
pub use project_tenant_registry::{
    ProjectSpecError, ProjectTenantRegistry, firebase_project_from_issuer,
    firebase_project_from_verified_principal,
};
pub use response::{
    batch_get_entry_json, batch_write_response_json, commit_response_json, firestore_document_name,
    format_timestamp, run_aggregation_query_response_entries, run_query_response_entries,
    serialize_json_lines,
};

/// Firestore adapter configuration.
///
/// Carries the project→tenant [`ProjectTenantRegistry`] that decides which tenant
/// a request reaches (#24), plus the **dev-mode token-verification bypass**
/// opt-in.
///
/// The registry defaults to an empty *strict* registry, so a `FirebaseConfig`
/// built without explicit project bindings **refuses all Firestore traffic**
/// (every project is unregistered) — fail-closed by construction. An operator
/// turns the adapter on by registering projects (`NIMBUS_FIREBASE_PROJECTS`), not
/// by leaving it unconfigured.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct FirebaseConfig {
    /// Dev-mode token-verification bypass: when set, the Firebase Emulator path
    /// fabricates a *verified* principal from an unsigned, unverified emulator
    /// token (see
    /// `nimbus_auth::firebase_emulator_verification_bypass_principal_from_bearer`).
    /// Because that forges a verified Firebase project from caller-controlled
    /// claims, it is refused on any non-loopback bind by the `nimbus-bin` boot
    /// guard. Never enable it on a network-reachable listener.
    allow_emulator_token_verification_bypass: bool,
    /// Project→tenant bindings. Empty strict by default = refuse-all (fail-closed).
    project_registry: project_tenant_registry::ProjectTenantRegistry,
}

impl FirebaseConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable the dev-mode token-verification bypass (Firebase Emulator).
    ///
    /// DANGER: fabricates a *verified* project from an unverified token. Only
    /// valid on a loopback bind — the `nimbus-bin` boot guard refuses it on any
    /// non-loopback host. Pairs with an identity registry for zero-config local
    /// dev.
    pub fn with_emulator_token_verification_bypass(mut self) -> Self {
        self.allow_emulator_token_verification_bypass = true;
        self
    }

    pub fn allows_emulator_token_verification_bypass(&self) -> bool {
        self.allow_emulator_token_verification_bypass
    }

    /// Install the project→tenant registry. Builder style.
    pub fn with_project_registry(
        mut self,
        registry: project_tenant_registry::ProjectTenantRegistry,
    ) -> Self {
        self.project_registry = registry;
        self
    }

    /// The project→tenant registry this adapter resolves requests through.
    pub fn project_registry(&self) -> &project_tenant_registry::ProjectTenantRegistry {
        &self.project_registry
    }
}

#[derive(Debug, Clone)]
pub struct BatchGetDocumentsOutcome {
    pub entries: Vec<BatchGetDocumentEntry>,
    pub read_time: Timestamp,
}

pub struct BatchWriteOutcome {
    pub entries: Vec<BatchWriteEntryOutcome>,
}

pub struct BatchWriteEntryOutcome {
    pub write_result: Option<AtomicWriteResult>,
    pub error: Option<Error>,
}

#[derive(Debug, Clone)]
pub struct BatchGetDocumentEntry {
    pub document_name: String,
    pub document: Option<Document>,
}

#[derive(Debug, Clone)]
pub struct RunQueryOutcome {
    pub documents: Vec<RunQueryDocument>,
    pub read_time: Timestamp,
    pub skipped_results: usize,
}

#[derive(Debug, Clone)]
pub struct RunQueryDocument {
    pub document_path: DocumentPath,
    pub document: Document,
}

#[derive(Debug, Clone)]
pub struct RunAggregationQueryOutcome {
    pub result: StructuredAggregationResult,
    pub read_time: Timestamp,
}
