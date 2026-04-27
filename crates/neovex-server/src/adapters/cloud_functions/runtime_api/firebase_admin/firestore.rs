use neovex_core::{
    AtomicWrite, AtomicWriteBatch, Document, DocumentPath, Error, ResourcePathBinding, Result,
    WriteKey, WritePrecondition, WriteSetMode,
};
use neovex_runtime::{HostCallCancellation, NeovexRuntimeError};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::provider_family::firestore::{
    locator_for_document_path, parse_document_path, validate_default_database_id,
};
use crate::runtime_host::capabilities::{
    RuntimeCapabilityHost, execute_atomic_write_batch, execute_atomic_write_batch_async,
    get_document, get_document_async, validate_runtime_capability_access,
};
use crate::runtime_host::responses::encode_runtime_core_result;

const FIRESTORE_ADMIN_GET_DOCUMENT_OPERATION: &str = "firebase_admin.firestore.get_document";
const FIRESTORE_ADMIN_SET_DOCUMENT_OPERATION: &str = "firebase_admin.firestore.set_document";
const FIRESTORE_ADMIN_UPDATE_DOCUMENT_OPERATION: &str = "firebase_admin.firestore.update_document";
const FIRESTORE_ADMIN_DELETE_DOCUMENT_OPERATION: &str = "firebase_admin.firestore.delete_document";

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct FirestoreAdminGetDocumentPayload {
    pub(crate) database_id: String,
    pub(crate) document_path: String,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct FirestoreAdminSetDocumentPayload {
    pub(crate) database_id: String,
    pub(crate) document_path: String,
    pub(crate) fields: Value,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct FirestoreAdminUpdateDocumentPayload {
    pub(crate) database_id: String,
    pub(crate) document_path: String,
    pub(crate) patch: Value,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct FirestoreAdminDeleteDocumentPayload {
    pub(crate) database_id: String,
    pub(crate) document_path: String,
    #[serde(default)]
    pub(crate) session_id: Option<String>,
}

fn decode_extension_payload<T>(
    operation: &str,
    payload: Value,
) -> std::result::Result<T, NeovexRuntimeError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value(payload).map_err(|error| {
        NeovexRuntimeError::Contract(format!(
            "invalid cloud functions runtime extension payload for `{operation}`: {error}"
        ))
    })
}

fn unsupported_firestore_admin_operation(
    operation: &str,
) -> std::result::Result<Value, NeovexRuntimeError> {
    Err(NeovexRuntimeError::Contract(format!(
        "cloud functions runtime does not support firestore admin operation `{operation}`"
    )))
}

pub(crate) fn dispatch_firestore_admin_runtime_extension<H>(
    host: &H,
    operation: &str,
    payload: Value,
) -> std::result::Result<Value, NeovexRuntimeError>
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

pub(crate) fn dispatch_firestore_admin_runtime_extension_cancellable<H>(
    host: &H,
    operation: &str,
    payload: Value,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
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

pub(crate) async fn dispatch_firestore_admin_runtime_extension_async<H>(
    host: &H,
    operation: &str,
    payload: Value,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
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
) -> std::result::Result<Value, NeovexRuntimeError>
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
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
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
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
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
) -> std::result::Result<Value, NeovexRuntimeError>
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
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    firebase_admin_firestore_write_result(
        firebase_admin_set_batch(&payload.database_id, &payload.document_path, payload.fields)
            .and_then(|batch| execute_atomic_write_batch(host, batch)),
    )
}

async fn invoke_firebase_admin_firestore_set_document_async_cancellable<H>(
    host: &H,
    payload: FirestoreAdminSetDocumentPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
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
) -> std::result::Result<Value, NeovexRuntimeError>
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
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    firebase_admin_firestore_write_result(
        firebase_admin_update_batch(&payload.database_id, &payload.document_path, payload.patch)
            .and_then(|batch| execute_atomic_write_batch(host, batch)),
    )
}

async fn invoke_firebase_admin_firestore_update_document_async_cancellable<H>(
    host: &H,
    payload: FirestoreAdminUpdateDocumentPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
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
) -> std::result::Result<Value, NeovexRuntimeError>
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
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    firebase_admin_firestore_write_result(
        firebase_admin_delete_batch(&payload.database_id, &payload.document_path)
            .and_then(|batch| execute_atomic_write_batch(host, batch)),
    )
}

async fn invoke_firebase_admin_firestore_delete_document_async_cancellable<H>(
    host: &H,
    payload: FirestoreAdminDeleteDocumentPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    let batch = match firebase_admin_delete_batch(&payload.database_id, &payload.document_path) {
        Ok(batch) => batch,
        Err(error) => return encode_runtime_core_result(Err(error)),
    };
    firebase_admin_firestore_write_result(
        execute_atomic_write_batch_async(host, batch, cancellation).await,
    )
}

fn firebase_admin_firestore_write_result(
    result: Result<neovex_core::AtomicWriteBatchOutcome>,
) -> std::result::Result<Value, NeovexRuntimeError> {
    encode_runtime_core_result(result.map(firebase_admin_write_result_value))
}

fn firebase_admin_document_path(path: &str) -> Result<DocumentPath> {
    parse_document_path(path, "firebase-admin/firestore document path")
}

fn firebase_admin_bound_key(database_id: &str, document_path: &str) -> Result<WriteKey> {
    validate_default_database_id(database_id, "firebase-admin/firestore database id")?;
    let (document_path, locator) = firebase_admin_resolve_document_target(document_path)?;
    Ok(WriteKey::from(ResourcePathBinding::new(
        locator,
        document_path,
    )))
}

fn firebase_admin_resolve_document_target(
    document_path: &str,
) -> Result<(DocumentPath, neovex_core::DocumentLocator)> {
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

fn firebase_admin_write_result_value(outcome: neovex_core::AtomicWriteBatchOutcome) -> Value {
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
