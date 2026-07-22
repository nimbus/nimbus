use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, Document, DocumentPath, Error, ResourcePathBinding, Result,
    WriteKey, WritePrecondition, WriteSetMode,
};
use nimbus_runtime::{HostCallCancellation, NimbusRuntimeError};
use serde::Deserialize;
use serde_json::{Value, json};

use nimbus_bridge::capabilities::{
    RuntimeCapabilityHost, execute_atomic_write_batch, execute_atomic_write_batch_async,
    get_document, get_document_async, validate_runtime_capability_access,
};
use nimbus_bridge::responses::encode_runtime_core_result;
use nimbus_core::{locator_for_document_path, parse_document_path, validate_default_database_id};

const FIRESTORE_ADMIN_GET_DOCUMENT_OPERATION: &str = "firebase_admin.firestore.get_document";
const FIRESTORE_ADMIN_SET_DOCUMENT_OPERATION: &str = "firebase_admin.firestore.set_document";
const FIRESTORE_ADMIN_UPDATE_DOCUMENT_OPERATION: &str = "firebase_admin.firestore.update_document";
const FIRESTORE_ADMIN_DELETE_DOCUMENT_OPERATION: &str = "firebase_admin.firestore.delete_document";

#[derive(Debug, Clone, Deserialize)]
pub struct FirestoreAdminGetDocumentPayload {
    pub database_id: String,
    pub document_path: String,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FirestoreAdminSetDocumentPayload {
    pub database_id: String,
    pub document_path: String,
    pub fields: Value,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FirestoreAdminUpdateDocumentPayload {
    pub database_id: String,
    pub document_path: String,
    pub patch: Value,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FirestoreAdminDeleteDocumentPayload {
    pub database_id: String,
    pub document_path: String,
    #[serde(default)]
    pub host_call_session_id: Option<String>,
}

fn decode_extension_payload<T>(
    operation: &str,
    payload: Value,
) -> std::result::Result<T, NimbusRuntimeError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(payload).map_err(|error| {
        NimbusRuntimeError::Contract(format!(
            "invalid cloud functions runtime extension payload for `{operation}`: {error}"
        ))
    })
}

fn unsupported_firestore_admin_operation(
    operation: &str,
) -> std::result::Result<Value, NimbusRuntimeError> {
    Err(NimbusRuntimeError::Contract(format!(
        "cloud functions runtime does not support firestore admin operation `{operation}`"
    )))
}

pub fn dispatch_firestore_admin_runtime_extension<H>(
    host: &H,
    operation: &str,
    payload: Value,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    match operation {
        FIRESTORE_ADMIN_GET_DOCUMENT_OPERATION => invoke_firebase_admin_firestore_get_document(
            host,
            decode_extension_payload(operation, payload)?,
        ),
        FIRESTORE_ADMIN_SET_DOCUMENT_OPERATION => invoke_firebase_admin_firestore_set_document(
            host,
            decode_extension_payload(operation, payload)?,
        ),
        FIRESTORE_ADMIN_UPDATE_DOCUMENT_OPERATION => {
            invoke_firebase_admin_firestore_update_document(
                host,
                decode_extension_payload(operation, payload)?,
            )
        }
        FIRESTORE_ADMIN_DELETE_DOCUMENT_OPERATION => {
            invoke_firebase_admin_firestore_delete_document(
                host,
                decode_extension_payload(operation, payload)?,
            )
        }
        _ => unsupported_firestore_admin_operation(operation),
    }
}

pub fn dispatch_firestore_admin_runtime_extension_cancellable<H>(
    host: &H,
    operation: &str,
    payload: Value,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    match operation {
        FIRESTORE_ADMIN_GET_DOCUMENT_OPERATION => {
            let payload = decode_extension_payload(operation, payload)?;
            invoke_firebase_admin_firestore_get_document_cancellable(host, payload, cancellation)
        }
        FIRESTORE_ADMIN_SET_DOCUMENT_OPERATION => {
            let payload = decode_extension_payload(operation, payload)?;
            invoke_firebase_admin_firestore_set_document_cancellable(host, payload, cancellation)
        }
        FIRESTORE_ADMIN_UPDATE_DOCUMENT_OPERATION => {
            let payload = decode_extension_payload(operation, payload)?;
            invoke_firebase_admin_firestore_update_document_cancellable(host, payload, cancellation)
        }
        FIRESTORE_ADMIN_DELETE_DOCUMENT_OPERATION => {
            let payload = decode_extension_payload(operation, payload)?;
            invoke_firebase_admin_firestore_delete_document_cancellable(host, payload, cancellation)
        }
        _ => unsupported_firestore_admin_operation(operation),
    }
}

pub async fn dispatch_firestore_admin_runtime_extension_async<H>(
    host: &H,
    operation: &str,
    payload: Value,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    match operation {
        FIRESTORE_ADMIN_GET_DOCUMENT_OPERATION => {
            let payload = decode_extension_payload(operation, payload)?;
            invoke_firebase_admin_firestore_get_document_async_cancellable(
                host,
                payload,
                cancellation,
            )
            .await
        }
        FIRESTORE_ADMIN_SET_DOCUMENT_OPERATION => {
            let payload = decode_extension_payload(operation, payload)?;
            invoke_firebase_admin_firestore_set_document_async_cancellable(
                host,
                payload,
                cancellation,
            )
            .await
        }
        FIRESTORE_ADMIN_UPDATE_DOCUMENT_OPERATION => {
            let payload = decode_extension_payload(operation, payload)?;
            invoke_firebase_admin_firestore_update_document_async_cancellable(
                host,
                payload,
                cancellation,
            )
            .await
        }
        FIRESTORE_ADMIN_DELETE_DOCUMENT_OPERATION => {
            let payload = decode_extension_payload(operation, payload)?;
            invoke_firebase_admin_firestore_delete_document_async_cancellable(
                host,
                payload,
                cancellation,
            )
            .await
        }
        _ => unsupported_firestore_admin_operation(operation),
    }
}

fn invoke_firebase_admin_firestore_get_document<H>(
    host: &H,
    payload: FirestoreAdminGetDocumentPayload,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    let cancellation = HostCallCancellation::default();
    invoke_firebase_admin_firestore_get_document_cancellable(host, payload, &cancellation)
}

fn invoke_firebase_admin_firestore_get_document_cancellable<H>(
    host: &H,
    payload: FirestoreAdminGetDocumentPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(
        host,
        payload.host_call_session_id.as_deref(),
        cancellation,
    )?;
    if let Err(error) = firebase_admin_validate_database_id(&payload.database_id) {
        return encode_runtime_core_result(Err(error));
    }
    let (document_path, locator) =
        match firebase_admin_resolve_document_target(&payload.document_path) {
            Ok(target) => target,
            Err(error) => return encode_runtime_core_result(Err(error)),
        };
    encode_runtime_core_result(
        get_document(host, &locator)
            .map(|document| firebase_admin_document_value(&document_path, document)),
    )
}

async fn invoke_firebase_admin_firestore_get_document_async_cancellable<H>(
    host: &H,
    payload: FirestoreAdminGetDocumentPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(
        host,
        payload.host_call_session_id.as_deref(),
        cancellation,
    )?;
    if let Err(error) = firebase_admin_validate_database_id(&payload.database_id) {
        return encode_runtime_core_result(Err(error));
    }
    let (document_path, locator) =
        match firebase_admin_resolve_document_target(&payload.document_path) {
            Ok(target) => target,
            Err(error) => return encode_runtime_core_result(Err(error)),
        };
    encode_runtime_core_result(
        get_document_async(host, &locator, cancellation)
            .await
            .map(|document| firebase_admin_document_value(&document_path, document)),
    )
}

fn invoke_firebase_admin_firestore_set_document<H>(
    host: &H,
    payload: FirestoreAdminSetDocumentPayload,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    let cancellation = HostCallCancellation::default();
    invoke_firebase_admin_firestore_set_document_cancellable(host, payload, &cancellation)
}

fn invoke_firebase_admin_firestore_set_document_cancellable<H>(
    host: &H,
    payload: FirestoreAdminSetDocumentPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(
        host,
        payload.host_call_session_id.as_deref(),
        cancellation,
    )?;
    firebase_admin_firestore_write_result(
        firebase_admin_set_batch(&payload.database_id, &payload.document_path, payload.fields)
            .and_then(|batch| execute_atomic_write_batch(host, batch)),
    )
}

async fn invoke_firebase_admin_firestore_set_document_async_cancellable<H>(
    host: &H,
    payload: FirestoreAdminSetDocumentPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(
        host,
        payload.host_call_session_id.as_deref(),
        cancellation,
    )?;
    let batch = match firebase_admin_set_batch(
        &payload.database_id,
        &payload.document_path,
        payload.fields,
    ) {
        Ok(batch) => batch,
        Err(error) => return encode_runtime_core_result(Err(error)),
    };
    firebase_admin_firestore_write_result(
        execute_atomic_write_batch_async(host, batch, cancellation).await,
    )
}

fn invoke_firebase_admin_firestore_update_document<H>(
    host: &H,
    payload: FirestoreAdminUpdateDocumentPayload,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    let cancellation = HostCallCancellation::default();
    invoke_firebase_admin_firestore_update_document_cancellable(host, payload, &cancellation)
}

fn invoke_firebase_admin_firestore_update_document_cancellable<H>(
    host: &H,
    payload: FirestoreAdminUpdateDocumentPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(
        host,
        payload.host_call_session_id.as_deref(),
        cancellation,
    )?;
    firebase_admin_firestore_write_result(
        firebase_admin_update_batch(&payload.database_id, &payload.document_path, payload.patch)
            .and_then(|batch| execute_atomic_write_batch(host, batch)),
    )
}

async fn invoke_firebase_admin_firestore_update_document_async_cancellable<H>(
    host: &H,
    payload: FirestoreAdminUpdateDocumentPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(
        host,
        payload.host_call_session_id.as_deref(),
        cancellation,
    )?;
    let batch = match firebase_admin_update_batch(
        &payload.database_id,
        &payload.document_path,
        payload.patch,
    ) {
        Ok(batch) => batch,
        Err(error) => return encode_runtime_core_result(Err(error)),
    };
    firebase_admin_firestore_write_result(
        execute_atomic_write_batch_async(host, batch, cancellation).await,
    )
}

fn invoke_firebase_admin_firestore_delete_document<H>(
    host: &H,
    payload: FirestoreAdminDeleteDocumentPayload,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    let cancellation = HostCallCancellation::default();
    invoke_firebase_admin_firestore_delete_document_cancellable(host, payload, &cancellation)
}

fn invoke_firebase_admin_firestore_delete_document_cancellable<H>(
    host: &H,
    payload: FirestoreAdminDeleteDocumentPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(
        host,
        payload.host_call_session_id.as_deref(),
        cancellation,
    )?;
    firebase_admin_firestore_write_result(
        firebase_admin_delete_batch(&payload.database_id, &payload.document_path)
            .and_then(|batch| execute_atomic_write_batch(host, batch)),
    )
}

async fn invoke_firebase_admin_firestore_delete_document_async_cancellable<H>(
    host: &H,
    payload: FirestoreAdminDeleteDocumentPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(
        host,
        payload.host_call_session_id.as_deref(),
        cancellation,
    )?;
    let batch = match firebase_admin_delete_batch(&payload.database_id, &payload.document_path) {
        Ok(batch) => batch,
        Err(error) => return encode_runtime_core_result(Err(error)),
    };
    firebase_admin_firestore_write_result(
        execute_atomic_write_batch_async(host, batch, cancellation).await,
    )
}

fn firebase_admin_firestore_write_result(
    result: Result<nimbus_core::AtomicWriteBatchOutcome>,
) -> std::result::Result<Value, NimbusRuntimeError> {
    encode_runtime_core_result(result.map(firebase_admin_write_result_value))
}

fn firebase_admin_document_path(path: &str) -> Result<DocumentPath> {
    parse_document_path(path, "firebase-admin/firestore document path")
}

fn firebase_admin_validate_database_id(database_id: &str) -> Result<()> {
    validate_default_database_id(database_id, "firebase-admin/firestore database id")
}

fn firebase_admin_bound_key(database_id: &str, document_path: &str) -> Result<WriteKey> {
    firebase_admin_validate_database_id(database_id)?;
    let (document_path, locator) = firebase_admin_resolve_document_target(document_path)?;
    Ok(WriteKey::from(ResourcePathBinding::new(
        locator,
        document_path,
    )))
}

fn firebase_admin_resolve_document_target(
    document_path: &str,
) -> Result<(DocumentPath, nimbus_core::DocumentLocator)> {
    let document_path = firebase_admin_document_path(document_path)?;
    let locator = locator_for_document_path(&document_path)?;
    Ok((document_path, locator))
}

fn firebase_admin_set_batch(
    database_id: &str,
    document_path: &str,
    fields: Value,
) -> Result<AtomicWriteBatch> {
    let document = json_object(fields, "firebase-admin/firestore set() data")?;
    AtomicWriteBatch::new(vec![AtomicWrite::Set {
        key: firebase_admin_bound_key(database_id, document_path)?,
        document,
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    }])
}

fn firebase_admin_update_batch(
    database_id: &str,
    document_path: &str,
    patch: Value,
) -> Result<AtomicWriteBatch> {
    let field_patch = json_object(patch, "firebase-admin/firestore update() data")?;
    if field_patch.is_empty() {
        return Err(Error::InvalidInput(
            "firebase-admin/firestore update() requires at least one field".to_string(),
        ));
    }
    let mask = field_patch.keys().cloned().collect::<Vec<_>>();
    AtomicWriteBatch::new(vec![AtomicWrite::Patch {
        key: firebase_admin_bound_key(database_id, document_path)?,
        field_patch,
        typed_fields: Default::default(),
        mask,
        precondition: WritePrecondition::exists(true),
        transforms: Vec::new(),
    }])
}

fn firebase_admin_delete_batch(database_id: &str, document_path: &str) -> Result<AtomicWriteBatch> {
    AtomicWriteBatch::new(vec![AtomicWrite::Delete {
        key: firebase_admin_bound_key(database_id, document_path)?,
        precondition: WritePrecondition::default(),
        missing_ok: true,
    }])
}

fn firebase_admin_document_value(
    document_path: &DocumentPath,
    document: Option<Document>,
) -> Value {
    match document {
        Some(document) => json!({
            "path": document_path.to_string(),
            "id": document.id.to_string(),
            "fields": document.fields,
            "create_time_ms": document.creation_time.0,
            "update_time_ms": document.update_time.0,
        }),
        None => Value::Null,
    }
}

fn firebase_admin_write_result_value(outcome: nimbus_core::AtomicWriteBatchOutcome) -> Value {
    let write_time = outcome
        .write_results
        .first()
        .and_then(|result| result.update_time)
        .unwrap_or(outcome.commit_time);
    json!({
        "write_time_ms": write_time.0,
    })
}

fn json_object(value: Value, label: &str) -> Result<serde_json::Map<String, Value>> {
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(Error::InvalidInput(format!(
            "{label} must be a plain JSON object"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    use nimbus_bridge::capabilities::RuntimeCapabilityHost;
    use nimbus_core::{DocumentLocator, PrincipalContext, TenantId};
    use nimbus_engine::{Engine, MutationExecutionUnit};
    use nimbus_runtime::{
        HostCallCancellation, InvocationKind, NimbusRuntimeError, RuntimeLimits, RuntimePolicy,
    };
    use nimbus_tenant::{
        RuntimeIsolationTier, TenantIsolationContext, TenantIsolationMode,
        TenantStorageAccessDecision, admit_runtime_invocation_decision,
    };
    use serde_json::{Value, json};
    use tempfile::TempDir;

    use super::*;

    struct TestHost {
        _data_dir: TempDir,
        engine: Arc<Engine>,
        storage_access: TenantStorageAccessDecision,
        principal: PrincipalContext,
        execution_unit: Arc<MutationExecutionUnit>,
        recorded_reads: Mutex<Vec<DocumentLocator>>,
    }

    impl TestHost {
        fn new() -> Self {
            let data_dir = tempfile::tempdir().expect("engine tempdir should create");
            let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
            let tenant_id = TenantId::new("cloud-functions-test").expect("tenant id should parse");
            engine
                .create_tenant(tenant_id.clone())
                .expect("tenant should create");
            let principal = PrincipalContext::anonymous();
            let context = TenantIsolationContext::application(
                tenant_id.clone(),
                principal.clone(),
                "cloud-functions.firestore-admin-test",
            );
            let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
            let decision = admit_runtime_invocation_decision(
                &context,
                "firestoreAdminTest",
                Some("invoke-firestore-admin-test"),
                &policy,
                RuntimeIsolationTier::InProcessUntrusted,
                TenantIsolationMode::Production,
                Vec::<String>::new(),
            )
            .expect("runtime invocation decision should admit");
            let execution_unit = engine
                .begin_mutation_execution_unit(tenant_id, principal.clone())
                .expect("mutation execution unit should start");

            Self {
                _data_dir: data_dir,
                engine,
                storage_access: decision.storage_access(),
                principal,
                execution_unit,
                recorded_reads: Mutex::new(Vec::new()),
            }
        }

        fn clear_recorded_reads(&self) {
            self.recorded_reads
                .lock()
                .expect("recorded reads should lock")
                .clear();
        }

        fn recorded_read_count(&self) -> usize {
            self.recorded_reads
                .lock()
                .expect("recorded reads should lock")
                .len()
        }
    }

    impl RuntimeCapabilityHost for TestHost {
        fn validate_host_call_session(
            &self,
            _host_call_session_id: Option<&str>,
        ) -> std::result::Result<(), NimbusRuntimeError> {
            Ok(())
        }

        fn mutation_execution_unit(&self) -> Option<&Arc<MutationExecutionUnit>> {
            Some(&self.execution_unit)
        }

        fn invocation_kind(&self) -> InvocationKind {
            // Firebase Admin Firestore calls execute inside the Cloud
            // Functions mutation invocation today.
            InvocationKind::Mutation
        }

        fn engine(&self) -> &Arc<Engine> {
            &self.engine
        }

        fn storage_access(&self) -> &TenantStorageAccessDecision {
            &self.storage_access
        }

        fn principal(&self) -> &PrincipalContext {
            &self.principal
        }

        fn record_document_read(&self, locator: &DocumentLocator) {
            self.recorded_reads
                .lock()
                .expect("recorded reads should lock")
                .push(locator.clone());
        }
    }

    #[test]
    fn firestore_admin_sync_dispatch_round_trips_crud() {
        assert_firestore_admin_crud_round_trip("sync", |host, operation, payload| {
            dispatch_firestore_admin_runtime_extension(host, operation, payload)
        });
    }

    #[test]
    fn firestore_admin_cancellable_dispatch_round_trips_crud() {
        let cancellation = HostCallCancellation::default();
        assert_firestore_admin_crud_round_trip("cancellable", |host, operation, payload| {
            dispatch_firestore_admin_runtime_extension_cancellable(
                host,
                operation,
                payload,
                &cancellation,
            )
        });
    }

    #[test]
    fn firestore_admin_async_dispatch_round_trips_crud() {
        assert_firestore_admin_crud_round_trip("async", |host, operation, payload| {
            let cancellation = HostCallCancellation::default();
            poll_ready(dispatch_firestore_admin_runtime_extension_async(
                host,
                operation,
                payload,
                &cancellation,
            ))
        });
    }

    #[test]
    fn firestore_admin_rejects_non_default_database_id_on_reads_and_writes() {
        let host = TestHost::new();
        let set_response = dispatch_firestore_admin_runtime_extension(
            &host,
            FIRESTORE_ADMIN_SET_DOCUMENT_OPERATION,
            set_payload("(default)", "users/ada", json!({ "name": "Ada" })),
        )
        .expect("default database set should dispatch");
        expect_ok_value(set_response);
        host.clear_recorded_reads();

        let read_response = dispatch_firestore_admin_runtime_extension(
            &host,
            FIRESTORE_ADMIN_GET_DOCUMENT_OPERATION,
            get_payload("analytics", "users/ada"),
        )
        .expect("invalid read should still return an encoded host envelope");
        expect_error_envelope(
            read_response,
            "op.invalid_input",
            "firebase-admin/firestore database id",
        );
        assert_eq!(
            host.recorded_read_count(),
            0,
            "non-default database reads must fail before touching storage"
        );

        let write_response = dispatch_firestore_admin_runtime_extension(
            &host,
            FIRESTORE_ADMIN_SET_DOCUMENT_OPERATION,
            set_payload("analytics", "users/ada", json!({ "name": "Grace" })),
        )
        .expect("invalid write should still return an encoded host envelope");
        expect_error_envelope(
            write_response,
            "op.invalid_input",
            "firebase-admin/firestore database id",
        );
    }

    #[test]
    fn firestore_admin_reports_validation_and_contract_errors() {
        let host = TestHost::new();
        let empty_patch_response = dispatch_firestore_admin_runtime_extension(
            &host,
            FIRESTORE_ADMIN_UPDATE_DOCUMENT_OPERATION,
            update_payload("(default)", "users/ada", json!({})),
        )
        .expect("invalid update should still return an encoded host envelope");
        expect_error_envelope(
            empty_patch_response,
            "op.invalid_input",
            "update() requires at least one field",
        );

        let unsupported = dispatch_firestore_admin_runtime_extension(
            &host,
            "firebase_admin.firestore.list_documents",
            json!({}),
        )
        .expect_err("unsupported operations should return a contract error");
        assert!(
            matches!(unsupported, NimbusRuntimeError::Contract(ref message) if message.contains("does not support firestore admin operation")),
            "unsupported operation should be a named contract error: {unsupported}"
        );
    }

    fn assert_firestore_admin_crud_round_trip(
        suffix: &str,
        mut dispatch: impl FnMut(
            &TestHost,
            &str,
            Value,
        ) -> std::result::Result<Value, NimbusRuntimeError>,
    ) {
        let host = TestHost::new();
        let document_path = format!("users/{suffix}");

        let set_response = dispatch(
            &host,
            FIRESTORE_ADMIN_SET_DOCUMENT_OPERATION,
            set_payload(
                "(default)",
                &document_path,
                json!({ "name": "Ada", "active": true }),
            ),
        )
        .expect("set should dispatch");
        assert!(
            expect_ok_value(set_response)
                .get("write_time_ms")
                .and_then(Value::as_i64)
                .is_some(),
            "set should return a write time"
        );

        let get_response = dispatch(
            &host,
            FIRESTORE_ADMIN_GET_DOCUMENT_OPERATION,
            get_payload("(default)", &document_path),
        )
        .expect("get should dispatch");
        let document = expect_ok_value(get_response);
        assert_eq!(document["path"], document_path);
        assert_eq!(document["id"], suffix);
        assert_eq!(document["fields"]["name"], json!("Ada"));
        assert_eq!(document["fields"]["active"], json!(true));

        let update_response = dispatch(
            &host,
            FIRESTORE_ADMIN_UPDATE_DOCUMENT_OPERATION,
            update_payload(
                "(default)",
                &document_path,
                json!({ "active": false, "score": 42 }),
            ),
        )
        .expect("update should dispatch");
        assert!(
            expect_ok_value(update_response)
                .get("write_time_ms")
                .and_then(Value::as_i64)
                .is_some(),
            "update should return a write time"
        );

        let updated_response = dispatch(
            &host,
            FIRESTORE_ADMIN_GET_DOCUMENT_OPERATION,
            get_payload("(default)", &document_path),
        )
        .expect("updated get should dispatch");
        let updated = expect_ok_value(updated_response);
        assert_eq!(updated["fields"]["name"], json!("Ada"));
        assert_eq!(updated["fields"]["active"], json!(false));
        assert_eq!(updated["fields"]["score"], json!(42));

        let delete_response = dispatch(
            &host,
            FIRESTORE_ADMIN_DELETE_DOCUMENT_OPERATION,
            delete_payload("(default)", &document_path),
        )
        .expect("delete should dispatch");
        assert!(
            expect_ok_value(delete_response)
                .get("write_time_ms")
                .and_then(Value::as_i64)
                .is_some(),
            "delete should return a write time"
        );

        let deleted_response = dispatch(
            &host,
            FIRESTORE_ADMIN_GET_DOCUMENT_OPERATION,
            get_payload("(default)", &document_path),
        )
        .expect("deleted get should dispatch");
        assert_eq!(expect_ok_value(deleted_response), Value::Null);
        assert!(
            host.recorded_read_count() >= 2,
            "successful get calls should record document reads"
        );
    }

    fn get_payload(database_id: &str, document_path: &str) -> Value {
        json!({
            "database_id": database_id,
            "document_path": document_path,
        })
    }

    fn set_payload(database_id: &str, document_path: &str, fields: Value) -> Value {
        json!({
            "database_id": database_id,
            "document_path": document_path,
            "fields": fields,
        })
    }

    fn update_payload(database_id: &str, document_path: &str, patch: Value) -> Value {
        json!({
            "database_id": database_id,
            "document_path": document_path,
            "patch": patch,
        })
    }

    fn delete_payload(database_id: &str, document_path: &str) -> Value {
        json!({
            "database_id": database_id,
            "document_path": document_path,
        })
    }

    fn expect_ok_value(response: Value) -> Value {
        assert_eq!(response["status"], json!("ok"), "response should be ok");
        response
            .get("value")
            .cloned()
            .expect("ok response should include value")
    }

    fn expect_error_envelope(response: Value, code: &str, message_substring: &str) {
        assert_eq!(
            response["status"],
            json!("error"),
            "response should be an encoded error envelope"
        );
        assert_eq!(response["error"]["code"], json!(code));
        let message = response["error"]["message"]
            .as_str()
            .expect("error should include a message");
        assert!(
            message.contains(message_substring),
            "error message should contain `{message_substring}` but was `{message}`"
        );
    }

    fn poll_ready<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = noop_waker();
        let mut context = Context::from_waker(&waker);
        let mut future = pin!(future);
        match Future::poll(future.as_mut(), &mut context) {
            Poll::Ready(output) => output,
            Poll::Pending => panic!("test future should complete without parking"),
        }
    }

    fn noop_waker() -> Waker {
        unsafe fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &NOOP_WAKER_VTABLE)
        }
        unsafe fn wake(_: *const ()) {}
        unsafe fn wake_by_ref(_: *const ()) {}
        unsafe fn drop(_: *const ()) {}

        static NOOP_WAKER_VTABLE: RawWakerVTable =
            RawWakerVTable::new(clone, wake, wake_by_ref, drop);

        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &NOOP_WAKER_VTABLE)) }
    }
}
