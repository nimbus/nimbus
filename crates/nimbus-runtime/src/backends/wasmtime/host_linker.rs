use std::fmt;
use std::sync::Arc;

use serde_json::{Value, json};
use wasmtime::component::Linker;

use crate::RuntimeInvocationContext;
use crate::error::{NimbusRuntimeError, Result};
use crate::host::{
    HostBridge, HostCallCancellation, HostCallOperation, HostCallRequest,
    RuntimeAsyncDbDeletePayload, RuntimeAsyncDbGetPayload, RuntimeAsyncDbInsertPayload,
    RuntimeAsyncDbPatchPayload, RuntimeAsyncFunctionCallPayload,
    RuntimeAsyncSchedulerCancelPayload, RuntimeAsyncSchedulerRunAfterPayload,
    RuntimeAsyncSchedulerRunAtPayload,
};

const DATABASE_INSTANCE: &str = "nimbus:host/database@0.1.0";
const SCHEDULER_INSTANCE: &str = "nimbus:host/scheduler@0.1.0";
const RUNTIME_INSTANCE: &str = "nimbus:host/runtime@0.1.0";
const CONTEXT_INSTANCE: &str = "nimbus:host/context@0.1.0";

pub(crate) type WasmtimeHostLinker = Linker<InvocationHostState>;

#[derive(Clone)]
pub(crate) struct InvocationHostState {
    bridge: Arc<dyn HostBridge>,
    context: RuntimeInvocationContext,
    cancellation: HostCallCancellation,
    limiter: WasmtimeResourceLimiter,
}

impl InvocationHostState {
    pub(crate) fn new(
        bridge: Arc<dyn HostBridge>,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Self {
        Self {
            bridge,
            context,
            cancellation: cancellation.unwrap_or_default(),
            limiter: WasmtimeResourceLimiter::default(),
        }
    }

    pub(crate) fn new_for_policy(
        bridge: Arc<dyn HostBridge>,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
        policy: &crate::limits::RuntimePolicy,
    ) -> Self {
        Self {
            bridge,
            context,
            cancellation: cancellation.unwrap_or_default(),
            limiter: WasmtimeResourceLimiter::for_policy(policy),
        }
    }

    pub(crate) fn reset_for_invocation(
        &mut self,
        bridge: Arc<dyn HostBridge>,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
        policy: &crate::limits::RuntimePolicy,
    ) {
        self.bridge = bridge;
        self.context = context;
        self.cancellation = cancellation.unwrap_or_default();
        self.limiter = WasmtimeResourceLimiter::for_policy(policy);
    }

    pub(crate) fn resource_limiter(&mut self) -> &mut dyn wasmtime::ResourceLimiter {
        &mut self.limiter
    }

    async fn call_async(&self, request: HostCallRequest) -> std::result::Result<String, String> {
        host_value_to_wit_result(
            self.bridge
                .call_async(request, self.cancellation.clone())
                .await,
        )
    }

    fn context_identity(&self) -> String {
        "null".to_string()
    }
}

#[derive(Clone)]
pub(crate) struct WasmtimeResourceLimiter {
    max_memory_bytes: usize,
    max_tables: usize,
    max_instances: usize,
}

impl Default for WasmtimeResourceLimiter {
    fn default() -> Self {
        Self {
            max_memory_bytes: 128 * 1024 * 1024,
            max_tables: 64,
            max_instances: 64,
        }
    }
}

impl WasmtimeResourceLimiter {
    fn for_policy(policy: &crate::limits::RuntimePolicy) -> Self {
        Self {
            max_memory_bytes: policy.limits().max_heap_mb.saturating_mul(1024 * 1024),
            max_tables: 64,
            max_instances: 64,
        }
    }
}

impl wasmtime::ResourceLimiter for WasmtimeResourceLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired <= self.max_memory_bytes {
            return Ok(true);
        }
        Err(wasmtime::Error::msg(format!(
            "Wasmtime ResourceLimiter memory limit exceeded: desired_bytes={desired} max_bytes={}",
            self.max_memory_bytes
        )))
    }

    fn table_growing(
        &mut self,
        _current: usize,
        _desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(true)
    }

    fn tables(&self) -> usize {
        self.max_tables
    }

    fn instances(&self) -> usize {
        self.max_instances
    }
}

impl fmt::Debug for InvocationHostState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InvocationHostState")
            .field("invocation_id", &self.context.invocation_id)
            .field("function_name", &self.context.function_name)
            .field("kind", &self.context.kind)
            .field("tenant_label", &self.context.tenant_label)
            .finish_non_exhaustive()
    }
}

pub(crate) fn create_wasmtime_component_engine() -> Result<wasmtime::Engine> {
    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.consume_fuel(true);
    wasmtime::Engine::new(&config).map_err(wasmtime_error)
}

pub(crate) fn component_linker_diagnostics() -> Result<()> {
    let engine = create_wasmtime_component_engine()?;
    let _linker = build_nimbus_host_linker(&engine)?;
    let _state = InvocationHostState::new(
        Arc::new(WasmtimeLinkerSmokeHost),
        RuntimeInvocationContext {
            invocation_id: 0,
            function_name: "wasmtime:diagnostics".to_string(),
            kind: "query",
            is_top_level: true,
            bypasses_concurrency_limit: true,
            tenant_label: Some("diagnostics".to_string()),
            runtime_owner_lease: None,
            deployment_authority_lease: None,
            server_request_id: None,
        },
        None,
    );
    Ok(())
}

pub(crate) fn build_nimbus_host_linker(engine: &wasmtime::Engine) -> Result<WasmtimeHostLinker> {
    let mut linker = Linker::<InvocationHostState>::new(engine);
    add_database_imports(&mut linker).map_err(wasmtime_error)?;
    add_scheduler_imports(&mut linker).map_err(wasmtime_error)?;
    add_runtime_imports(&mut linker).map_err(wasmtime_error)?;
    add_context_imports(&mut linker).map_err(wasmtime_error)?;
    Ok(linker)
}

fn add_database_imports(linker: &mut WasmtimeHostLinker) -> wasmtime::Result<()> {
    let mut database = linker.instance(DATABASE_INSTANCE)?;
    database.func_wrap_async("get", |store, (table, id): (String, String)| {
        let request = HostCallRequest::new(
            HostCallOperation::DocumentGet,
            json!(RuntimeAsyncDbGetPayload {
                table,
                id,
                host_call_session_id: None,
            }),
        );
        Box::new(async move { Ok((store.data().call_async(request).await,)) })
    })?;
    database.func_wrap_async("insert", |store, (table, fields_json): (String, String)| {
        let request = match parse_json_argument("fields-json", &fields_json).map(|fields| {
            HostCallRequest::new(
                HostCallOperation::DocumentInsert,
                json!(RuntimeAsyncDbInsertPayload {
                    table,
                    fields,
                    host_call_session_id: None,
                }),
            )
        }) {
            Ok(request) => request,
            Err(error) => return Box::new(async move { Ok((Err(error.to_string()),)) }),
        };
        Box::new(async move { Ok((store.data().call_async(request).await,)) })
    })?;
    database.func_wrap_async(
        "patch",
        |store, (table, id, patch_json): (String, String, String)| {
            let request = match parse_json_argument("patch-json", &patch_json).map(|patch| {
                HostCallRequest::new(
                    HostCallOperation::DocumentPatch,
                    json!(RuntimeAsyncDbPatchPayload {
                        table,
                        id,
                        patch,
                        host_call_session_id: None,
                    }),
                )
            }) {
                Ok(request) => request,
                Err(error) => return Box::new(async move { Ok((Err(error.to_string()),)) }),
            };
            Box::new(async move { Ok((store.data().call_async(request).await,)) })
        },
    )?;
    database.func_wrap_async("delete", |store, (table, id): (String, String)| {
        let request = HostCallRequest::new(
            HostCallOperation::DocumentDelete,
            json!(RuntimeAsyncDbDeletePayload {
                table,
                id,
                host_call_session_id: None,
            }),
        );
        Box::new(async move { Ok((store.data().call_async(request).await,)) })
    })?;
    Ok(())
}

fn add_scheduler_imports(linker: &mut WasmtimeHostLinker) -> wasmtime::Result<()> {
    let mut scheduler = linker.instance(SCHEDULER_INSTANCE)?;
    scheduler.func_wrap_async(
        "run-after",
        |store, (delay_ms, name, visibility, args_json): (u64, String, String, String)| {
            let request = match parse_json_argument("args-json", &args_json).map(|args| {
                HostCallRequest::new(
                    HostCallOperation::CtxSchedulerRunAfter,
                    json!(RuntimeAsyncSchedulerRunAfterPayload {
                        delay_ms,
                        name,
                        visibility,
                        args,
                        host_call_session_id: None,
                    }),
                )
            }) {
                Ok(request) => request,
                Err(error) => return Box::new(async move { Ok((Err(error.to_string()),)) }),
            };
            Box::new(async move { Ok((store.data().call_async(request).await,)) })
        },
    )?;
    scheduler.func_wrap_async(
        "run-at",
        |store, (timestamp_ms, name, visibility, args_json): (u64, String, String, String)| {
            let request = match parse_json_argument("args-json", &args_json).map(|args| {
                HostCallRequest::new(
                    HostCallOperation::CtxSchedulerRunAt,
                    json!(RuntimeAsyncSchedulerRunAtPayload {
                        timestamp_ms,
                        name,
                        visibility,
                        args,
                        host_call_session_id: None,
                    }),
                )
            }) {
                Ok(request) => request,
                Err(error) => return Box::new(async move { Ok((Err(error.to_string()),)) }),
            };
            Box::new(async move { Ok((store.data().call_async(request).await,)) })
        },
    )?;
    scheduler.func_wrap_async("cancel", |store, (job_id,): (String,)| {
        let request = HostCallRequest::new(
            HostCallOperation::CtxSchedulerCancel,
            json!(RuntimeAsyncSchedulerCancelPayload {
                job_id,
                host_call_session_id: None,
            }),
        );
        Box::new(async move { Ok((store.data().call_async(request).await,)) })
    })?;
    Ok(())
}

fn add_runtime_imports(linker: &mut WasmtimeHostLinker) -> wasmtime::Result<()> {
    let mut runtime = linker.instance(RUNTIME_INSTANCE)?;
    runtime.func_wrap_async(
        "run-query",
        |store, (name, visibility, args_json): (String, String, String)| {
            let request = match function_call_request(
                HostCallOperation::CtxRunQuery,
                name,
                visibility,
                args_json,
            ) {
                Ok(request) => request,
                Err(error) => return Box::new(async move { Ok((Err(error.to_string()),)) }),
            };
            Box::new(async move { Ok((store.data().call_async(request).await,)) })
        },
    )?;
    runtime.func_wrap_async(
        "run-mutation",
        |store, (name, visibility, args_json): (String, String, String)| {
            let request = match function_call_request(
                HostCallOperation::CtxRunMutation,
                name,
                visibility,
                args_json,
            ) {
                Ok(request) => request,
                Err(error) => return Box::new(async move { Ok((Err(error.to_string()),)) }),
            };
            Box::new(async move { Ok((store.data().call_async(request).await,)) })
        },
    )?;
    runtime.func_wrap_async(
        "run-action",
        |store, (name, visibility, args_json): (String, String, String)| {
            let request = match function_call_request(
                HostCallOperation::CtxRunAction,
                name,
                visibility,
                args_json,
            ) {
                Ok(request) => request,
                Err(error) => return Box::new(async move { Ok((Err(error.to_string()),)) }),
            };
            Box::new(async move { Ok((store.data().call_async(request).await,)) })
        },
    )?;
    Ok(())
}

fn add_context_imports(linker: &mut WasmtimeHostLinker) -> wasmtime::Result<()> {
    let mut context = linker.instance(CONTEXT_INSTANCE)?;
    context.func_wrap("tenant-id", |store, (): ()| {
        Ok((store
            .data()
            .context
            .tenant_label
            .clone()
            .unwrap_or_default(),))
    })?;
    context.func_wrap("function-name", |store, (): ()| {
        Ok((store.data().context.function_name.clone(),))
    })?;
    context.func_wrap("invocation-id", |store, (): ()| {
        Ok((store.data().context.invocation_id.to_string(),))
    })?;
    context.func_wrap("invocation-kind", |store, (): ()| {
        Ok((store.data().context.kind.to_string(),))
    })?;
    context.func_wrap("identity", |store, (): ()| {
        Ok((store.data().context_identity(),))
    })?;
    Ok(())
}

fn function_call_request(
    operation: HostCallOperation,
    name: String,
    visibility: String,
    args_json: String,
) -> Result<HostCallRequest> {
    let args = parse_json_argument("args-json", &args_json)?;
    Ok(HostCallRequest::new(
        operation,
        json!(RuntimeAsyncFunctionCallPayload {
            name,
            visibility,
            args,
            host_call_session_id: None,
        }),
    ))
}

fn parse_json_argument(label: &str, value: &str) -> Result<Value> {
    serde_json::from_str(value).map_err(|error| {
        NimbusRuntimeError::Contract(format!(
            "invalid {label} JSON for Wasmtime host import: {error}"
        ))
    })
}

fn host_value_to_wit_result(result: Result<Value>) -> std::result::Result<String, String> {
    match result {
        Ok(value) => serde_json::to_string(&value).map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    }
}

fn wasmtime_error(error: wasmtime::Error) -> NimbusRuntimeError {
    NimbusRuntimeError::Contract(format!("wasmtime component host linker error: {error}"))
}

struct WasmtimeLinkerSmokeHost;

impl HostBridge for WasmtimeLinkerSmokeHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "Wasmtime linker diagnostics host must not be invoked".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;
    use wasmtime::component::Component;

    use crate::host::{HOST_CALL_ABI_VERSION, HostBridgeFuture};
    use crate::runtime::{InvocationKind, InvocationRequest};

    use super::*;

    #[derive(Default)]
    struct RecordingHost {
        requests: Mutex<Vec<HostCallRequest>>,
    }

    impl RecordingHost {
        fn requests(&self) -> Vec<HostCallRequest> {
            self.requests
                .lock()
                .expect("recorded host requests should not be poisoned")
                .clone()
        }
    }

    impl HostBridge for RecordingHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            self.requests
                .lock()
                .expect("recorded host requests should not be poisoned")
                .push(request);
            Ok(json!({ "ok": true }))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            let result = self.call_cancellable(request, &cancellation);
            Box::pin(async move { result })
        }
    }

    fn test_context() -> RuntimeInvocationContext {
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:list".to_string(),
            args: json!({}),
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };
        RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a")
    }

    #[tokio::test]
    async fn wasmtime_linker_routes_database_imports_through_host_bridge() {
        let host = Arc::new(RecordingHost::default());
        let state = InvocationHostState::new(host.clone(), test_context(), None);

        let response = state
            .call_async(HostCallRequest::new(
                HostCallOperation::DocumentGet,
                json!({ "table": "messages", "id": "doc-1" }),
            ))
            .await
            .expect("host call should succeed");

        assert_eq!(response, r#"{"ok":true}"#);
        assert_eq!(
            host.requests(),
            vec![HostCallRequest {
                abi_version: HOST_CALL_ABI_VERSION,
                operation: HostCallOperation::DocumentGet,
                payload: json!({ "table": "messages", "id": "doc-1" }),
            }]
        );
    }

    #[tokio::test]
    async fn wasmtime_linker_typechecks_nimbus_host_database_import() {
        let engine = create_wasmtime_component_engine().expect("engine should be configured");
        let linker = build_nimbus_host_linker(&engine).expect("linker should build");
        let host = Arc::new(RecordingHost::default());
        let mut store = wasmtime::Store::new(
            &engine,
            InvocationHostState::new(host.clone(), test_context(), None),
        );
        let component = Component::new(
            &engine,
            r#"
                (component
                  (type $host-result (result string (error string)))
                  (type $get (func
                    (param "table" string)
                    (param "id" string)
                    (result $host-result)))
                  (type $database (instance
                    (export "get" (func (type $get)))))
                  (import "nimbus:host/database@0.1.0" (instance $database (type $database)))
                )
            "#,
        )
        .expect("test component should compile");

        let _instance = linker
            .instantiate_async(&mut store, &component)
            .await
            .expect("component should instantiate against nimbus:host imports");
        assert!(
            host.requests().is_empty(),
            "linker typechecking must not call host imports until guest code invokes them"
        );
    }

    #[test]
    fn wasmtime_linker_rejects_invalid_json_payloads_before_host_bridge() {
        let host = Arc::new(RecordingHost::default());

        let error = function_call_request(
            HostCallOperation::CtxRunQuery,
            "tasks:list".to_string(),
            "public".to_string(),
            "not-json".to_string(),
        )
        .expect_err("invalid WIT JSON argument should fail before host call");

        assert!(
            error.to_string().contains("invalid args-json JSON"),
            "unexpected error: {error}"
        );
        assert!(host.requests().is_empty());
    }
}
