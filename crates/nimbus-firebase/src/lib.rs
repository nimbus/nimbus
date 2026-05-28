pub mod batch_get_request;
pub mod batch_write_request;
pub mod commit_request;
pub mod errors;
pub mod firestore_model;
pub mod grpc;
pub mod list_collection_ids_request;
pub mod operations;
pub mod resource_names;
pub mod response;
pub mod run_aggregation_query_request;
pub mod run_query_request;
pub mod serializer;
pub mod transaction_request;

pub use errors::{
    batch_get_request_error_to_core, batch_write_request_error_to_core,
    commit_request_error_to_core, firebase_error_response_json, firestore_google_rpc_status_json,
    firestore_grpc_code, list_collection_ids_request_error_to_core, resource_name_error_to_core,
    run_aggregation_query_request_error_to_core, run_query_request_error_to_core,
    transaction_request_error_to_core,
};
pub use firestore_model::{
    locator_for_document_path, parse_document_path, storage_table_for_collection_path,
    validate_default_database_id,
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
pub use response::{
    batch_get_entry_json, batch_write_response_json, commit_response_json, firestore_document_name,
    format_timestamp, run_aggregation_query_response_entries, run_query_response_entries,
    serialize_json_lines,
};

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct FirebaseConfig {
    allow_emulator_mock_user_token_auth: bool,
}

impl FirebaseConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_emulator_mock_user_token_auth(mut self) -> Self {
        self.allow_emulator_mock_user_token_auth = true;
        self
    }

    pub fn allows_emulator_mock_user_token_auth(&self) -> bool {
        self.allow_emulator_mock_user_token_auth
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
