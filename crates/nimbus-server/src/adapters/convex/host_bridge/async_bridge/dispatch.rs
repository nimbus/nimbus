use super::*;
use nimbus_runtime::{HostCallEnvelope, HostCallOperation, HostCallPayload, InvocationKind};

impl ConvexHostBridge {
    pub(in crate::adapters::convex) async fn dispatch_host_call_async(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        self.validate_host_call_session(envelope.payload.host_call_session_id())?;
        self.ensure_invocation_allows_host_call(&envelope.payload)?;
        match envelope.payload {
            payload @ (HostCallPayload::HttpRoute(_)
            | HostCallPayload::CtxQuery(_)
            | HostCallPayload::CtxPaginatedQuery(_)
            | HostCallPayload::CtxMutation(_)
            | HostCallPayload::CtxAction(_)
            | HostCallPayload::CtxRunQuery(_)
            | HostCallPayload::CtxRunMutation(_)
            | HostCallPayload::CtxRunAction(_)
            | HostCallPayload::CtxServiceLookup(_)
            | HostCallPayload::CtxRuntimeEnterNestedCall(_)
            | HostCallPayload::CtxResolveCalleeLane(_)) => {
                self.dispatch_function_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::QueryBuilderStart(_)
            | HostCallPayload::QueryBuilderWithIndex(_)
            | HostCallPayload::QueryBuilderFilter(_)
            | HostCallPayload::QueryBuilderOrder(_)) => {
                self.dispatch_query_builder_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::QueryReadCollect(_)
            | HostCallPayload::QueryReadTake(_)
            | HostCallPayload::QueryReadPaginate(_)
            | HostCallPayload::QueryReadFirst(_)
            | HostCallPayload::QueryReadUnique(_)) => {
                self.dispatch_query_read_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => {
                self.dispatch_document_host_call_async(payload, cancellation)
                    .await
            }
            HostCallPayload::RuntimeExtensionCall(payload) => {
                self.dispatch_adapter_extension_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::CtxSchedulerRunAfter(_)
            | HostCallPayload::CtxSchedulerRunAt(_)
            | HostCallPayload::CtxSchedulerCancel(_)) => {
                self.dispatch_scheduler_host_call_async(payload, cancellation)
                    .await
            }
            payload @ (HostCallPayload::CfKvGet(_)
            | HostCallPayload::CfKvPut(_)
            | HostCallPayload::CfKvDelete(_)
            | HostCallPayload::CfKvList(_)) => unsupported_adapter_owned_host_call(payload),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_host_call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        self.validate_host_call_session(envelope.payload.host_call_session_id())?;
        self.ensure_invocation_allows_host_call(&envelope.payload)?;
        match envelope.payload {
            payload @ (HostCallPayload::HttpRoute(_)
            | HostCallPayload::CtxQuery(_)
            | HostCallPayload::CtxPaginatedQuery(_)
            | HostCallPayload::CtxMutation(_)
            | HostCallPayload::CtxAction(_)
            | HostCallPayload::CtxRunQuery(_)
            | HostCallPayload::CtxRunMutation(_)
            | HostCallPayload::CtxRunAction(_)
            | HostCallPayload::CtxServiceLookup(_)
            | HostCallPayload::CtxRuntimeEnterNestedCall(_)
            | HostCallPayload::CtxResolveCalleeLane(_)) => {
                self.dispatch_function_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::QueryBuilderStart(_)
            | HostCallPayload::QueryBuilderWithIndex(_)
            | HostCallPayload::QueryBuilderFilter(_)
            | HostCallPayload::QueryBuilderOrder(_)) => {
                self.dispatch_query_builder_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::QueryReadCollect(_)
            | HostCallPayload::QueryReadTake(_)
            | HostCallPayload::QueryReadPaginate(_)
            | HostCallPayload::QueryReadFirst(_)
            | HostCallPayload::QueryReadUnique(_)) => {
                self.dispatch_query_read_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => {
                self.dispatch_document_host_call_cancellable(payload, cancellation)
            }
            HostCallPayload::RuntimeExtensionCall(payload) => {
                self.dispatch_adapter_extension_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::CtxSchedulerRunAfter(_)
            | HostCallPayload::CtxSchedulerRunAt(_)
            | HostCallPayload::CtxSchedulerCancel(_)) => {
                self.dispatch_scheduler_host_call_cancellable(payload, cancellation)
            }
            payload @ (HostCallPayload::CfKvGet(_)
            | HostCallPayload::CfKvPut(_)
            | HostCallPayload::CfKvDelete(_)
            | HostCallPayload::CfKvList(_)) => unsupported_adapter_owned_host_call(payload),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_host_call(
        &self,
        request: HostCallRequest,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        self.validate_host_call_session(envelope.payload.host_call_session_id())?;
        self.ensure_invocation_allows_host_call(&envelope.payload)?;
        match envelope.payload {
            payload @ (HostCallPayload::HttpRoute(_)
            | HostCallPayload::CtxQuery(_)
            | HostCallPayload::CtxPaginatedQuery(_)
            | HostCallPayload::CtxMutation(_)
            | HostCallPayload::CtxAction(_)
            | HostCallPayload::CtxRunQuery(_)
            | HostCallPayload::CtxRunMutation(_)
            | HostCallPayload::CtxRunAction(_)
            | HostCallPayload::CtxServiceLookup(_)
            | HostCallPayload::CtxRuntimeEnterNestedCall(_)
            | HostCallPayload::CtxResolveCalleeLane(_)) => {
                self.dispatch_function_host_call(payload)
            }
            payload @ (HostCallPayload::QueryBuilderStart(_)
            | HostCallPayload::QueryBuilderWithIndex(_)
            | HostCallPayload::QueryBuilderFilter(_)
            | HostCallPayload::QueryBuilderOrder(_)) => {
                self.dispatch_query_builder_host_call(payload)
            }
            payload @ (HostCallPayload::QueryReadCollect(_)
            | HostCallPayload::QueryReadTake(_)
            | HostCallPayload::QueryReadPaginate(_)
            | HostCallPayload::QueryReadFirst(_)
            | HostCallPayload::QueryReadUnique(_)) => self.dispatch_query_read_host_call(payload),
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => self.dispatch_document_host_call(payload),
            HostCallPayload::RuntimeExtensionCall(payload) => {
                self.dispatch_adapter_extension_host_call(payload)
            }
            payload @ (HostCallPayload::CtxSchedulerRunAfter(_)
            | HostCallPayload::CtxSchedulerRunAt(_)
            | HostCallPayload::CtxSchedulerCancel(_)) => self.dispatch_scheduler_host_call(payload),
            payload @ (HostCallPayload::CfKvGet(_)
            | HostCallPayload::CfKvPut(_)
            | HostCallPayload::CfKvDelete(_)
            | HostCallPayload::CfKvList(_)) => unsupported_adapter_owned_host_call(payload),
        }
    }

    fn ensure_invocation_allows_host_call(
        &self,
        payload: &HostCallPayload,
    ) -> std::result::Result<(), NimbusRuntimeError> {
        if convex_host_call_allowed(self.invocation_kind(), payload) {
            return Ok(());
        }
        Err(NimbusRuntimeError::Contract(format!(
            "convex `{}` invocation cannot use `{}` host capability",
            self.invocation_kind().as_str(),
            convex_host_operation_name(payload.operation())
        )))
    }
}

fn convex_host_call_allowed(kind: &InvocationKind, payload: &HostCallPayload) -> bool {
    let nested_kind = match payload {
        HostCallPayload::CtxRuntimeEnterNestedCall(payload) => payload.kind.as_deref(),
        _ => None,
    };
    convex_host_operation_allowed(kind, payload.operation(), nested_kind)
}

fn convex_host_operation_allowed(
    kind: &InvocationKind,
    operation: HostCallOperation,
    nested_kind: Option<&str>,
) -> bool {
    let shared = matches!(
        operation,
        HostCallOperation::CtxServiceLookup
            | HostCallOperation::CtxResolveCalleeLane
            | HostCallOperation::RuntimeExtensionCall
    );
    let read = matches!(
        operation,
        HostCallOperation::CtxQuery
            | HostCallOperation::CtxPaginatedQuery
            | HostCallOperation::CtxRunQuery
            | HostCallOperation::DocumentGet
            | HostCallOperation::QueryBuilderStart
            | HostCallOperation::QueryBuilderWithIndex
            | HostCallOperation::QueryBuilderFilter
            | HostCallOperation::QueryBuilderOrder
            | HostCallOperation::QueryReadCollect
            | HostCallOperation::QueryReadTake
            | HostCallOperation::QueryReadPaginate
            | HostCallOperation::QueryReadFirst
            | HostCallOperation::QueryReadUnique
    );
    let write = matches!(
        operation,
        HostCallOperation::CtxMutation
            | HostCallOperation::CtxRunMutation
            | HostCallOperation::DocumentInsert
            | HostCallOperation::DocumentPatch
            | HostCallOperation::DocumentDelete
    );
    let schedule = matches!(
        operation,
        HostCallOperation::CtxSchedulerRunAfter
            | HostCallOperation::CtxSchedulerRunAt
            | HostCallOperation::CtxSchedulerCancel
    );

    match kind {
        InvocationKind::Query | InvocationKind::PaginatedQuery => {
            shared
                || read
                || (operation == HostCallOperation::CtxRuntimeEnterNestedCall
                    && matches!(nested_kind, Some("query" | "paginated_query")))
        }
        InvocationKind::Mutation => {
            shared
                || read
                || write
                || schedule
                || (operation == HostCallOperation::CtxRuntimeEnterNestedCall
                    && matches!(nested_kind, Some("query" | "paginated_query" | "mutation")))
        }
        InvocationKind::Action => {
            shared
                || matches!(
                    operation,
                    HostCallOperation::HttpRoute
                        | HostCallOperation::CtxAction
                        | HostCallOperation::CtxRunQuery
                        | HostCallOperation::CtxRunMutation
                        | HostCallOperation::CtxRunAction
                )
                || schedule
                || (operation == HostCallOperation::CtxRuntimeEnterNestedCall
                    && matches!(
                        nested_kind,
                        Some("query" | "paginated_query" | "mutation" | "action")
                    ))
        }
        InvocationKind::CloudflareWorkerFetch => matches!(
            operation,
            HostCallOperation::HttpRoute
                | HostCallOperation::CfKvGet
                | HostCallOperation::CfKvPut
                | HostCallOperation::CfKvDelete
                | HostCallOperation::CfKvList
                | HostCallOperation::RuntimeExtensionCall
        ),
    }
}

fn unsupported_adapter_owned_host_call(
    payload: HostCallPayload,
) -> std::result::Result<Value, NimbusRuntimeError> {
    Err(NimbusRuntimeError::Contract(format!(
        "convex host bridge does not own `{}` runtime compatibility; that host call is adapter-owned",
        convex_host_operation_name(payload.operation())
    )))
}

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn query_capabilities_reject_writes_scheduling_and_effectful_nested_calls() {
        for operation in [
            HostCallOperation::CtxMutation,
            HostCallOperation::DocumentInsert,
            HostCallOperation::CtxSchedulerRunAfter,
            HostCallOperation::CtxRunMutation,
            HostCallOperation::CtxRunAction,
        ] {
            assert!(!convex_host_operation_allowed(
                &InvocationKind::Query,
                operation,
                None
            ));
        }
        assert!(convex_host_operation_allowed(
            &InvocationKind::Query,
            HostCallOperation::CtxRunQuery,
            None
        ));
        assert!(convex_host_operation_allowed(
            &InvocationKind::Query,
            HostCallOperation::CtxRuntimeEnterNestedCall,
            Some("query")
        ));
        assert!(!convex_host_operation_allowed(
            &InvocationKind::Query,
            HostCallOperation::CtxRuntimeEnterNestedCall,
            Some("mutation")
        ));
    }

    #[test]
    fn mutation_and_action_capabilities_match_the_context_contract() {
        for operation in [
            HostCallOperation::DocumentGet,
            HostCallOperation::DocumentInsert,
            HostCallOperation::CtxSchedulerCancel,
            HostCallOperation::CtxRunMutation,
        ] {
            assert!(convex_host_operation_allowed(
                &InvocationKind::Mutation,
                operation,
                None
            ));
        }
        assert!(!convex_host_operation_allowed(
            &InvocationKind::Mutation,
            HostCallOperation::CtxRunAction,
            None
        ));

        for operation in [
            HostCallOperation::CtxAction,
            HostCallOperation::CtxRunQuery,
            HostCallOperation::CtxRunMutation,
            HostCallOperation::CtxRunAction,
            HostCallOperation::CtxSchedulerRunAt,
        ] {
            assert!(convex_host_operation_allowed(
                &InvocationKind::Action,
                operation,
                None
            ));
        }
        for operation in [
            HostCallOperation::CtxQuery,
            HostCallOperation::CtxMutation,
            HostCallOperation::DocumentGet,
            HostCallOperation::DocumentInsert,
        ] {
            assert!(!convex_host_operation_allowed(
                &InvocationKind::Action,
                operation,
                None
            ));
        }
    }

    #[test]
    fn cloudflare_worker_capabilities_remain_adapter_owned() {
        assert!(convex_host_operation_allowed(
            &InvocationKind::CloudflareWorkerFetch,
            HostCallOperation::CfKvGet,
            None
        ));
        assert!(!convex_host_operation_allowed(
            &InvocationKind::CloudflareWorkerFetch,
            HostCallOperation::DocumentGet,
            None
        ));
        assert!(!convex_host_operation_allowed(
            &InvocationKind::CloudflareWorkerFetch,
            HostCallOperation::CtxRuntimeEnterNestedCall,
            Some("query")
        ));
    }
}
