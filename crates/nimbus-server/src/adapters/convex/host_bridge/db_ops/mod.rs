use super::*;
use nimbus_runtime::{HostCallPayload, RuntimeAsyncExtensionPayload};

mod dispatch;
mod documents;
mod query_builder;
mod scheduler;

use dispatch::{DocumentHostCall, QueryBuilderHostCall, QueryReadHostCall, SchedulerHostCall};

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn dispatch_query_builder_host_call(
        &self,
        payload: HostCallPayload,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        QueryBuilderHostCall::from_payload(payload)?.dispatch_sync(self)
    }

    pub(in crate::adapters::convex) fn dispatch_query_builder_host_call_cancellable(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        QueryBuilderHostCall::from_payload(payload)?.dispatch_cancellable(self, cancellation)
    }

    pub(in crate::adapters::convex) async fn dispatch_query_builder_host_call_async(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        QueryBuilderHostCall::from_payload(payload)?
            .dispatch_async(self, cancellation)
            .await
    }

    pub(in crate::adapters::convex) fn dispatch_query_read_host_call(
        &self,
        payload: HostCallPayload,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        QueryReadHostCall::from_payload(payload)?.dispatch_sync(self)
    }

    pub(in crate::adapters::convex) fn dispatch_query_read_host_call_cancellable(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        QueryReadHostCall::from_payload(payload)?.dispatch_cancellable(self, cancellation)
    }

    pub(in crate::adapters::convex) async fn dispatch_query_read_host_call_async(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        QueryReadHostCall::from_payload(payload)?
            .dispatch_async(self, cancellation)
            .await
    }

    pub(in crate::adapters::convex) fn dispatch_document_host_call(
        &self,
        payload: HostCallPayload,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        DocumentHostCall::from_payload(payload)?.dispatch_sync(self)
    }

    pub(in crate::adapters::convex) fn dispatch_document_host_call_cancellable(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        DocumentHostCall::from_payload(payload)?.dispatch_cancellable(self, cancellation)
    }

    pub(in crate::adapters::convex) async fn dispatch_document_host_call_async(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        DocumentHostCall::from_payload(payload)?
            .dispatch_async(self, cancellation)
            .await
    }

    pub(in crate::adapters::convex) fn dispatch_adapter_extension_host_call(
        &self,
        payload: RuntimeAsyncExtensionPayload,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        unsupported_adapter_owned_host_call_name(&runtime_extension_operation_name(&payload))
    }

    pub(in crate::adapters::convex) fn dispatch_adapter_extension_host_call_cancellable(
        &self,
        payload: RuntimeAsyncExtensionPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let _ = cancellation;
        unsupported_adapter_owned_host_call_name(&runtime_extension_operation_name(&payload))
    }

    pub(in crate::adapters::convex) async fn dispatch_adapter_extension_host_call_async(
        &self,
        payload: RuntimeAsyncExtensionPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let _ = cancellation;
        unsupported_adapter_owned_host_call_name(&runtime_extension_operation_name(&payload))
    }

    pub(in crate::adapters::convex) fn dispatch_scheduler_host_call(
        &self,
        payload: HostCallPayload,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        SchedulerHostCall::from_payload(payload)?.dispatch_sync(self)
    }

    pub(in crate::adapters::convex) fn dispatch_scheduler_host_call_cancellable(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        SchedulerHostCall::from_payload(payload)?.dispatch_cancellable(self, cancellation)
    }

    pub(in crate::adapters::convex) async fn dispatch_scheduler_host_call_async(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        SchedulerHostCall::from_payload(payload)?
            .dispatch_async(self, cancellation)
            .await
    }
}

fn unsupported_adapter_owned_host_call_name(
    operation_name: &str,
) -> std::result::Result<Value, NimbusRuntimeError> {
    Err(NimbusRuntimeError::Contract(format!(
        "convex host bridge does not own `{}` runtime compatibility; that host call is adapter-owned by cloud_functions",
        operation_name
    )))
}

fn runtime_extension_operation_name(payload: &RuntimeAsyncExtensionPayload) -> String {
    format!("{}.{}", payload.namespace, payload.operation)
}
