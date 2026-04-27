use neovex_core::{Document, DocumentId, DocumentLocator, Error, Result, TableName};
use neovex_runtime::{
    HostCallCancellation, HostCallPayload, NeovexRuntimeError, RuntimeAsyncDbDeletePayload,
    RuntimeAsyncDbGetPayload, RuntimeAsyncDbInsertPayload, RuntimeAsyncDbPatchPayload,
};
use serde_json::{Map, Value};

use crate::runtime_host::capabilities::{
    RuntimeCapabilityHost, delete_document, delete_document_async, get_document,
    get_document_async, insert_document, insert_document_async, update_document,
    update_document_async, validate_runtime_capability_access,
};
use crate::runtime_host::responses::encode_runtime_core_result;

pub(crate) fn dispatch_document_host_call<H>(
    host: &H,
    payload: HostCallPayload,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    match payload {
        HostCallPayload::DocumentGet(payload) => invoke_document_get(host, payload),
        HostCallPayload::DocumentInsert(payload) => invoke_document_insert(host, payload),
        HostCallPayload::DocumentPatch(payload) => invoke_document_patch(host, payload),
        HostCallPayload::DocumentDelete(payload) => invoke_document_delete(host, payload),
        _ => unreachable!("non-document host operation routed to document host dispatcher"),
    }
}

pub(crate) fn dispatch_document_host_call_cancellable<H>(
    host: &H,
    payload: HostCallPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    match payload {
        HostCallPayload::DocumentGet(payload) => {
            invoke_document_get_cancellable(host, payload, cancellation)
        }
        HostCallPayload::DocumentInsert(payload) => {
            invoke_document_insert_cancellable(host, payload, cancellation)
        }
        HostCallPayload::DocumentPatch(payload) => {
            invoke_document_patch_cancellable(host, payload, cancellation)
        }
        HostCallPayload::DocumentDelete(payload) => {
            invoke_document_delete_cancellable(host, payload, cancellation)
        }
        _ => unreachable!("non-document host operation routed to document host dispatcher"),
    }
}

pub(crate) async fn dispatch_document_host_call_async<H>(
    host: &H,
    payload: HostCallPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    match payload {
        HostCallPayload::DocumentGet(payload) => {
            invoke_document_get_async_cancellable(host, payload, cancellation).await
        }
        HostCallPayload::DocumentInsert(payload) => {
            invoke_document_insert_async_cancellable(host, payload, cancellation).await
        }
        HostCallPayload::DocumentPatch(payload) => {
            invoke_document_patch_async_cancellable(host, payload, cancellation).await
        }
        HostCallPayload::DocumentDelete(payload) => {
            invoke_document_delete_async_cancellable(host, payload, cancellation).await
        }
        _ => unreachable!("non-document host operation routed to document host dispatcher"),
    }
}

fn invoke_document_get<H>(
    host: &H,
    payload: RuntimeAsyncDbGetPayload,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    let cancellation = HostCallCancellation::default();
    invoke_document_get_cancellable(host, payload, &cancellation)
}

fn invoke_document_get_cancellable<H>(
    host: &H,
    payload: RuntimeAsyncDbGetPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    let locator = match resolve_runtime_input(runtime_document_locator(&payload.table, &payload.id))
    {
        Ok(locator) => locator,
        Err(response) => return response,
    };
    encode_runtime_core_result(
        get_document(host, &locator)
            .map(|document| document.map_or(Value::Null, Document::into_json)),
    )
}

async fn invoke_document_get_async_cancellable<H>(
    host: &H,
    payload: RuntimeAsyncDbGetPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    let locator = match resolve_runtime_input(runtime_document_locator(&payload.table, &payload.id))
    {
        Ok(locator) => locator,
        Err(response) => return response,
    };
    encode_runtime_core_result(
        get_document_async(host, &locator, cancellation)
            .await
            .map(|document| document.map_or(Value::Null, Document::into_json)),
    )
}

fn invoke_document_insert<H>(
    host: &H,
    payload: RuntimeAsyncDbInsertPayload,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    let cancellation = HostCallCancellation::default();
    invoke_document_insert_cancellable(host, payload, &cancellation)
}

fn invoke_document_insert_cancellable<H>(
    host: &H,
    payload: RuntimeAsyncDbInsertPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    let table = match resolve_runtime_input(TableName::new(payload.table)) {
        Ok(table) => table,
        Err(response) => return response,
    };
    let fields = match resolve_runtime_input(json_object(payload.fields, "ctx.db.insert() fields"))
    {
        Ok(fields) => fields,
        Err(response) => return response,
    };
    encode_runtime_core_result(
        insert_document(host, table, fields).map(|id| Value::String(id.to_string())),
    )
}

async fn invoke_document_insert_async_cancellable<H>(
    host: &H,
    payload: RuntimeAsyncDbInsertPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    let table = match resolve_runtime_input(TableName::new(payload.table)) {
        Ok(table) => table,
        Err(response) => return response,
    };
    let fields = match resolve_runtime_input(json_object(payload.fields, "ctx.db.insert() fields"))
    {
        Ok(fields) => fields,
        Err(response) => return response,
    };
    encode_runtime_core_result(
        insert_document_async(host, table, fields, cancellation)
            .await
            .map(|id| Value::String(id.to_string())),
    )
}

fn invoke_document_patch<H>(
    host: &H,
    payload: RuntimeAsyncDbPatchPayload,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    let cancellation = HostCallCancellation::default();
    invoke_document_patch_cancellable(host, payload, &cancellation)
}

fn invoke_document_patch_cancellable<H>(
    host: &H,
    payload: RuntimeAsyncDbPatchPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    let table = match resolve_runtime_input(TableName::new(payload.table)) {
        Ok(table) => table,
        Err(response) => return response,
    };
    let id = match resolve_runtime_input(DocumentId::from_key(payload.id)) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let patch = match resolve_runtime_input(json_object(payload.patch, "ctx.db.patch() patch")) {
        Ok(patch) => patch,
        Err(response) => return response,
    };
    encode_runtime_core_result(
        update_document(host, table, id, patch).map(|id| Value::String(id.to_string())),
    )
}

async fn invoke_document_patch_async_cancellable<H>(
    host: &H,
    payload: RuntimeAsyncDbPatchPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    let table = match resolve_runtime_input(TableName::new(payload.table)) {
        Ok(table) => table,
        Err(response) => return response,
    };
    let id = match resolve_runtime_input(DocumentId::from_key(payload.id)) {
        Ok(id) => id,
        Err(response) => return response,
    };
    let patch = match resolve_runtime_input(json_object(payload.patch, "ctx.db.patch() patch")) {
        Ok(patch) => patch,
        Err(response) => return response,
    };
    encode_runtime_core_result(
        update_document_async(host, table, id, patch, cancellation)
            .await
            .map(|id| Value::String(id.to_string())),
    )
}

fn invoke_document_delete<H>(
    host: &H,
    payload: RuntimeAsyncDbDeletePayload,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    let cancellation = HostCallCancellation::default();
    invoke_document_delete_cancellable(host, payload, &cancellation)
}

fn invoke_document_delete_cancellable<H>(
    host: &H,
    payload: RuntimeAsyncDbDeletePayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    let table = match resolve_runtime_input(TableName::new(payload.table)) {
        Ok(table) => table,
        Err(response) => return response,
    };
    let id = match resolve_runtime_input(DocumentId::from_key(payload.id)) {
        Ok(id) => id,
        Err(response) => return response,
    };
    encode_runtime_core_result(delete_document(host, table, id).map(|_| Value::Null))
}

async fn invoke_document_delete_async_cancellable<H>(
    host: &H,
    payload: RuntimeAsyncDbDeletePayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    validate_runtime_capability_access(host, payload.session_id.as_deref(), cancellation)?;
    let table = match resolve_runtime_input(TableName::new(payload.table)) {
        Ok(table) => table,
        Err(response) => return response,
    };
    let id = match resolve_runtime_input(DocumentId::from_key(payload.id)) {
        Ok(id) => id,
        Err(response) => return response,
    };
    encode_runtime_core_result(
        delete_document_async(host, table, id, cancellation)
            .await
            .map(|_| Value::Null),
    )
}

fn runtime_document_locator(table: &str, id: &str) -> Result<DocumentLocator> {
    Ok(DocumentLocator::new(
        TableName::new(table.to_string())?,
        DocumentId::from_key(id.to_string())?,
    ))
}

fn json_object(value: Value, label: &str) -> Result<Map<String, Value>> {
    match value {
        Value::Object(object) => Ok(object),
        _ => Err(Error::InvalidInput(format!(
            "{label} must be a plain JSON object"
        ))),
    }
}

fn resolve_runtime_input<T>(
    result: Result<T>,
) -> std::result::Result<T, std::result::Result<Value, NeovexRuntimeError>> {
    match result {
        Ok(value) => Ok(value),
        Err(error) => Err(encode_runtime_core_result(Err(error))),
    }
}
