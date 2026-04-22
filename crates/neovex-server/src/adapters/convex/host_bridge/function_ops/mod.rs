use super::*;
use crate::execution::invocations::{
    RuntimeBundleInvocationOptions, invoke_runtime_bundle_blocking_with_host,
    invoke_runtime_bundle_on_worker_with_host,
};

mod ctx_ops;
mod http_route;
mod nested_runtime;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn dispatch_function_host_call(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::HttpRoute => self.invoke_http_route(payload),
            ConvexHostCallOperation::CtxQuery => self.invoke_ctx_query(payload),
            ConvexHostCallOperation::CtxPaginatedQuery => self.invoke_ctx_paginated_query(payload),
            ConvexHostCallOperation::CtxMutation => self.invoke_ctx_mutation(payload),
            ConvexHostCallOperation::CtxAction => self.invoke_ctx_action(payload),
            ConvexHostCallOperation::CtxRunQuery => self.invoke_ctx_run_query(payload),
            ConvexHostCallOperation::CtxRunMutation => self.invoke_ctx_run_mutation(payload),
            ConvexHostCallOperation::CtxRunAction => self.invoke_ctx_run_action(payload),
            ConvexHostCallOperation::CtxServiceLookup => self.invoke_ctx_service_lookup(payload),
            ConvexHostCallOperation::CtxRuntimeEnterNestedCall => {
                self.invoke_ctx_runtime_enter_nested_call(payload)
            }
            _ => unreachable!("non-function host operation routed to function dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_function_host_call_cancellable(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::HttpRoute => {
                self.invoke_http_route_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxQuery => {
                self.invoke_ctx_query_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxPaginatedQuery => {
                self.invoke_ctx_paginated_query_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxMutation => {
                self.invoke_ctx_mutation_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxAction => {
                self.invoke_ctx_action_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxRunQuery => {
                self.invoke_ctx_run_query_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxRunMutation => {
                self.invoke_ctx_run_mutation_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxRunAction => {
                self.invoke_ctx_run_action_cancellable(payload, cancellation)
            }
            ConvexHostCallOperation::CtxServiceLookup
            | ConvexHostCallOperation::CtxRuntimeEnterNestedCall => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.dispatch_function_host_call(operation, payload)
            }
            _ => unreachable!("non-function host operation routed to function dispatcher"),
        }
    }

    pub(in crate::adapters::convex) async fn dispatch_function_host_call_async(
        &self,
        operation: ConvexHostCallOperation,
        payload: Value,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match operation {
            ConvexHostCallOperation::HttpRoute => {
                self.invoke_http_route_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxQuery => {
                self.invoke_ctx_query_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxPaginatedQuery => {
                self.invoke_ctx_paginated_query_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxMutation => {
                self.invoke_ctx_mutation_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxAction => {
                self.invoke_ctx_action_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxRunQuery => {
                self.invoke_ctx_run_query_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxRunMutation => {
                self.invoke_ctx_run_mutation_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxRunAction => {
                self.invoke_ctx_run_action_async_cancellable(payload, cancellation)
                    .await
            }
            ConvexHostCallOperation::CtxServiceLookup
            | ConvexHostCallOperation::CtxRuntimeEnterNestedCall => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.dispatch_function_host_call(operation, payload)
            }
            _ => unreachable!("non-function host operation routed to function dispatcher"),
        }
    }
}
