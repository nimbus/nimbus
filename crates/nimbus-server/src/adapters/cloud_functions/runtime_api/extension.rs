use nimbus_runtime::{HostCallCancellation, NimbusRuntimeError, RuntimeAsyncExtensionPayload};
use serde_json::Value;

use super::firebase_admin::firestore;
use crate::runtime_host::capabilities::RuntimeCapabilityHost;

const CLOUD_FUNCTIONS_RUNTIME_EXTENSION_NAMESPACE: &str = "cloud_functions";

pub(crate) fn dispatch_runtime_extension_call<H>(
    host: &H,
    payload: RuntimeAsyncExtensionPayload,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if payload.namespace != CLOUD_FUNCTIONS_RUNTIME_EXTENSION_NAMESPACE {
        return unsupported_runtime_extension_namespace(&payload.namespace);
    }
    firestore::dispatch_firestore_admin_runtime_extension(host, &payload.operation, payload.payload)
}

pub(crate) fn dispatch_runtime_extension_call_cancellable<H>(
    host: &H,
    payload: RuntimeAsyncExtensionPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if payload.namespace != CLOUD_FUNCTIONS_RUNTIME_EXTENSION_NAMESPACE {
        return unsupported_runtime_extension_namespace(&payload.namespace);
    }
    firestore::dispatch_firestore_admin_runtime_extension_cancellable(
        host,
        &payload.operation,
        payload.payload,
        cancellation,
    )
}

pub(crate) async fn dispatch_runtime_extension_call_async<H>(
    host: &H,
    payload: RuntimeAsyncExtensionPayload,
    cancellation: &HostCallCancellation,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if payload.namespace != CLOUD_FUNCTIONS_RUNTIME_EXTENSION_NAMESPACE {
        return unsupported_runtime_extension_namespace(&payload.namespace);
    }
    firestore::dispatch_firestore_admin_runtime_extension_async(
        host,
        &payload.operation,
        payload.payload,
        cancellation,
    )
    .await
}

fn unsupported_runtime_extension_namespace(
    namespace: &str,
) -> std::result::Result<Value, NimbusRuntimeError> {
    Err(NimbusRuntimeError::Contract(format!(
        "cloud functions runtime does not support runtime extension namespace `{namespace}`"
    )))
}
