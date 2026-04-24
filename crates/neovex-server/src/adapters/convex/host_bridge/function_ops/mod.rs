use super::*;
use crate::execution::invocations::{
    RuntimeBundleInvocationOptions, invoke_runtime_bundle_blocking_with_host,
    invoke_runtime_bundle_on_worker_with_host,
};
use neovex_runtime::HostCallPayload;

mod ctx_ops;
mod http_route;
mod nested_runtime;

impl ConvexHostBridge {
    pub(in crate::adapters::convex) fn dispatch_function_host_call(
        &self,
        payload: HostCallPayload,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::HttpRoute(payload) => {
                self.invoke_http_route(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxQuery(payload) => {
                self.invoke_ctx_query(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxPaginatedQuery(payload) => {
                self.invoke_ctx_paginated_query(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxMutation(payload) => {
                self.invoke_ctx_mutation(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxAction(payload) => {
                self.invoke_ctx_action(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxRunQuery(payload) => {
                self.invoke_ctx_run_query(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxRunMutation(payload) => {
                self.invoke_ctx_run_mutation(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxRunAction(payload) => {
                self.invoke_ctx_run_action(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxServiceLookup(payload) => {
                self.invoke_ctx_service_lookup(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxRuntimeEnterNestedCall(payload) => {
                self.invoke_ctx_runtime_enter_nested_call(runtime_host_payload_value(payload)?)
            }
            _ => unreachable!("non-function host operation routed to function dispatcher"),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_function_host_call_cancellable(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::HttpRoute(payload) => self
                .invoke_http_route_cancellable(runtime_host_payload_value(payload)?, cancellation),
            HostCallPayload::CtxQuery(payload) => self
                .invoke_ctx_query_cancellable(runtime_host_payload_value(payload)?, cancellation),
            HostCallPayload::CtxPaginatedQuery(payload) => self
                .invoke_ctx_paginated_query_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                ),
            HostCallPayload::CtxMutation(payload) => self.invoke_ctx_mutation_cancellable(
                runtime_host_payload_value(payload)?,
                cancellation,
            ),
            HostCallPayload::CtxAction(payload) => self
                .invoke_ctx_action_cancellable(runtime_host_payload_value(payload)?, cancellation),
            HostCallPayload::CtxRunQuery(payload) => self.invoke_ctx_run_query_cancellable(
                runtime_host_payload_value(payload)?,
                cancellation,
            ),
            HostCallPayload::CtxRunMutation(payload) => self.invoke_ctx_run_mutation_cancellable(
                runtime_host_payload_value(payload)?,
                cancellation,
            ),
            HostCallPayload::CtxRunAction(payload) => self.invoke_ctx_run_action_cancellable(
                runtime_host_payload_value(payload)?,
                cancellation,
            ),
            HostCallPayload::CtxServiceLookup(payload) => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_service_lookup(runtime_host_payload_value(payload)?)
            }
            HostCallPayload::CtxRuntimeEnterNestedCall(payload) => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_runtime_enter_nested_call(runtime_host_payload_value(payload)?)
            }
            _ => unreachable!("non-function host operation routed to function dispatcher"),
        }
    }

    pub(in crate::adapters::convex) async fn dispatch_function_host_call_async(
        &self,
        payload: HostCallPayload,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match payload {
            HostCallPayload::HttpRoute(payload) => {
                self.invoke_http_route_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxQuery(payload) => {
                self.invoke_ctx_query_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxPaginatedQuery(payload) => {
                self.invoke_ctx_paginated_query_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxMutation(payload) => {
                self.invoke_ctx_mutation_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxAction(payload) => {
                self.invoke_ctx_action_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxRunQuery(payload) => {
                self.invoke_ctx_run_query_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxRunMutation(payload) => {
                self.invoke_ctx_run_mutation_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxRunAction(payload) => {
                self.invoke_ctx_run_action_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxServiceLookup(payload) => {
                self.invoke_ctx_service_lookup_async_cancellable(
                    runtime_host_payload_value(payload)?,
                    cancellation,
                )
                .await
            }
            HostCallPayload::CtxRuntimeEnterNestedCall(payload) => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_runtime_enter_nested_call(runtime_host_payload_value(payload)?)
            }
            _ => unreachable!("non-function host operation routed to function dispatcher"),
        }
    }
}
