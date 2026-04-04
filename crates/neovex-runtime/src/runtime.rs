use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use deno_core::{JsRuntime, PollEventLoopOptions, RuntimeOptions, scope, serde_v8, v8};
use serde_json::Value;

use crate::RuntimeInvocationContext;
use crate::error::{NeovexRuntimeError, Result};
use crate::executor::{RuntimeExecutor, SharedInvocationPermit};
use crate::host::{HostBridge, HostCallCancellation};
use crate::limits::{RuntimeLimits, RuntimePolicy};
use crate::module_loader::SandboxedModuleLoader;
use crate::watchdog::WatchdogTimer;

mod bootstrap;
mod bundle;
mod invocation;

use self::bootstrap::{RuntimeCancellationState, RuntimeStartupSnapshot};
pub(crate) use self::bootstrap::{RuntimeInvocationTimeoutController, RuntimeWorkerIsolatePool};
pub use self::bundle::RuntimeBundle;
pub use self::invocation::{
    InvocationAuth, InvocationKind, InvocationRequest, RuntimeUserIdentity, VerifiedUserIdentity,
    VerifiedUserIdentityKind,
};

#[derive(Clone)]
pub struct NeovexRuntime {
    host: Arc<dyn HostBridge>,
    policy: Arc<RuntimePolicy>,
    bypass_concurrency_limit: bool,
    owned_executor: Arc<OnceLock<RuntimeExecutor>>,
}

pub(crate) struct RuntimeInvocationExecution {
    pub(crate) watchdog: WatchdogTimer,
    pub(crate) bundle: RuntimeBundle,
    pub(crate) request: InvocationRequest,
    pub(crate) context: RuntimeInvocationContext,
    pub(crate) external_cancellation: Option<HostCallCancellation>,
    pub(crate) permit: SharedInvocationPermit,
}

/// Legacy alias for Convex-shaped integrations.
pub type ConvexRuntime = NeovexRuntime;

impl NeovexRuntime {
    pub fn new(host: Arc<dyn HostBridge>) -> Self {
        Self::with_policy(host, Arc::new(RuntimePolicy::default()))
    }

    pub fn with_limits(host: Arc<dyn HostBridge>, limits: RuntimeLimits) -> Self {
        Self::with_policy(host, Arc::new(RuntimePolicy::new(limits)))
    }

    pub fn with_policy(host: Arc<dyn HostBridge>, policy: Arc<RuntimePolicy>) -> Self {
        Self {
            host,
            policy,
            bypass_concurrency_limit: false,
            owned_executor: Arc::new(OnceLock::new()),
        }
    }

    pub fn with_policy_bypassing_limit(
        host: Arc<dyn HostBridge>,
        policy: Arc<RuntimePolicy>,
    ) -> Self {
        Self {
            host,
            policy,
            bypass_concurrency_limit: true,
            owned_executor: Arc::new(OnceLock::new()),
        }
    }

    pub(crate) fn into_policy(self, policy: Arc<RuntimePolicy>) -> Self {
        Self {
            policy,
            owned_executor: Arc::new(OnceLock::new()),
            ..self
        }
    }

    /// Returns the stable executor handle that powers this runtime's public
    /// convenience invocation APIs.
    pub fn executor(&self) -> RuntimeExecutor {
        self.owned_executor
            .get_or_init(|| RuntimeExecutor::new(self.policy.clone()))
            .clone()
    }

    pub async fn invoke_bundle(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
    ) -> Result<Value> {
        self.invoke_bundle_with_cancellation(bundle, request, None)
            .await
    }

    pub async fn invoke_bundle_with_cancellation(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.executor()
            .invoke_on_worker(
                self.clone(),
                bundle.clone(),
                request.clone(),
                RuntimeInvocationContext::top_level(request),
                cancellation,
            )
            .await
    }

    pub fn invoke_bundle_blocking(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
    ) -> Result<Value> {
        self.invoke_bundle_blocking_with_cancellation(bundle, request, None)
    }

    pub fn invoke_bundle_blocking_with_cancellation(
        &self,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.executor().invoke_blocking_with_cancellation(
            self.clone(),
            bundle.clone(),
            request.clone(),
            RuntimeInvocationContext::top_level(request),
            cancellation,
        )
    }

    pub(crate) fn bypasses_concurrency_limit(&self) -> bool {
        self.bypass_concurrency_limit
    }

    pub(crate) fn policy(&self) -> Arc<RuntimePolicy> {
        self.policy.clone()
    }

    pub(crate) async fn invoke_bundle_unmanaged(
        &self,
        isolate_pool: Option<&mut RuntimeWorkerIsolatePool>,
        invocation: RuntimeInvocationExecution,
    ) -> Result<Value> {
        let RuntimeInvocationExecution {
            watchdog,
            bundle,
            request,
            context: _context,
            external_cancellation,
            permit,
        } = invocation;
        bundle.verify_integrity()?;
        let mut isolate_pool = isolate_pool;
        let mut runtime = match isolate_pool.as_deref_mut() {
            Some(pool) => pool.take_runtime(self, &bundle)?,
            None => {
                let snapshot = self.bootstrap_snapshot()?;
                self.create_runtime_from_snapshot(&bundle, snapshot)?
            }
        };
        let timeout = self.policy.limits().execution_timeout;
        let timeout_triggered = Arc::new(AtomicBool::new(false));
        let heap_limit_triggered = Arc::new(AtomicBool::new(false));
        let external_cancellation_triggered = Arc::new(AtomicBool::new(false));
        let cancellation_signal = {
            let op_state = runtime.op_state();
            op_state
                .borrow()
                .borrow::<RuntimeCancellationState>()
                .signal
                .clone()
        };
        let external_cancellation_watchdog = external_cancellation
            .clone()
            .map(|external| {
                let isolate_handle = runtime.v8_isolate().thread_safe_handle();
                let cancellation_signal = cancellation_signal.clone();
                let external_cancellation_triggered = external_cancellation_triggered.clone();
                watchdog.register_cancellation(external, move || {
                    external_cancellation_triggered.store(true, Ordering::SeqCst);
                    cancellation_signal.cancel();
                    let _ = isolate_handle.terminate_execution();
                })
            })
            .transpose()?;

        {
            let heap_limit_triggered = heap_limit_triggered.clone();
            let cancellation_signal = cancellation_signal.clone();
            let isolate_handle = runtime.v8_isolate().thread_safe_handle();
            runtime.add_near_heap_limit_callback(move |current_limit, _initial_limit| {
                heap_limit_triggered.store(true, Ordering::SeqCst);
                cancellation_signal.cancel();
                let _ = isolate_handle.terminate_execution();
                current_limit.saturating_mul(2)
            });
        }

        let timeout_controller = if timeout.is_zero() {
            None
        } else {
            let isolate_handle = runtime.v8_isolate().thread_safe_handle();
            let timeout_triggered = timeout_triggered.clone();
            let cancellation_signal = cancellation_signal.clone();
            let callback: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                timeout_triggered.store(true, Ordering::SeqCst);
                cancellation_signal.cancel();
                let _ = isolate_handle.terminate_execution();
            });
            Some(RuntimeInvocationTimeoutController::new(
                watchdog.clone(),
                timeout,
                callback,
            )?)
        };
        if let Some(timeout_controller) = timeout_controller.clone() {
            permit.set_timeout_controller(timeout_controller);
        }
        {
            let op_state = runtime.op_state();
            op_state.borrow_mut().put(permit.clone());
        }

        let result = {
            let isolate_handle = runtime.v8_isolate().thread_safe_handle();
            let cancellation_signal = cancellation_signal.clone();
            let external_cancellation_triggered = external_cancellation_triggered.clone();
            let invoke = async {
                self.load_bundle(&mut runtime, &bundle).await?;
                self.invoke_loaded_bundle(&mut runtime, &request).await
            };
            tokio::pin!(invoke);
            match external_cancellation {
                Some(external_cancellation) => {
                    tokio::select! {
                        result = &mut invoke => result,
                        _ = external_cancellation.cancelled() => {
                            external_cancellation_triggered.store(true, Ordering::SeqCst);
                            cancellation_signal.cancel();
                            let _ = isolate_handle.terminate_execution();
                            invoke.await
                        }
                    }
                }
                None => invoke.await,
            }
        };

        if let Some(timeout_controller) = timeout_controller {
            timeout_controller.disarm().await;
        }
        permit.clear_timeout_controller();
        if let Some(external_cancellation_watchdog) = external_cancellation_watchdog {
            external_cancellation_watchdog.disarm().await;
        }

        let replacement_required = timeout_triggered.load(Ordering::SeqCst)
            || heap_limit_triggered.load(Ordering::SeqCst)
            || external_cancellation_triggered.load(Ordering::SeqCst);

        let result = result.map_err(|error| {
            classify_runtime_error(
                error,
                &timeout_triggered,
                &heap_limit_triggered,
                &external_cancellation_triggered,
                self.policy.limits(),
            )
        });
        if result.is_err()
            && replacement_required
            && let Some(pool) = isolate_pool.as_deref()
        {
            pool.record_replacement(self);
        }
        result
    }

    async fn load_bundle(&self, runtime: &mut JsRuntime, bundle: &RuntimeBundle) -> Result<()> {
        let module_specifier = bundle.module_specifier()?;
        let module_id = runtime
            .load_main_es_module(&module_specifier)
            .await
            .map_err(runtime_js_error)?;
        let evaluation = runtime.mod_evaluate(module_id);
        runtime
            .run_event_loop(Default::default())
            .await
            .map_err(runtime_js_error)?;
        evaluation.await.map_err(runtime_js_error)?;
        Ok(())
    }

    async fn invoke_loaded_bundle(
        &self,
        runtime: &mut JsRuntime,
        request: &InvocationRequest,
    ) -> Result<Value> {
        let request_json = serde_json::to_string(request)?;
        let expression = format!("globalThis.__neovexInvoke({request_json})");
        let value = runtime
            .execute_script("<neovex-runtime:invoke>", expression)
            .map_err(runtime_js_error)?;
        let resolve = runtime.resolve(value);
        let value = runtime
            .with_event_loop_promise(resolve, PollEventLoopOptions::default())
            .await
            .map_err(runtime_js_error)?;
        deserialize_json_value(runtime, value)
    }

    fn bootstrap_snapshot(&self) -> Result<&'static RuntimeStartupSnapshot> {
        static BOOTSTRAP_SNAPSHOT: OnceLock<std::result::Result<RuntimeStartupSnapshot, String>> =
            OnceLock::new();
        match BOOTSTRAP_SNAPSHOT
            .get_or_init(|| Self::create_bootstrap_snapshot().map_err(|error| error.to_string()))
        {
            Ok(snapshot) => Ok(snapshot),
            Err(message) => Err(NeovexRuntimeError::Contract(format!(
                "failed to initialize runtime bootstrap snapshot: {message}"
            ))),
        }
    }

    fn create_bootstrap_snapshot() -> Result<RuntimeStartupSnapshot> {
        bootstrap::create_bootstrap_snapshot()
    }

    fn create_runtime_from_snapshot(
        &self,
        bundle: &RuntimeBundle,
        snapshot: &RuntimeStartupSnapshot,
    ) -> Result<JsRuntime> {
        self.create_runtime(bundle, Some(snapshot))
    }

    fn create_runtime(
        &self,
        bundle: &RuntimeBundle,
        startup_snapshot: Option<&RuntimeStartupSnapshot>,
    ) -> Result<JsRuntime> {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            create_params: Some(self.create_isolate_params()),
            module_loader: Some(Rc::new(SandboxedModuleLoader::new(bundle.module_root()?))),
            extensions: vec![bootstrap::runtime_extension()],
            startup_snapshot: startup_snapshot.map(RuntimeStartupSnapshot::as_startup_snapshot),
            ..Default::default()
        });
        self.initialize_runtime_state(&mut runtime);
        if startup_snapshot.is_none() {
            Self::install_bootstrap(&mut runtime)?;
        }
        Self::finalize_bootstrap(&mut runtime)?;
        Ok(runtime)
    }

    fn create_isolate_params(&self) -> v8::CreateParams {
        let heap_megabyte = 1usize << 20;
        v8::Isolate::create_params().heap_limits(
            self.policy.limits().initial_heap_mb * heap_megabyte,
            self.policy.limits().max_heap_mb * heap_megabyte,
        )
    }

    fn initialize_runtime_state(&self, runtime: &mut JsRuntime) {
        bootstrap::initialize_runtime_state(runtime, self);
    }

    fn install_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
        bootstrap::install_bootstrap(runtime)
    }

    fn finalize_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
        bootstrap::finalize_bootstrap(runtime)
    }
}

#[cfg(test)]
pub(crate) fn bootstrap_snapshot_build_count_for_test() -> usize {
    bootstrap::bootstrap_snapshot_build_count_for_test()
}

fn deserialize_json_value(runtime: &mut JsRuntime, value: v8::Global<v8::Value>) -> Result<Value> {
    scope!(scope, runtime);
    let local = v8::Local::new(scope, value);
    serde_v8::from_v8(scope, local)
        .map_err(|error| NeovexRuntimeError::JavaScript(error.to_string()))
}

fn runtime_js_error(error: impl std::fmt::Display) -> NeovexRuntimeError {
    NeovexRuntimeError::JavaScript(error.to_string())
}

fn classify_runtime_error(
    error: NeovexRuntimeError,
    timeout_triggered: &AtomicBool,
    heap_limit_triggered: &AtomicBool,
    external_cancellation_triggered: &AtomicBool,
    limits: &RuntimeLimits,
) -> NeovexRuntimeError {
    match error {
        NeovexRuntimeError::JavaScript(message)
            if heap_limit_triggered.load(Ordering::SeqCst)
                && is_execution_terminated_error(&message) =>
        {
            NeovexRuntimeError::HeapLimitExceeded(limits.max_heap_mb)
        }
        NeovexRuntimeError::JavaScript(message) if is_host_call_canceled_error(&message) => {
            NeovexRuntimeError::Cancelled
        }
        NeovexRuntimeError::JavaScript(_message)
            if external_cancellation_triggered.load(Ordering::SeqCst) =>
        {
            NeovexRuntimeError::Cancelled
        }
        NeovexRuntimeError::JavaScript(_message) if timeout_triggered.load(Ordering::SeqCst) => {
            NeovexRuntimeError::ExecutionTimeout(limits.execution_timeout)
        }
        other => other,
    }
}

fn is_execution_terminated_error(message: &str) -> bool {
    message.contains("execution terminated")
}

fn is_host_call_canceled_error(message: &str) -> bool {
    message.contains("runtime host call canceled")
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use serde_json::Map;
    use tempfile::tempdir;

    use super::*;
    use crate::host::{HostBridgeFuture, HostCallCancellation, HostCallOperation, HostCallRequest};

    #[derive(Default)]
    struct RecordingHost {
        calls: Mutex<Vec<HostCallRequest>>,
    }

    impl HostBridge for RecordingHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            self.calls
                .lock()
                .expect("recording host lock should not be poisoned")
                .push(request.clone());
            Ok(serde_json::json!({
                "operation": request.operation,
                "payload": request.payload,
            }))
        }
    }

    struct SlowEnvelopeHost {
        delay: std::time::Duration,
    }

    impl HostBridge for SlowEnvelopeHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            std::thread::sleep(self.delay);
            Ok(serde_json::json!({
                "status": "ok",
                "value": Value::Null,
            }))
        }
    }

    struct AsyncOnlyHost;

    impl HostBridge for AsyncOnlyHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(NeovexRuntimeError::Contract(
                "sync host bridge path should not be used for async ops".to_string(),
            ))
        }

        fn call_async(
            &self,
            _request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            Box::pin(async move {
                Ok(serde_json::json!({
                    "status": "ok",
                    "value": "async-host",
                }))
            })
        }
    }

    struct AsyncEchoHost;

    impl HostBridge for AsyncEchoHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(NeovexRuntimeError::Contract(
                "sync host bridge path should not be used for async ops".to_string(),
            ))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            Box::pin(async move {
                Ok(serde_json::json!({
                    "status": "ok",
                    "value": {
                        "operation": request.operation,
                        "payload": request.payload,
                    },
                }))
            })
        }
    }

    #[derive(Default)]
    struct PaginateHost {
        sync_calls: Mutex<Vec<HostCallRequest>>,
        async_calls: Mutex<Vec<HostCallRequest>>,
    }

    impl HostBridge for PaginateHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            self.sync_calls
                .lock()
                .expect("paginate host sync lock should not be poisoned")
                .push(request.clone());
            let value = match request.operation {
                HostCallOperation::CtxDbQueryStart => Value::String("builder-1".to_string()),
                _ => Value::Null,
            };
            Ok(serde_json::json!({
                "status": "ok",
                "value": value,
            }))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            self.async_calls
                .lock()
                .expect("paginate host async lock should not be poisoned")
                .push(request.clone());
            Box::pin(async move {
                Ok(serde_json::json!({
                    "status": "ok",
                    "value": {
                        "data": [
                            { "body": "hello" }
                        ],
                        "has_more": false,
                        "next_cursor": Value::Null,
                    },
                }))
            })
        }
    }

    #[derive(Default)]
    struct PaginateContinuationHost;

    impl HostBridge for PaginateContinuationHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            let value = match request.operation {
                HostCallOperation::CtxDbQueryStart => Value::String("builder-1".to_string()),
                _ => Value::Null,
            };
            Ok(serde_json::json!({
                "status": "ok",
                "value": value,
            }))
        }

        fn call_async(
            &self,
            _request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            Box::pin(async move {
                Ok(serde_json::json!({
                    "status": "ok",
                    "value": {
                        "data": [
                            { "body": "beta" }
                        ],
                        "has_more": false,
                        "next_cursor": "after-beta",
                    },
                }))
            })
        }
    }

    #[derive(Default)]
    struct SyncOnlyHost {
        calls: Mutex<Vec<HostCallRequest>>,
    }

    impl HostBridge for SyncOnlyHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            self.calls
                .lock()
                .expect("sync-only host lock should not be poisoned")
                .push(request.clone());
            let value = match request.operation {
                HostCallOperation::CtxDbQueryStart => Value::String("builder-1".to_string()),
                _ => Value::Null,
            };
            Ok(serde_json::json!({
                "status": "ok",
                "value": value,
            }))
        }

        fn call_async(
            &self,
            _request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            Box::pin(async move {
                Err(NeovexRuntimeError::Contract(
                    "async host bridge path should not be used for sync ops".to_string(),
                ))
            })
        }
    }

    async fn invoke_on_single_worker(
        executor: &RuntimeExecutor,
        runtime: NeovexRuntime,
        bundle: &RuntimeBundle,
        request: InvocationRequest,
    ) -> Result<Value> {
        executor
            .invoke_on_worker(
                runtime,
                bundle.clone(),
                request.clone(),
                RuntimeInvocationContext::top_level(&request),
                None,
            )
            .await
    }

    fn test_invocation_auth(token_identifier: &str) -> InvocationAuth {
        InvocationAuth {
            identity: Some(RuntimeUserIdentity {
                token_identifier: token_identifier.to_string(),
                subject: token_identifier.to_string(),
                issuer: "https://issuer.example.com".to_string(),
                name: None,
                given_name: None,
                family_name: None,
                nickname: None,
                preferred_username: None,
                profile_url: None,
                picture_url: None,
                email: None,
                email_verified: None,
                gender: None,
                birthday: None,
                timezone: None,
                language: None,
                phone_number: None,
                phone_number_verified: None,
                address: None,
                updated_at: None,
                custom_claims: Map::new(),
            }),
            verified_identity: None,
            throw_on_missing_identity: false,
        }
    }

    #[tokio::test]
    async fn runtime_loads_bundle_and_invokes_host_bridge() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const host = await ctx.db.get("messages", "doc-1");
  return {
    ok: true,
    host,
  };
};

export {};
"#,
        )
        .expect("bundle should write");

        let host = Arc::new(RecordingHost::default());
        let runtime = NeovexRuntime::new(host.clone());
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: serde_json::json!({ "author": "Ada" }),
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("bundle invocation should succeed");

        assert_eq!(
            result,
            serde_json::json!({
                "ok": true,
                "host": {
                    "operation": "convex.ctx.db.get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "query:messages:list",
                    }
                }
            })
        );

        let calls = host
            .calls
            .lock()
            .expect("recording host lock should not be poisoned")
            .clone();
        assert_eq!(
            calls,
            vec![HostCallRequest {
                operation: HostCallOperation::CtxDbGet,
                payload: serde_json::json!({
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "query:messages:list",
                }),
            }]
        );
    }

    #[tokio::test]
    async fn runtime_requires_bundle_contract() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(&bundle_path, "export const noop = 1;").expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let error = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Action,
                    function_name: "messages:missing".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("missing global invoke contract should fail");

        assert!(
            error.to_string().contains("__neovexInvoke"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn runtime_awaits_async_bundle_handlers() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const value = await ctx.db.get("messages", "doc-1");
  return {
    ok: true,
    awaited: await Promise.resolve({
      operation: value.operation,
      payload: value.payload,
    }),
  };
};

export {};
"#,
        )
        .expect("bundle should write");

        let host = Arc::new(RecordingHost::default());
        let runtime = NeovexRuntime::new(host);
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("async bundle invocation should succeed");

        assert_eq!(
            result,
            serde_json::json!({
                "ok": true,
                "awaited": {
                    "operation": "convex.ctx.db.get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "query:messages:list",
                    }
                }
            })
        );
    }

    #[tokio::test]
    async fn runtime_does_not_expose_legacy_host_globals() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  return {
    rawHostCall: typeof globalThis.__neovexRawHostCall,
    hostValue: typeof globalThis.__neovexHostValue,
  };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("bundle should observe runtime globals");

        assert_eq!(
            result,
            serde_json::json!({
                "rawHostCall": "undefined",
                "hostValue": "undefined",
            })
        );
    }

    #[tokio::test]
    async fn runtime_removes_deno_global_from_bundle_execution() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
if (typeof Deno !== "undefined") {
  throw new Error("Deno should not be exposed to runtime bundles");
}

globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("bundle should execute without exposing Deno");

        assert_eq!(result, serde_json::json!({ "ok": true }));
    }

    #[tokio::test]
    async fn runtime_times_out_infinite_loops() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  while (true) {}
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::with_limits(
            Arc::new(RecordingHost::default()),
            RuntimeLimits {
                execution_timeout: std::time::Duration::from_millis(50),
                ..RuntimeLimits::default()
            },
        );
        let error = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("infinite loop should time out");

        match error {
            NeovexRuntimeError::ExecutionTimeout(timeout) => {
                assert_eq!(timeout, std::time::Duration::from_millis(50));
            }
            other => panic!("unexpected timeout error: {other}"),
        }
        assert_eq!(runtime.policy.metrics_snapshot().timed_out_invocations, 1);
    }

    #[tokio::test]
    async fn runtime_external_cancellation_stops_infinite_loops() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  while (true) {}
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::with_limits(
            Arc::new(RecordingHost::default()),
            RuntimeLimits {
                execution_timeout: std::time::Duration::from_secs(5),
                ..RuntimeLimits::default()
            },
        );
        let cancellation = HostCallCancellation::default();
        let cancellation_clone = cancellation.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            cancellation_clone.cancel();
        });

        let error = runtime
            .invoke_bundle_with_cancellation(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
                Some(cancellation),
            )
            .await
            .expect_err("external cancellation should stop the runtime invocation");

        assert!(matches!(error, NeovexRuntimeError::Cancelled));
    }

    #[tokio::test]
    async fn runtime_times_out_slow_async_host_ops() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  await ctx.db.get("messages", "doc-1");
  return { ok: true };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::with_limits(
            Arc::new(SlowEnvelopeHost {
                delay: std::time::Duration::from_secs(1),
            }),
            RuntimeLimits {
                execution_timeout: std::time::Duration::from_millis(50),
                ..RuntimeLimits::default()
            },
        );
        let error = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("slow async host op should time out");

        match error {
            NeovexRuntimeError::ExecutionTimeout(timeout) => {
                assert_eq!(timeout, std::time::Duration::from_millis(50));
            }
            other => panic!("unexpected timeout error: {other}"),
        }
    }

    #[tokio::test]
    async fn runtime_async_ops_use_async_host_bridge_path() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const value = await ctx.db.get("messages", "doc-1");
  return { value };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(AsyncOnlyHost));
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("async host bridge should satisfy async op");

        assert_eq!(result, serde_json::json!({ "value": "async-host" }));
    }

    #[tokio::test]
    async fn runtime_exposes_verified_identity_extension_separately_from_convex_identity() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const request = arguments[0];
  const ctx = globalThis.__neovexCreateContext({ request });
  return {
    user: await ctx.auth.getUserIdentity(),
    verified: await ctx.auth.getVerifiedIdentity(),
  };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "auth:whoami".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: Some(InvocationAuth::with_identities(
                        RuntimeUserIdentity {
                            token_identifier: "https://issuer.example.com|user-123".to_string(),
                            subject: "user-123".to_string(),
                            issuer: "https://issuer.example.com".to_string(),
                            name: None,
                            given_name: None,
                            family_name: None,
                            nickname: None,
                            preferred_username: None,
                            profile_url: None,
                            picture_url: None,
                            email: None,
                            email_verified: None,
                            gender: None,
                            birthday: None,
                            timezone: None,
                            language: None,
                            phone_number: None,
                            phone_number_verified: None,
                            address: None,
                            updated_at: None,
                            custom_claims: serde_json::from_value(serde_json::json!({
                                "email": "ada@example.com",
                                "given_name": "Ada",
                                "updated_at": 1710000000,
                                "address.formatted": "123 Analytical Engine Way",
                                "role": "admin"
                            }))
                            .expect("custom jwt compat claims should parse"),
                        },
                        VerifiedUserIdentity {
                            kind: VerifiedUserIdentityKind::CustomJwt,
                            token_identifier: "https://issuer.example.com|user-123".to_string(),
                            subject: "user-123".to_string(),
                            issuer: "https://issuer.example.com".to_string(),
                            name: Some("Ada Lovelace".to_string()),
                            given_name: Some("Ada".to_string()),
                            family_name: None,
                            nickname: None,
                            preferred_username: None,
                            profile_url: None,
                            picture_url: None,
                            email: Some("ada@example.com".to_string()),
                            email_verified: None,
                            gender: None,
                            birthday: None,
                            timezone: None,
                            language: None,
                            phone_number: None,
                            phone_number_verified: None,
                            address: Some("123 Analytical Engine Way".to_string()),
                            updated_at: Some("1710000000".to_string()),
                            custom_claims: serde_json::from_value(serde_json::json!({
                                "role": "admin"
                            }))
                            .expect("verified custom claims should parse"),
                        },
                        false,
                    )),
                },
            )
            .await
            .expect("runtime should expose both auth views");

        assert_eq!(
            result,
            serde_json::json!({
                "user": {
                    "tokenIdentifier": "https://issuer.example.com|user-123",
                    "subject": "user-123",
                    "issuer": "https://issuer.example.com",
                    "email": "ada@example.com",
                    "given_name": "Ada",
                    "updated_at": 1710000000,
                    "address.formatted": "123 Analytical Engine Way",
                    "role": "admin"
                },
                "verified": {
                    "kind": "custom_jwt",
                    "tokenIdentifier": "https://issuer.example.com|user-123",
                    "subject": "user-123",
                    "issuer": "https://issuer.example.com",
                    "name": "Ada Lovelace",
                    "givenName": "Ada",
                    "email": "ada@example.com",
                    "address": "123 Analytical Engine Way",
                    "updatedAt": "1710000000",
                    "role": "admin"
                }
            })
        );
    }

    #[tokio::test]
    async fn pooled_runtime_invocations_keep_module_state_fresh() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__moduleLoadCount = (globalThis.__moduleLoadCount ?? 0) + 1;

globalThis.__neovexInvoke = async function () {
  return { moduleLoadCount: globalThis.__moduleLoadCount };
};

export {};
"#,
        )
        .expect("bundle should write");

        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let runtime = NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
        let bundle = RuntimeBundle::new(&bundle_path);
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:list".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        };

        let first = invoke_on_single_worker(&executor, runtime.clone(), &bundle, request.clone())
            .await
            .expect("first pooled invocation should succeed");
        let second = invoke_on_single_worker(&executor, runtime, &bundle, request)
            .await
            .expect("second pooled invocation should succeed");

        assert_eq!(first, serde_json::json!({ "moduleLoadCount": 1 }));
        assert_eq!(second, serde_json::json!({ "moduleLoadCount": 1 }));
        let metrics = executor.policy().metrics_snapshot();
        assert_eq!(metrics.isolate_pool_misses, 1);
        assert_eq!(metrics.isolate_pool_hits, 1);
    }

    #[tokio::test]
    async fn convenience_runtime_invocations_reuse_runtime_owned_executor() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__moduleLoadCount = (globalThis.__moduleLoadCount ?? 0) + 1;

globalThis.__neovexInvoke = async function () {
  return { moduleLoadCount: globalThis.__moduleLoadCount };
};

export {};
"#,
        )
        .expect("bundle should write");

        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let runtime =
            NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy.clone());
        let bundle = RuntimeBundle::new(&bundle_path);
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:list".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        };

        let first = runtime
            .invoke_bundle(&bundle, &request)
            .await
            .expect("first convenience invocation should succeed");
        let second = runtime
            .invoke_bundle(&bundle, &request)
            .await
            .expect("second convenience invocation should succeed");

        assert_eq!(first, serde_json::json!({ "moduleLoadCount": 1 }));
        assert_eq!(second, serde_json::json!({ "moduleLoadCount": 1 }));
        let metrics = policy.metrics_snapshot();
        assert_eq!(metrics.isolate_pool_misses, 1);
        assert_eq!(metrics.isolate_pool_hits, 1);
        assert_eq!(metrics.isolate_pool_replacements, 0);
    }

    #[tokio::test]
    async fn pooled_runtime_invocations_reset_auth_and_session_state() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({ request });
  const user = await ctx.auth.getUserIdentity();
  const host = await ctx.db.get("messages", "doc-1");
  return {
    token: user?.tokenIdentifier ?? null,
    session: host.payload.session_id,
  };
};

export {};
"#,
        )
        .expect("bundle should write");

        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let runtime = NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
        let bundle = RuntimeBundle::new(&bundle_path);

        let first = invoke_on_single_worker(
            &executor,
            runtime.clone(),
            &bundle,
            InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "auth:first".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: Some(test_invocation_auth("token-1")),
            },
        )
        .await
        .expect("first pooled invocation should succeed");
        let second = invoke_on_single_worker(
            &executor,
            runtime,
            &bundle,
            InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "auth:second".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: Some(test_invocation_auth("token-2")),
            },
        )
        .await
        .expect("second pooled invocation should succeed");

        assert_eq!(
            first,
            serde_json::json!({
                "token": "token-1",
                "session": "session-1",
            })
        );
        assert_eq!(
            second,
            serde_json::json!({
                "token": "token-2",
                "session": "session-1",
            })
        );
        let metrics = executor.policy().metrics_snapshot();
        assert_eq!(metrics.isolate_pool_misses, 1);
        assert_eq!(metrics.isolate_pool_hits, 1);
        assert_eq!(metrics.isolate_pool_replacements, 0);
    }

    #[tokio::test]
    async fn runtime_query_builder_setup_uses_sync_host_bridge_path() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const builder = ctx
    .db
    .query("messages")
    .withIndex("by_author", (q) => q.eq(q.field("author"), "Ada"))
    .filter((q) => q.eq(q.field("channel"), "general"))
    .order("desc");
  return { builderId: builder.__builderId };
};

export {};
"#,
        )
        .expect("bundle should write");

        let host = Arc::new(SyncOnlyHost::default());
        let runtime = NeovexRuntime::new(host.clone());
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("sync host bridge should satisfy query builder setup");

        assert_eq!(result, serde_json::json!({ "builderId": "builder-1" }));
        let calls = host
            .calls
            .lock()
            .expect("sync-only host lock should not be poisoned")
            .clone();
        assert_eq!(
            calls
                .into_iter()
                .map(|call| call.operation)
                .collect::<Vec<_>>(),
            vec![
                HostCallOperation::CtxDbQueryStart,
                HostCallOperation::CtxDbQueryWithIndex,
                HostCallOperation::CtxDbQueryFilter,
                HostCallOperation::CtxDbQueryOrder,
            ]
        );
    }

    #[tokio::test]
    async fn runtime_async_write_and_scheduler_ops_use_async_host_bridge_path() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const insert = await ctx.db.insert("messages", { body: "hello" });
  const patch = await ctx.db.patch("messages", "doc-1", { body: "updated" });
  const deletion = await ctx.db.delete("messages", "doc-1");
  const runAfter = await ctx.scheduler.runAfter(
    100,
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "scheduled" },
  );
  const runAt = await ctx.scheduler.runAt(
    500,
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "scheduled-at" },
  );
  const cancel = await ctx.scheduler.cancel("job-1");
  return { insert, patch, deletion, runAfter, runAt, cancel };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(AsyncEchoHost));
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Mutation,
                    function_name: "messages:write".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("async host bridge should satisfy write and scheduler ops");

        assert_eq!(
            result,
            serde_json::json!({
                "insert": {
                    "operation": "convex.ctx.db.insert",
                    "payload": {
                        "table": "messages",
                        "fields": { "body": "hello" },
                        "session_id": "session-1",
                    }
                },
                "patch": {
                    "operation": "convex.ctx.db.patch",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "patch": { "body": "updated" },
                        "session_id": "session-1",
                    }
                },
                "deletion": {
                    "operation": "convex.ctx.db.delete",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "session-1",
                    }
                },
                "runAfter": {
                    "operation": "convex.ctx.scheduler.run_after",
                    "payload": {
                        "delay_ms": 100,
                        "name": "messages:storeInternal",
                        "visibility": "internal",
                        "args": { "body": "scheduled" },
                        "session_id": "session-1",
                    }
                },
                "runAt": {
                    "operation": "convex.ctx.scheduler.run_at",
                    "payload": {
                        "timestamp_ms": 500,
                        "name": "messages:storeInternal",
                        "visibility": "internal",
                        "args": { "body": "scheduled-at" },
                        "session_id": "session-1",
                    }
                },
                "cancel": {
                    "operation": "convex.ctx.scheduler.cancel",
                    "payload": {
                        "job_id": "job-1",
                        "session_id": "session-1",
                    }
                }
            })
        );
    }

    #[tokio::test]
    async fn runtime_query_paginate_uses_async_host_bridge_and_returns_official_shape() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  return await ctx.db.query("messages").paginate({
    numItems: 2,
    cursor: null,
    maximumRowsRead: 32,
  });
};

export {};
"#,
        )
        .expect("bundle should write");

        let host = Arc::new(PaginateHost::default());
        let runtime = NeovexRuntime::new(host.clone());
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:listPage".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("paginate query should succeed");

        assert_eq!(
            result,
            serde_json::json!({
                "page": [
                    { "body": "hello" }
                ],
                "isDone": true,
                "continueCursor": "",
                "splitCursor": null,
                "pageStatus": null,
            })
        );

        let sync_calls = host
            .sync_calls
            .lock()
            .expect("paginate host sync lock should not be poisoned")
            .clone();
        assert_eq!(sync_calls.len(), 1);
        assert_eq!(sync_calls[0].operation, HostCallOperation::CtxDbQueryStart);

        let async_calls = host
            .async_calls
            .lock()
            .expect("paginate host async lock should not be poisoned")
            .clone();
        assert_eq!(async_calls.len(), 1);
        assert_eq!(
            async_calls[0].operation,
            HostCallOperation::CtxDbQueryPaginate
        );
        assert_eq!(
            async_calls[0].payload,
            serde_json::json!({
                "builder_id": "builder-1",
                "page_size": 2,
                "cursor": Value::Null,
                "session_id": "session-1",
            })
        );
    }

    #[tokio::test]
    async fn runtime_query_paginate_treats_full_page_with_cursor_as_not_done() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  return await ctx.db.query("messages").paginate({
    numItems: 1,
    cursor: "after-alpha",
  });
};

export {};
"#,
        )
        .expect("bundle should write");

        let host = Arc::new(PaginateContinuationHost);
        let runtime = NeovexRuntime::new(host);
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:listPage".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("paginate query should succeed");

        assert_eq!(
            result,
            serde_json::json!({
                "page": [
                    { "body": "beta" }
                ],
                "isDone": false,
                "continueCursor": "after-beta",
                "splitCursor": null,
                "pageStatus": null,
            })
        );
    }

    #[tokio::test]
    async fn runtime_same_isolate_nested_entry_uses_sync_host_bridge_path() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvokeNamedLocal = async function () {
  return "local-ok";
};

globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  return await ctx.runQuery(
    { name: "messages:list", visibility: "public" },
    { author: "Ada" },
  );
};

export {};
"#,
        )
        .expect("bundle should write");

        let host = Arc::new(SyncOnlyHost::default());
        let runtime = NeovexRuntime::new(host.clone());
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:outer".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("same-isolate nested entry should succeed");

        assert_eq!(result, serde_json::json!("local-ok"));
        let calls = host
            .calls
            .lock()
            .expect("sync-only host lock should not be poisoned")
            .clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].operation,
            HostCallOperation::CtxRuntimeEnterNestedCall
        );
        assert_eq!(
            calls[0].payload,
            serde_json::json!({
                "name": "messages:list",
                "visibility": "public",
                "session_id": "session-1",
            })
        );
    }

    #[tokio::test]
    async fn runtime_async_ctx_run_ops_use_async_host_bridge_path() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const query = await ctx.runQuery(
    { name: "messages:list", visibility: "public" },
    { author: "Ada" },
  );
  const mutation = await ctx.runMutation(
    { name: "messages:storeInternal", visibility: "internal" },
    { body: "hello" },
  );
  const action = await ctx.runAction(
    { name: "messages:sendViaAction", visibility: "public" },
    { body: "wave" },
  );
  return { query, mutation, action };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(AsyncEchoHost));
        let result = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Action,
                    function_name: "messages:outer".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect("async host bridge should satisfy ctx.run* fallback ops");

        assert_eq!(
            result,
            serde_json::json!({
                "query": {
                    "operation": "convex.ctx.run_query",
                    "payload": {
                        "name": "messages:list",
                        "visibility": "public",
                        "args": { "author": "Ada" },
                        "session_id": "session-1",
                    }
                },
                "mutation": {
                    "operation": "convex.ctx.run_mutation",
                    "payload": {
                        "name": "messages:storeInternal",
                        "visibility": "internal",
                        "args": { "body": "hello" },
                        "session_id": "session-1",
                    }
                },
                "action": {
                    "operation": "convex.ctx.run_action",
                    "payload": {
                        "name": "messages:sendViaAction",
                        "visibility": "public",
                        "args": { "body": "wave" },
                        "session_id": "session-1",
                    }
                }
            })
        );
    }

    #[tokio::test]
    async fn runtime_reports_heap_limit_exceeded() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  let value = "";
  while (true) {
    value += "hello world";
  }
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::with_limits(
            Arc::new(RecordingHost::default()),
            RuntimeLimits {
                max_heap_mb: 8,
                initial_heap_mb: 4,
                execution_timeout: std::time::Duration::from_secs(2),
                max_concurrent_isolates: 1,
                worker_threads: RuntimeLimits::default().worker_threads,
                max_active_top_level_invocations_per_tenant: RuntimeLimits::default()
                    .max_active_top_level_invocations_per_tenant,
                max_in_flight_top_level_invocations_per_tenant: RuntimeLimits::default()
                    .max_in_flight_top_level_invocations_per_tenant,
                max_queued_top_level_invocations_per_tenant: RuntimeLimits::default()
                    .max_queued_top_level_invocations_per_tenant,
                max_nested_runtime_invocations: RuntimeLimits::default()
                    .max_nested_runtime_invocations,
            },
        );
        let error = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("heap growth should trip the runtime heap limit");

        match error {
            NeovexRuntimeError::HeapLimitExceeded(limit) => assert_eq!(limit, 8),
            other => panic!("unexpected heap-limit error: {other}"),
        }
    }

    #[tokio::test]
    async fn runtime_rejects_module_imports_outside_bundle_root() {
        let tempdir = tempdir().expect("tempdir should build");
        let outside_path = tempdir.path().join("outside.mjs");
        let bundle_dir = tempdir.path().join("bundle");
        std::fs::create_dir_all(&bundle_dir).expect("bundle dir should exist");
        let bundle_path = bundle_dir.join("bundle.mjs");

        std::fs::write(&outside_path, "export const secret = 'outside';")
            .expect("outside module should write");
        std::fs::write(
            &bundle_path,
            r#"
import "../outside.mjs";

globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let error = runtime
            .invoke_bundle(
                &RuntimeBundle::new(&bundle_path),
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("outside import should be rejected");

        assert!(
            error.to_string().contains("outside the bundle root"),
            "unexpected loader sandbox error: {error}"
        );
    }

    #[tokio::test]
    async fn runtime_rejects_bundle_integrity_mismatch() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
        )
        .expect("bundle should write");
        let expected_sha256 =
            RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  return { ok: false };
};

export {};
"#,
        )
        .expect("tampered bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let bundle = RuntimeBundle::with_expected_sha256(&bundle_path, expected_sha256)
            .expect("bundle integrity metadata should build");
        let error = runtime
            .invoke_bundle(
                &bundle,
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:list".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                },
            )
            .await
            .expect_err("tampered bundle should fail integrity verification");

        match error {
            NeovexRuntimeError::BundleIntegrityMismatch(message) => {
                assert!(message.contains("bundle.mjs"));
            }
            other => panic!("unexpected integrity error: {other}"),
        }
    }

    #[test]
    fn runtime_bundle_clones_share_normalized_identity() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
        )
        .expect("bundle should write");

        let expected_sha256 =
            RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
        let bundle = RuntimeBundle::with_expected_sha256(
            bundle_path
                .parent()
                .expect("bundle parent should exist")
                .join(".")
                .join("bundle.mjs"),
            expected_sha256.to_ascii_uppercase(),
        )
        .expect("bundle identity metadata should build");
        let cloned = bundle.clone();
        let canonical_bundle_path = bundle_path
            .canonicalize()
            .expect("bundle path should canonicalize");

        assert!(bundle.shares_storage_with(&cloned));
        assert_eq!(bundle.bundle_identity(), cloned.bundle_identity());
        assert_eq!(bundle.bundle_identity().entrypoint(), canonical_bundle_path);
        assert_eq!(
            bundle.bundle_identity().expected_sha256(),
            Some(expected_sha256.as_str())
        );
        assert_eq!(
            bundle.canonical_entrypoint(),
            Some(canonical_bundle_path.as_path())
        );
    }

    #[tokio::test]
    async fn runtime_bundle_rechecks_integrity_after_prior_success() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
        )
        .expect("bundle should write");

        let expected_sha256 =
            RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
        let bundle = RuntimeBundle::with_expected_sha256(&bundle_path, expected_sha256)
            .expect("bundle integrity metadata should build");
        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:list".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        };

        let first_result = runtime
            .invoke_bundle(&bundle, &request)
            .await
            .expect("first bundle invocation should succeed");
        assert_eq!(first_result, serde_json::json!({ "ok": true }));

        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  return { ok: false };
};

export {};
"#,
        )
        .expect("tampered bundle should write");

        let error = runtime
            .invoke_bundle(&bundle, &request)
            .await
            .expect_err("tampered bundle should fail integrity verification");
        assert!(matches!(
            error,
            NeovexRuntimeError::BundleIntegrityMismatch(_)
        ));
    }

    #[tokio::test]
    async fn runtime_bundle_identity_canonicalizes_paths_without_changing_integrity_results() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
        )
        .expect("bundle should write");

        let expected_sha256 =
            RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
        let canonical_bundle = RuntimeBundle::with_expected_sha256(&bundle_path, &expected_sha256)
            .expect("canonical bundle should build");
        let dot_path_bundle = RuntimeBundle::with_expected_sha256(
            bundle_path
                .parent()
                .expect("bundle parent should exist")
                .join(".")
                .join("bundle.mjs"),
            format!("{expected_sha256}\n"),
        )
        .expect("dot path bundle should build");
        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:list".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        };

        assert_eq!(
            canonical_bundle.bundle_identity(),
            dot_path_bundle.bundle_identity()
        );

        let canonical_result = runtime
            .invoke_bundle(&canonical_bundle, &request)
            .await
            .expect("canonical bundle invocation should succeed");
        let dot_path_result = runtime
            .invoke_bundle(&dot_path_bundle, &request)
            .await
            .expect("dot path bundle invocation should succeed");

        assert_eq!(canonical_result, serde_json::json!({ "ok": true }));
        assert_eq!(dot_path_result, canonical_result);
    }
}
