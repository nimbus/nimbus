#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use crate::context::RuntimeInvocationContext;
#[cfg(test)]
use crate::error::{NeovexRuntimeError, Result};
#[cfg(test)]
use crate::host::HostCallCancellation;
#[cfg(test)]
use crate::limits::RuntimePolicy;
#[cfg(test)]
use crate::limits::{RuntimeExecutionModel, RuntimePoolKind, RuntimeRoutingAffinity};
#[cfg(test)]
use crate::runtime::{InvocationRequest, NeovexRuntime, RuntimeBundle};

mod admission;
mod facade;
mod invoke;
mod lifecycle;
mod queue;

pub(crate) use self::admission::SharedInvocationPermit;
pub use self::facade::RuntimeExecutor;
#[cfg(test)]
pub(crate) use self::facade::RuntimeExecutorTestState;
pub(crate) use self::lifecycle::run_invocation_lifecycle;
pub(crate) use self::queue::RuntimeWorkerJob;
pub(crate) use self::queue::{RuntimeWorkerQueue, RuntimeWorkerShutdown, WorkerActivitySignal};

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::sync::Mutex as StdMutex;
    use std::sync::{Arc, OnceLock};

    use serde_json::{Value, json};
    use tempfile::tempdir;
    use tokio::sync::{Mutex as TokioMutex, Notify};

    use super::*;
    use crate::host::{HostBridge, HostBridgeFuture, HostCallOperation, HostCallRequest};
    use crate::limits::RuntimeLimits;
    use crate::runtime::RuntimeConstructionMode;

    struct NoopHost;

    impl HostBridge for NoopHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Ok(Value::Null)
        }
    }

    struct WorkerRuntimeIdHost {
        test_state: Arc<RuntimeExecutorTestState>,
    }

    impl HostBridge for WorkerRuntimeIdHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            assert_eq!(request.operation, HostCallOperation::CtxDbGet);
            Ok(json!({
                "workerRuntimeId": self.test_state.worker_runtime_id_for_current_thread(),
            }))
        }
    }

    struct ControlledAsyncWorkerRuntimeIdHost {
        test_state: Arc<RuntimeExecutorTestState>,
        started: StdMutex<std::collections::HashMap<String, usize>>,
        started_notify: Arc<Notify>,
        release_slow: Arc<Notify>,
        release_slow_flag: Arc<std::sync::atomic::AtomicBool>,
    }

    impl ControlledAsyncWorkerRuntimeIdHost {
        fn new(test_state: Arc<RuntimeExecutorTestState>) -> Self {
            Self {
                test_state,
                started: StdMutex::new(std::collections::HashMap::new()),
                started_notify: Arc::new(Notify::new()),
                release_slow: Arc::new(Notify::new()),
                release_slow_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            }
        }

        async fn wait_until_started(&self, document_id: &str) {
            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    let notified = self.started_notify.notified();
                    if self
                        .started
                        .lock()
                        .expect("controlled runtime-id host lock should not be poisoned")
                        .contains_key(document_id)
                    {
                        return;
                    }
                    notified.await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("host request {document_id} should start"));
        }

        fn started_runtime_id(&self, document_id: &str) -> Option<usize> {
            self.started
                .lock()
                .expect("controlled runtime-id host lock should not be poisoned")
                .get(document_id)
                .copied()
        }

        fn release_slow_jobs(&self) {
            self.release_slow_flag
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.release_slow.notify_waiters();
        }
    }

    impl HostBridge for ControlledAsyncWorkerRuntimeIdHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(NeovexRuntimeError::Contract(
                "controlled runtime-id host expects async db.get path".to_string(),
            ))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            let document_id = request
                .payload
                .get("id")
                .and_then(Value::as_str)
                .expect("db.get payload should carry an id")
                .to_string();
            let worker_runtime_id = self
                .test_state
                .worker_runtime_id_for_current_thread()
                .expect("worker runtime id should be registered before async host calls");
            self.started
                .lock()
                .expect("controlled runtime-id host lock should not be poisoned")
                .insert(document_id.clone(), worker_runtime_id);
            self.started_notify.notify_waiters();
            let release_slow = self.release_slow.clone();
            let release_slow_flag = self.release_slow_flag.clone();
            Box::pin(async move {
                if document_id.starts_with("slow-")
                    && !release_slow_flag.load(std::sync::atomic::Ordering::SeqCst)
                {
                    release_slow.notified().await;
                }
                Ok(json!({
                    "status": "ok",
                    "value": {
                        "id": document_id,
                        "workerRuntimeId": worker_runtime_id,
                    },
                }))
            })
        }
    }

    #[derive(Default)]
    struct TenantFairnessHost {
        started_ids: StdMutex<Vec<String>>,
        slow_started: Arc<Notify>,
        release_slow: Arc<Notify>,
    }

    impl TenantFairnessHost {
        fn started_ids(&self) -> Vec<String> {
            self.started_ids
                .lock()
                .expect("tenant fairness host lock should not be poisoned")
                .clone()
        }

        fn release_slow_job(&self) {
            self.release_slow.notify_waiters();
        }
    }

    impl HostBridge for TenantFairnessHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(NeovexRuntimeError::Contract(
                "tenant fairness host expects async db.get path".to_string(),
            ))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            let document_id = request
                .payload
                .get("id")
                .and_then(Value::as_str)
                .expect("db.get payload should carry an id")
                .to_string();
            self.started_ids
                .lock()
                .expect("tenant fairness host lock should not be poisoned")
                .push(document_id.clone());
            let slow_started = self.slow_started.clone();
            let release_slow = self.release_slow.clone();
            Box::pin(async move {
                if document_id == "slow-1" {
                    slow_started.notify_waiters();
                    release_slow.notified().await;
                }
                Ok(json!({
                    "status": "ok",
                    "value": {
                        "id": document_id,
                    },
                }))
            })
        }
    }

    #[derive(Default)]
    struct ControlledAsyncGetHost {
        started_ids: StdMutex<Vec<String>>,
        started_notify: Arc<Notify>,
        release_slow: Arc<Notify>,
        release_slow_flag: Arc<std::sync::atomic::AtomicBool>,
    }

    impl ControlledAsyncGetHost {
        fn started_ids(&self) -> Vec<String> {
            self.started_ids
                .lock()
                .expect("controlled async host lock should not be poisoned")
                .clone()
        }

        async fn wait_until_started(&self, document_id: &str) {
            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    let notified = self.started_notify.notified();
                    if self
                        .started_ids()
                        .iter()
                        .any(|started| started == document_id)
                    {
                        return;
                    }
                    notified.await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("host request {document_id} should start"));
        }

        fn release_slow_jobs(&self) {
            self.release_slow_flag
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.release_slow.notify_waiters();
        }
    }

    impl HostBridge for ControlledAsyncGetHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(NeovexRuntimeError::Contract(
                "controlled async host expects async db.get path".to_string(),
            ))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            let document_id = request
                .payload
                .get("id")
                .and_then(Value::as_str)
                .expect("db.get payload should carry an id")
                .to_string();
            self.started_ids
                .lock()
                .expect("controlled async host lock should not be poisoned")
                .push(document_id.clone());
            self.started_notify.notify_waiters();
            let release_slow = self.release_slow.clone();
            let release_slow_flag = self.release_slow_flag.clone();
            Box::pin(async move {
                if document_id.starts_with("slow-")
                    && !release_slow_flag.load(std::sync::atomic::Ordering::SeqCst)
                {
                    release_slow.notified().await;
                }
                Ok(json!({
                    "status": "ok",
                    "value": {
                        "id": document_id,
                    },
                }))
            })
        }
    }

    #[derive(Clone, Copy)]
    struct DelayedAsyncGetHost {
        delay: Duration,
    }

    impl DelayedAsyncGetHost {
        fn new(delay: Duration) -> Self {
            Self { delay }
        }
    }

    impl HostBridge for DelayedAsyncGetHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(NeovexRuntimeError::Contract(
                "delayed async host expects async db.get path".to_string(),
            ))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            let document_id = request
                .payload
                .get("id")
                .and_then(Value::as_str)
                .expect("db.get payload should carry an id")
                .to_string();
            let delay = self.delay;
            Box::pin(async move {
                tokio::time::sleep(delay).await;
                Ok(json!({
                    "status": "ok",
                    "value": {
                        "id": document_id,
                    },
                }))
            })
        }
    }

    struct SlowSyncQueryHost {
        delay: Duration,
        started: Arc<Notify>,
    }

    impl SlowSyncQueryHost {
        fn new(delay: Duration) -> Self {
            Self {
                delay,
                started: Arc::new(Notify::new()),
            }
        }

        async fn wait_until_started(&self) {
            tokio::time::timeout(Duration::from_secs(1), self.started.notified())
                .await
                .expect("slow sync query host should start");
        }
    }

    impl HostBridge for SlowSyncQueryHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            assert_eq!(request.operation, HostCallOperation::CtxDbQueryStart);
            self.started.notify_waiters();
            std::thread::sleep(self.delay);
            Ok(json!({
                "status": "ok",
                "value": "builder-1",
            }))
        }

        fn call_async(
            &self,
            _request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            Box::pin(async move {
                Err(NeovexRuntimeError::Contract(
                    "async host bridge path should not be used for sync query builder setup"
                        .to_string(),
                ))
            })
        }
    }

    fn write_runtime_id_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
        let bundle_dir = tempdir().expect("tempdir should build");
        let bundle_path = bundle_dir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
        )
        .expect("bundle should write");
        (bundle_dir, bundle_path)
    }

    fn write_busy_loop_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
        let bundle_dir = tempdir().expect("tempdir should build");
        let bundle_path = bundle_dir.path().join("bundle.mjs");
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
        (bundle_dir, bundle_path)
    }

    fn write_function_named_get_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
        let bundle_dir = tempdir().expect("tempdir should build");
        let bundle_path = bundle_dir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  return await ctx.db.get("messages", request.function_name);
};

export {};
"#,
        )
        .expect("bundle should write");
        (bundle_dir, bundle_path)
    }

    fn write_retained_counter_get_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
        let bundle_dir = tempdir().expect("tempdir should build");
        let bundle_path = bundle_dir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function (request) {
  globalThis.__userCounter = (globalThis.__userCounter ?? 0) + 1;
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `session-${globalThis.__userCounter}`,
  });
  const value = await ctx.db.get("messages", request.function_name);
  return {
    counter: globalThis.__userCounter,
    id: value.id,
  };
};

export {};
"#,
        )
        .expect("bundle should write");
        (bundle_dir, bundle_path)
    }

    fn write_sync_query_builder_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
        let bundle_dir = tempdir().expect("tempdir should build");
        let bundle_path = bundle_dir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const builder = ctx.db.query("messages");
  return { builderId: builder.__builderId };
};

export {};
"#,
        )
        .expect("bundle should write");
        (bundle_dir, bundle_path)
    }

    fn write_constant_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
        let bundle_dir = tempdir().expect("tempdir should build");
        let bundle_path = bundle_dir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  return "ok";
};

export {};
"#,
        )
        .expect("bundle should write");
        (bundle_dir, bundle_path)
    }

    fn retained_async_host_batch_policy() -> Arc<RuntimePolicy> {
        Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_model: RuntimeExecutionModel::RunToCompletion,
            runtime_pool_kind: RuntimePoolKind::RetainedJsRuntimePool,
            routing_affinity: RuntimeRoutingAffinity::Tenant,
            max_concurrent_isolates: 1,
            worker_threads: 1,
            max_retained_runtimes_per_worker: 4,
            max_retained_runtimes_per_affinity_key_per_worker: 1,
            ..RuntimeLimits::default()
        }))
    }

    fn init_runtime_repro_tracing() {
        static TRACING_INIT: OnceLock<()> = OnceLock::new();
        TRACING_INIT.get_or_init(|| {
            let _ = tracing_subscriber::fmt()
                .with_test_writer()
                .with_max_level(tracing::Level::DEBUG)
                .without_time()
                .try_init();
        });
    }

    fn stress_env_usize(name: &str, default: usize) -> usize {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(default)
    }

    fn run_retained_async_host_batch_round(
        executor: &RuntimeExecutor,
        policy: &Arc<RuntimePolicy>,
        host: &Arc<DelayedAsyncGetHost>,
        bundle: &RuntimeBundle,
        batch_index: usize,
        construction_mode: RuntimeConstructionMode,
    ) {
        let tenant_labels = ["tenant-a", "tenant-b", "tenant-c", "tenant-d"];
        let mut handles = Vec::with_capacity(tenant_labels.len());
        let mut expected_ids = Vec::with_capacity(tenant_labels.len());
        for (tenant_index, tenant_label) in tenant_labels.iter().enumerate() {
            let executor = executor.clone();
            let runtime = NeovexRuntime::with_policy(host.clone(), policy.clone())
                .with_retained_runtime_construction_mode_for_test(construction_mode);
            let bundle = bundle.clone();
            let function_name = format!("doc-{batch_index}-{tenant_index}");
            let request = test_request(&function_name);
            let request_id = format!("req-retained-async-batch-{batch_index}-{tenant_index}");
            let context = test_context_for_tenant(&request, tenant_label, &request_id);
            expected_ids.push(function_name.clone());
            handles.push(std::thread::spawn(move || {
                executor.invoke_blocking(runtime, bundle, request, context)
            }));
        }

        for (handle, expected_id) in handles.into_iter().zip(expected_ids) {
            let result = handle
                .join()
                .expect("retained async batch caller thread should join")
                .expect("retained async batch invocation should succeed");
            assert_eq!(result, json!({ "id": expected_id }));
        }
    }

    fn test_request(function_name: &str) -> InvocationRequest {
        InvocationRequest {
            kind: crate::runtime::InvocationKind::Query,
            function_name: function_name.to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        }
    }

    fn test_context_for_tenant(
        request: &InvocationRequest,
        tenant_label: &str,
        request_id: &str,
    ) -> RuntimeInvocationContext {
        RuntimeInvocationContext::top_level_for_tenant_and_request(
            request,
            tenant_label,
            request_id,
        )
    }

    fn test_context(request: &InvocationRequest, request_id: &str) -> RuntimeInvocationContext {
        test_context_for_tenant(request, "demo", request_id)
    }

    fn worker_runtime_id(result: &Value) -> usize {
        result
            .get("workerRuntimeId")
            .and_then(Value::as_u64)
            .map(|id| id as usize)
            .expect("result should include a workerRuntimeId")
    }

    fn runtime_executor_test_lock() -> &'static TokioMutex<()> {
        static RUNTIME_EXECUTOR_TEST_LOCK: OnceLock<TokioMutex<()>> = OnceLock::new();
        RUNTIME_EXECUTOR_TEST_LOCK.get_or_init(|| TokioMutex::new(()))
    }

    #[tokio::test]
    async fn pre_canceled_worker_invocation_records_request_correlation() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let policy = Arc::new(RuntimePolicy::default());
        let executor = RuntimeExecutor::new(policy.clone());
        let request = test_request("messages:list");
        let context = test_context(&request, "req-pre-canceled");
        let cancellation = HostCallCancellation::default();
        cancellation.cancel_due_to_disconnect();

        let error = executor
            .invoke_on_worker(
                NeovexRuntime::new(Arc::new(NoopHost)),
                RuntimeBundle::new("unused.mjs"),
                request,
                context,
                Some(cancellation),
            )
            .await
            .expect_err("pre-canceled worker invocation should fail");

        assert!(matches!(error, NeovexRuntimeError::Cancelled));

        let snapshot = policy.metrics_snapshot();
        assert_eq!(snapshot.queued_canceled_invocations, 1);
        assert_eq!(snapshot.disconnect_canceled_invocations, 1);
        assert_eq!(snapshot.recent_request_correlations.len(), 1);
        let correlation = &snapshot.recent_request_correlations[0];
        assert_eq!(correlation.server_request_id, "req-pre-canceled");
        assert_eq!(correlation.function_name, "messages:list");
        assert_eq!(correlation.kind, "query");
        assert_eq!(correlation.tenant_label.as_deref(), Some("demo"));
        assert!(correlation.invocation_id > 0);
    }

    #[tokio::test]
    async fn worker_invocations_reuse_worker_local_tokio_runtime() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let test_state = executor.test_state();
        let host = Arc::new(WorkerRuntimeIdHost {
            test_state: test_state.clone(),
        });
        let request = test_request("messages:list");

        let first_result = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context(&request, "req-worker-1"),
                None,
            )
            .await
            .expect("first worker invocation should succeed");
        let second_result = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host, policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context(&request, "req-worker-2"),
                None,
            )
            .await
            .expect("second worker invocation should succeed");

        assert_eq!(first_result, json!({ "workerRuntimeId": 1 }));
        assert_eq!(second_result, json!({ "workerRuntimeId": 1 }));
        assert_eq!(test_state.worker_runtime_builds(), 1);
        let metrics = executor.policy().metrics_snapshot();
        assert_eq!(metrics.isolate_pool_misses, 1);
        assert_eq!(metrics.isolate_pool_hits, 1);
        assert_eq!(metrics.isolate_pool_replacements, 0);
    }

    #[tokio::test]
    async fn cooperative_execution_model_processes_worker_invocations() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let test_state = executor.test_state();
        let host = Arc::new(WorkerRuntimeIdHost {
            test_state: test_state.clone(),
        });
        let request = test_request("messages:list");

        let first_result = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context(&request, "req-cooperative-1"),
                None,
            )
            .await
            .expect("first cooperative worker invocation should succeed");
        let second_result = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host, policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context(&request, "req-cooperative-2"),
                None,
            )
            .await
            .expect("second cooperative worker invocation should succeed");

        assert_eq!(first_result, json!({ "workerRuntimeId": 1 }));
        assert_eq!(second_result, json!({ "workerRuntimeId": 1 }));
        assert_eq!(test_state.worker_runtime_builds(), 1);

        let metrics = executor.policy().metrics_snapshot();
        assert_eq!(metrics.isolate_pool_misses, 1);
        assert_eq!(metrics.isolate_pool_hits, 1);
        assert_eq!(metrics.isolate_pool_replacements, 0);
    }

    #[tokio::test]
    async fn cooperative_execution_model_retained_runtime_pool_reuses_locker_runtime() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let before_snapshot_builds = crate::runtime::bootstrap_snapshot_build_count_for_test();
        let (_bundle_dir, bundle_path) = write_retained_counter_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            runtime_pool_kind: RuntimePoolKind::RetainedJsRuntimePool,
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(ControlledAsyncGetHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);
        let first_request = test_request("retained-1");
        let second_request = test_request("retained-2");

        let first_result = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                bundle.clone(),
                first_request.clone(),
                test_context(&first_request, "req-cooperative-retained-1"),
                None,
            )
            .await
            .expect("first retained cooperative invocation should succeed");
        let second_result = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host, policy.clone()),
                bundle,
                second_request.clone(),
                test_context(&second_request, "req-cooperative-retained-2"),
                None,
            )
            .await
            .expect("second retained cooperative invocation should reuse the reset runtime");

        assert_eq!(first_result, json!({ "counter": 1, "id": "retained-1" }));
        assert_eq!(second_result, json!({ "counter": 1, "id": "retained-2" }));

        let metrics = executor.policy().metrics_snapshot();
        assert_eq!(metrics.isolate_pool_misses, 1);
        assert_eq!(metrics.isolate_pool_hits, 1);
        assert_eq!(metrics.isolate_pool_replacements, 0);

        let after_snapshot_builds = crate::runtime::bootstrap_snapshot_build_count_for_test();
        assert_eq!(
            after_snapshot_builds, before_snapshot_builds,
            "retained cooperative Locker pooling should reuse unsnapshotted runtimes instead of building the startup snapshot"
        );
        assert!(
            policy
                .limits()
                .reset_capabilities()
                .user_module_state_per_invocation,
            "retained cooperative Locker pooling should now advertise fresh user-module state per invocation"
        );
    }

    #[test]
    fn retained_run_to_completion_async_host_batch_survives_repeated_blocking_batches() {
        let _test_lock = runtime_executor_test_lock().blocking_lock();
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = retained_async_host_batch_policy();
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(DelayedAsyncGetHost::new(Duration::from_millis(1)));
        let bundle = RuntimeBundle::new(&bundle_path);
        let total_batches = 32;

        for batch_index in 0..total_batches {
            run_retained_async_host_batch_round(
                &executor,
                &policy,
                &host,
                &bundle,
                batch_index,
                RuntimeConstructionMode::Unsnapshotted,
            );
        }

        let snapshot = policy.metrics_snapshot();
        let total_invocations = (total_batches * 4) as u64;
        assert_eq!(snapshot.isolate_pool_misses, 1);
        assert_eq!(
            snapshot.isolate_pool_hits,
            total_invocations.saturating_sub(1)
        );
        assert_eq!(
            snapshot.retained_runtime_main_realm_resets,
            snapshot.isolate_pool_hits
        );
        assert_eq!(
            snapshot.retained_runtime_bootstrap_replays,
            snapshot.isolate_pool_hits
        );
        assert_eq!(snapshot.retained_runtime_pool_entries, 1);
        assert_eq!(snapshot.retained_runtime_pool_evictions, 0);
        assert_eq!(snapshot.retained_runtime_pool_retirements, 0);
    }

    #[test]
    fn retained_run_to_completion_async_host_batch_survives_repeated_scenario_rebuilds() {
        let _test_lock = runtime_executor_test_lock().blocking_lock();
        let scenarios = stress_env_usize("NEOVEX_RETAINED_ASYNC_SCENARIOS", 24);
        let measured_batches_per_scenario = stress_env_usize("NEOVEX_RETAINED_ASYNC_BATCHES", 8);

        for scenario_index in 0..scenarios {
            let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
            let policy = retained_async_host_batch_policy();
            let executor = RuntimeExecutor::new(policy.clone());
            let host = Arc::new(DelayedAsyncGetHost::new(Duration::from_millis(1)));
            let bundle = RuntimeBundle::new(&bundle_path);
            let base_batch_index = scenario_index * (measured_batches_per_scenario + 1);

            run_retained_async_host_batch_round(
                &executor,
                &policy,
                &host,
                &bundle,
                base_batch_index,
                RuntimeConstructionMode::Unsnapshotted,
            );
            for local_batch_index in 0..measured_batches_per_scenario {
                run_retained_async_host_batch_round(
                    &executor,
                    &policy,
                    &host,
                    &bundle,
                    base_batch_index + local_batch_index + 1,
                    RuntimeConstructionMode::Unsnapshotted,
                );
            }

            let snapshot = policy.metrics_snapshot();
            let total_invocations = ((measured_batches_per_scenario + 1) * 4) as u64;
            assert_eq!(snapshot.isolate_pool_misses, 1);
            assert_eq!(
                snapshot.isolate_pool_hits,
                total_invocations.saturating_sub(1)
            );
            assert_eq!(
                snapshot.retained_runtime_main_realm_resets,
                snapshot.isolate_pool_hits
            );
            assert_eq!(
                snapshot.retained_runtime_bootstrap_replays,
                snapshot.isolate_pool_hits
            );
            assert_eq!(snapshot.retained_runtime_pool_entries, 1);
            assert_eq!(snapshot.retained_runtime_pool_evictions, 0);
            assert_eq!(snapshot.retained_runtime_pool_retirements, 0);
        }
    }

    #[test]
    #[ignore = "manual repro for snapshot-seeded retained async-host reuse; run with --ignored --nocapture"]
    fn retained_snapshot_seeded_async_host_batch_repro() {
        init_runtime_repro_tracing();
        let _test_lock = runtime_executor_test_lock().blocking_lock();
        let scenarios = stress_env_usize("NEOVEX_RETAINED_ASYNC_SCENARIOS", 24);
        let measured_batches_per_scenario = stress_env_usize("NEOVEX_RETAINED_ASYNC_BATCHES", 8);

        for scenario_index in 0..scenarios {
            let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
            let policy = retained_async_host_batch_policy();
            let executor = RuntimeExecutor::new(policy.clone());
            let host = Arc::new(DelayedAsyncGetHost::new(Duration::from_millis(1)));
            let bundle = RuntimeBundle::new(&bundle_path);
            let base_batch_index = scenario_index * (measured_batches_per_scenario + 1);

            run_retained_async_host_batch_round(
                &executor,
                &policy,
                &host,
                &bundle,
                base_batch_index,
                RuntimeConstructionMode::StartupSnapshot,
            );
            for local_batch_index in 0..measured_batches_per_scenario {
                run_retained_async_host_batch_round(
                    &executor,
                    &policy,
                    &host,
                    &bundle,
                    base_batch_index + local_batch_index + 1,
                    RuntimeConstructionMode::StartupSnapshot,
                );
            }
        }
    }

    #[tokio::test]
    async fn cooperative_execution_model_resumes_parked_invocations_after_host_completion() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(ControlledAsyncGetHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);
        let request = test_request("slow-1");
        let parked_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&request, "tenant-a", "req-cooperative-parked");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        request,
                        context,
                        None,
                    )
                    .await
            }
        });

        host.wait_until_started("slow-1").await;
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if policy.metrics_snapshot().active_isolates == 0 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cooperative invocation should suspend its active isolate while parked");
        tokio::task::yield_now().await;
        assert!(
            !parked_task.is_finished(),
            "cooperative invocation should remain pending until host work completes"
        );

        host.release_slow_jobs();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), parked_task)
                .await
                .expect("cooperative invocation should resume after host completion")
                .expect("cooperative parked task should join")
                .expect("cooperative parked invocation should succeed"),
            json!({ "id": "slow-1" })
        );
    }

    #[tokio::test]
    async fn cooperative_execution_model_startup_snapshot_handles_multiple_parked_runtimes() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(ControlledAsyncGetHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);

        let slow_requests = [
            ("slow-1", "tenant-a", "req-cooperative-slow-1"),
            ("slow-2", "tenant-b", "req-cooperative-slow-2"),
            ("slow-3", "tenant-c", "req-cooperative-slow-3"),
            ("slow-4", "tenant-d", "req-cooperative-slow-4"),
        ];

        let tasks = slow_requests.map(|(function_name, tenant_label, request_id)| {
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let request = test_request(function_name);
            let context = test_context_for_tenant(&request, tenant_label, request_id);
            tokio::spawn(async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        request,
                        context,
                        None,
                    )
                    .await
            })
        });

        for (function_name, _, _) in slow_requests {
            host.wait_until_started(function_name).await;
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let metrics = policy.metrics_snapshot();
                if metrics.active_isolates == 0 && host.started_ids().len() >= slow_requests.len() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("all cooperative invocations should park and release the worker isolate");

        host.release_slow_jobs();

        for (task, (function_name, _, _)) in tasks.into_iter().zip(slow_requests) {
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), task)
                    .await
                    .expect("cooperative parked invocation should resume after host completion")
                    .expect("cooperative parked task should join")
                    .expect("cooperative parked invocation should succeed"),
                json!({ "id": function_name })
            );
        }

        let metrics = policy.metrics_snapshot();
        assert_eq!(metrics.isolate_pool_misses, 1);
        assert_eq!(metrics.isolate_pool_hits, 3);
        assert_eq!(metrics.isolate_pool_replacements, 0);
        assert_eq!(metrics.retained_runtime_pool_entries, 0);
    }

    #[tokio::test]
    async fn worker_router_prefers_tenant_affinity_for_warm_worker_reuse() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            max_concurrent_isolates: 2,
            worker_threads: 2,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let test_state = executor.test_state();
        let host = Arc::new(WorkerRuntimeIdHost {
            test_state: test_state.clone(),
        });
        let request = test_request("messages:list");

        let tenant_a_first = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context_for_tenant(&request, "tenant-a", "req-affinity-a-1"),
                None,
            )
            .await
            .expect("tenant-a invocation should succeed");
        let tenant_b_first = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context_for_tenant(&request, "tenant-b", "req-affinity-b-1"),
                None,
            )
            .await
            .expect("tenant-b invocation should succeed");
        let tenant_b_second = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host, policy),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context_for_tenant(&request, "tenant-b", "req-affinity-b-2"),
                None,
            )
            .await
            .expect("second tenant-b invocation should succeed");

        let tenant_a_worker = worker_runtime_id(&tenant_a_first);
        let tenant_b_worker = worker_runtime_id(&tenant_b_first);
        let tenant_b_second_worker = worker_runtime_id(&tenant_b_second);

        assert_ne!(
            tenant_a_worker, tenant_b_worker,
            "initial tie-broken routing should spread different tenants across workers"
        );
        assert_eq!(
            tenant_b_second_worker, tenant_b_worker,
            "tenant affinity should keep follow-up work on the warmed worker"
        );

        let metrics = executor.policy().metrics_snapshot();
        assert_eq!(metrics.worker_dispatched_invocations, 3);
        assert_eq!(metrics.worker_affinity_routed_invocations, 1);
        assert_eq!(metrics.worker_least_loaded_routed_invocations, 2);
    }

    #[tokio::test]
    async fn worker_router_uses_least_loaded_fallback_when_affinity_is_absent() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            max_concurrent_isolates: 2,
            worker_threads: 2,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let test_state = executor.test_state();
        let host = Arc::new(ControlledAsyncWorkerRuntimeIdHost::new(test_state));
        let bundle = RuntimeBundle::new(&bundle_path);

        let slow_request = test_request("slow-1");
        let slow_task = tokio::spawn({
            let executor = executor.clone();
            let host = host.clone();
            let bundle = bundle.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&slow_request, "tenant-a", "req-router-slow");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        slow_request,
                        context,
                        None,
                    )
                    .await
            }
        });

        host.wait_until_started("slow-1").await;
        let slow_worker = host
            .started_runtime_id("slow-1")
            .expect("slow invocation should record a worker runtime id");

        let fast_request = test_request("fast-1");
        let fast_result = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy),
                bundle,
                fast_request.clone(),
                test_context_for_tenant(&fast_request, "tenant-b", "req-router-fast"),
                None,
            )
            .await
            .expect("fast invocation should succeed");

        assert_ne!(
            worker_runtime_id(&fast_result),
            slow_worker,
            "a tenant without affinity should fall back to the least-loaded worker"
        );

        host.release_slow_jobs();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), slow_task)
                .await
                .expect("slow invocation should complete after host release")
                .expect("slow invocation task should join")
                .expect("slow invocation should succeed")
                .get("workerRuntimeId")
                .and_then(Value::as_u64)
                .map(|id| id as usize)
                .expect("slow result should include a workerRuntimeId"),
            slow_worker,
            "slow invocation should resume and finish on its original worker"
        );

        let metrics = executor.policy().metrics_snapshot();
        assert_eq!(metrics.worker_dispatched_invocations, 2);
        assert_eq!(metrics.worker_affinity_routed_invocations, 0);
        assert_eq!(metrics.worker_least_loaded_routed_invocations, 2);
    }

    #[tokio::test]
    async fn worker_router_can_affinitize_by_function() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            routing_affinity: crate::limits::RuntimeRoutingAffinity::Function,
            max_concurrent_isolates: 2,
            worker_threads: 2,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let test_state = executor.test_state();
        let host = Arc::new(WorkerRuntimeIdHost {
            test_state: test_state.clone(),
        });

        let first_request = test_request("messages:list");
        let second_request = test_request("messages:get");

        let function_a_first = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                first_request.clone(),
                test_context_for_tenant(&first_request, "tenant-a", "req-function-a-1"),
                None,
            )
            .await
            .expect("first function-affinitized invocation should succeed");
        let function_b_first = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                second_request.clone(),
                test_context_for_tenant(&second_request, "tenant-a", "req-function-b-1"),
                None,
            )
            .await
            .expect("second function-affinitized invocation should succeed");
        let function_b_second = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host, policy),
                RuntimeBundle::new(&bundle_path),
                second_request.clone(),
                test_context_for_tenant(&second_request, "tenant-a", "req-function-b-2"),
                None,
            )
            .await
            .expect("repeated function-affinitized invocation should succeed");

        let function_a_worker = worker_runtime_id(&function_a_first);
        let function_b_worker = worker_runtime_id(&function_b_first);
        let function_b_second_worker = worker_runtime_id(&function_b_second);

        assert_ne!(
            function_a_worker, function_b_worker,
            "different functions within one tenant should not share function affinity"
        );
        assert_eq!(
            function_b_second_worker, function_b_worker,
            "matching tenant+function should route back to the warmed worker"
        );
    }

    #[tokio::test]
    async fn worker_router_can_affinitize_by_script_identity() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir_a, bundle_path_a) = write_runtime_id_bundle();
        let (_bundle_dir_b, bundle_path_b) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            routing_affinity: crate::limits::RuntimeRoutingAffinity::Script,
            max_concurrent_isolates: 2,
            worker_threads: 2,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let test_state = executor.test_state();
        let host = Arc::new(WorkerRuntimeIdHost {
            test_state: test_state.clone(),
        });
        let request = test_request("messages:list");

        let script_a_first = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path_a),
                request.clone(),
                test_context_for_tenant(&request, "tenant-a", "req-script-a-1"),
                None,
            )
            .await
            .expect("first script-affinitized invocation should succeed");
        let script_b_first = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path_b),
                request.clone(),
                test_context_for_tenant(&request, "tenant-a", "req-script-b-1"),
                None,
            )
            .await
            .expect("second script-affinitized invocation should succeed");
        let script_b_second = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host, policy),
                RuntimeBundle::new(&bundle_path_b),
                request.clone(),
                test_context_for_tenant(&request, "tenant-a", "req-script-b-2"),
                None,
            )
            .await
            .expect("repeated script-affinitized invocation should succeed");

        let script_a_worker = worker_runtime_id(&script_a_first);
        let script_b_worker = worker_runtime_id(&script_b_first);
        let script_b_second_worker = worker_runtime_id(&script_b_second);

        assert_ne!(
            script_a_worker, script_b_worker,
            "different bundle identities should not share script affinity"
        );
        assert_eq!(
            script_b_second_worker, script_b_worker,
            "matching bundle identity should route back to the warmed worker"
        );
    }

    #[tokio::test]
    async fn worker_router_can_disable_affinity() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            routing_affinity: crate::limits::RuntimeRoutingAffinity::None,
            max_concurrent_isolates: 2,
            worker_threads: 2,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let test_state = executor.test_state();
        let host = Arc::new(WorkerRuntimeIdHost {
            test_state: test_state.clone(),
        });
        let request = test_request("messages:list");

        executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context_for_tenant(&request, "tenant-a", "req-no-affinity-1"),
                None,
            )
            .await
            .expect("first no-affinity invocation should succeed");
        executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host, policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context_for_tenant(&request, "tenant-a", "req-no-affinity-2"),
                None,
            )
            .await
            .expect("second no-affinity invocation should succeed");

        let metrics = executor.policy().metrics_snapshot();
        assert_eq!(metrics.worker_affinity_routed_invocations, 0);
        assert_eq!(metrics.worker_least_loaded_routed_invocations, 2);
        assert_eq!(metrics.worker_affinity_cache_entries, 0);
        assert_eq!(metrics.worker_affinity_cache_evictions, 0);
    }

    #[tokio::test]
    async fn worker_router_bounds_affinity_cache_and_records_evictions() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            routing_affinity: crate::limits::RuntimeRoutingAffinity::Tenant,
            routing_affinity_max_entries: 1,
            max_concurrent_isolates: 2,
            worker_threads: 2,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let test_state = executor.test_state();
        let host = Arc::new(WorkerRuntimeIdHost {
            test_state: test_state.clone(),
        });
        let request = test_request("messages:list");

        executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context_for_tenant(&request, "tenant-a", "req-affinity-cap-a-1"),
                None,
            )
            .await
            .expect("first bounded-affinity invocation should succeed");
        executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context_for_tenant(&request, "tenant-b", "req-affinity-cap-b-1"),
                None,
            )
            .await
            .expect("second bounded-affinity invocation should succeed");
        executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host, policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context_for_tenant(&request, "tenant-a", "req-affinity-cap-a-2"),
                None,
            )
            .await
            .expect("third bounded-affinity invocation should succeed");

        let metrics = executor.policy().metrics_snapshot();
        assert_eq!(metrics.worker_affinity_routed_invocations, 0);
        assert_eq!(metrics.worker_least_loaded_routed_invocations, 3);
        assert_eq!(metrics.worker_affinity_cache_entries, 1);
        assert_eq!(metrics.worker_affinity_cache_evictions, 2);
    }

    #[test]
    fn sibling_threads_can_boot_runtime_executors_in_parallel() {
        let _test_lock = runtime_executor_test_lock().blocking_lock();
        let (_bundle_dir, bundle_path) = write_constant_bundle();
        let before = crate::runtime::bootstrap_snapshot_build_count_for_test();
        let barrier = Arc::new(Barrier::new(3));

        let worker =
            |request_id: &'static str, barrier: Arc<Barrier>, bundle_path: std::path::PathBuf| {
                std::thread::spawn(move || {
                    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
                        max_concurrent_isolates: 1,
                        worker_threads: 1,
                        ..RuntimeLimits::default()
                    }));
                    let executor = RuntimeExecutor::new(policy.clone());
                    let request = test_request("messages:list");
                    barrier.wait();
                    executor.invoke_blocking_with_cancellation(
                        NeovexRuntime::with_policy(Arc::new(NoopHost), policy),
                        RuntimeBundle::new(bundle_path),
                        request.clone(),
                        test_context(&request, request_id),
                        None,
                    )
                })
            };

        let first = worker("req-sibling-1", barrier.clone(), bundle_path.clone());
        let second = worker("req-sibling-2", barrier.clone(), bundle_path);
        barrier.wait();

        assert_eq!(
            first
                .join()
                .expect("first sibling-thread executor should join")
                .expect("first sibling-thread invocation should succeed"),
            json!("ok")
        );
        assert_eq!(
            second
                .join()
                .expect("second sibling-thread executor should join")
                .expect("second sibling-thread invocation should succeed"),
            json!("ok")
        );

        let after = crate::runtime::bootstrap_snapshot_build_count_for_test();
        assert!(
            after.saturating_sub(before) <= 1,
            "parallel sibling-thread executor startups should reuse one process-global bootstrap snapshot"
        );
    }

    #[test]
    fn blocking_worker_invocation_succeeds_without_tokio_runtime_on_calling_thread() {
        let _test_lock = runtime_executor_test_lock().blocking_lock();
        assert!(
            tokio::runtime::Handle::try_current().is_err(),
            "plain #[test] should not already be inside a Tokio runtime"
        );

        let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let test_state = executor.test_state();
        let request = test_request("messages:list");
        let cancellation = HostCallCancellation::default();

        let result = executor
            .invoke_blocking_with_cancellation(
                NeovexRuntime::with_policy(
                    Arc::new(WorkerRuntimeIdHost {
                        test_state: test_state.clone(),
                    }),
                    policy.clone(),
                ),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context(&request, "req-blocking-worker"),
                Some(cancellation),
            )
            .expect("blocking worker invocation should succeed");

        assert_eq!(result, json!({ "workerRuntimeId": 1 }));
        assert_eq!(test_state.worker_runtime_builds(), 1);
    }

    #[tokio::test]
    async fn timed_out_worker_invocations_record_isolate_pool_replacements() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_timeout_bundle_dir, timeout_bundle_path) = write_busy_loop_bundle();
        let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_timeout: Duration::from_millis(50),
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let request = test_request("messages:list");

        let error = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(Arc::new(NoopHost), policy.clone()),
                RuntimeBundle::new(&timeout_bundle_path),
                request.clone(),
                test_context(&request, "req-timeout"),
                None,
            )
            .await
            .expect_err("busy-loop invocation should time out");
        assert!(matches!(error, NeovexRuntimeError::ExecutionTimeout(_)));

        let recovery_result = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(Arc::new(NoopHost), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context(&request, "req-recovery"),
                None,
            )
            .await
            .expect("follow-up invocation should succeed");
        assert_eq!(recovery_result, Value::Null);

        let metrics = policy.metrics_snapshot();
        assert_eq!(metrics.isolate_pool_misses, 1);
        assert_eq!(metrics.isolate_pool_hits, 1);
        assert_eq!(metrics.isolate_pool_replacements, 1);
    }

    #[tokio::test]
    async fn permit_suspend_frees_capacity() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            worker_threads: 2,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(ControlledAsyncGetHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);

        let slow_request = test_request("slow-1");
        let slow_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&slow_request, "tenant-a", "req-permit-slow");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        slow_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        host.wait_until_started("slow-1").await;

        let fast_request = test_request("fast-1");
        let fast_result = tokio::time::timeout(
            Duration::from_secs(1),
            executor.invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                bundle.clone(),
                fast_request.clone(),
                test_context_for_tenant(&fast_request, "tenant-b", "req-permit-fast"),
                None,
            ),
        )
        .await
        .expect("fast invocation should use the freed permit")
        .expect("fast invocation should succeed");

        assert_eq!(fast_result, json!({ "id": "fast-1" }));
        assert!(
            !slow_task.is_finished(),
            "slow invocation should still be parked while the second worker uses the freed permit"
        );

        host.release_slow_jobs();
        assert_eq!(
            slow_task
                .await
                .expect("slow task should join")
                .expect("slow invocation should succeed after resume"),
            json!({ "id": "slow-1" })
        );
    }

    #[tokio::test]
    async fn parked_invocation_resumes_after_host_completion() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(ControlledAsyncGetHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);
        let request = test_request("slow-1");
        let parked_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&request, "tenant-a", "req-parked-resume");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        request,
                        context,
                        None,
                    )
                    .await
            }
        });

        host.wait_until_started("slow-1").await;
        assert!(
            !parked_task.is_finished(),
            "parked invocation should remain pending until host work completes"
        );

        host.release_slow_jobs();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), parked_task)
                .await
                .expect("parked invocation should resume after host completion")
                .expect("parked task should join")
                .expect("parked invocation should succeed"),
            json!({ "id": "slow-1" })
        );
    }

    #[tokio::test]
    async fn parked_invocation_counts_toward_in_flight_limit() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            worker_threads: 2,
            max_active_top_level_invocations_per_tenant: 1,
            max_in_flight_top_level_invocations_per_tenant: 2,
            max_queued_top_level_invocations_per_tenant: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(ControlledAsyncGetHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);

        let first_request = test_request("slow-1");
        let first_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&first_request, "tenant-a", "req-inflight-1");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        first_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        host.wait_until_started("slow-1").await;

        let second_request = test_request("slow-2");
        let second_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&second_request, "tenant-a", "req-inflight-2");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        second_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        host.wait_until_started("slow-2").await;

        let third_request = test_request("fast-1");
        let third_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&third_request, "tenant-a", "req-inflight-3");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        third_request,
                        context,
                        None,
                    )
                    .await
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !host.started_ids().iter().any(|id| id == "fast-1"),
            "third invocation should remain queued while two parked invocations consume the tenant in-flight limit"
        );

        host.release_slow_jobs();
        assert_eq!(
            first_task
                .await
                .expect("first slow task should join")
                .expect("first slow invocation should succeed"),
            json!({ "id": "slow-1" })
        );
        assert_eq!(
            second_task
                .await
                .expect("second slow task should join")
                .expect("second slow invocation should succeed"),
            json!({ "id": "slow-2" })
        );
        assert_eq!(
            third_task
                .await
                .expect("third task should join")
                .expect("third invocation should succeed after queue promotion"),
            json!({ "id": "fast-1" })
        );
    }

    #[tokio::test]
    async fn timeout_excludes_permit_reacquire_wait() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_async_bundle_dir, async_bundle_path) = write_function_named_get_bundle();
        let (_sync_bundle_dir, sync_bundle_path) = write_sync_query_builder_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_timeout: Duration::from_millis(120),
            max_concurrent_isolates: 1,
            worker_threads: 2,
            max_active_top_level_invocations_per_tenant: 1,
            max_in_flight_top_level_invocations_per_tenant: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let parked_host = Arc::new(ControlledAsyncGetHost::default());
        let blocker_host = Arc::new(SlowSyncQueryHost::new(Duration::from_millis(80)));
        let async_bundle = RuntimeBundle::new(&async_bundle_path);
        let sync_bundle = RuntimeBundle::new(&sync_bundle_path);

        let slow_request = test_request("slow-1");
        let slow_started_at = std::time::Instant::now();
        let parked_task = tokio::spawn({
            let executor = executor.clone();
            let async_bundle = async_bundle.clone();
            let parked_host = parked_host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&slow_request, "tenant-a", "req-timeout-parked");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(parked_host, policy),
                        async_bundle,
                        slow_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        parked_host.wait_until_started("slow-1").await;
        tokio::time::sleep(Duration::from_millis(80)).await;

        let blocker_request = test_request("messages:list");
        let blocker_task = tokio::spawn({
            let executor = executor.clone();
            let sync_bundle = sync_bundle.clone();
            let blocker_host = blocker_host.clone();
            let policy = policy.clone();
            let context =
                test_context_for_tenant(&blocker_request, "tenant-b", "req-timeout-blocker");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(blocker_host, policy),
                        sync_bundle,
                        blocker_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        blocker_host.wait_until_started().await;
        parked_host.release_slow_jobs();

        assert_eq!(
            blocker_task
                .await
                .expect("blocker task should join")
                .expect("blocker invocation should succeed"),
            json!({ "builderId": "builder-1" })
        );
        assert_eq!(
            parked_task
                .await
                .expect("parked task should join")
                .expect("parked invocation should succeed after waiting to re-acquire the permit"),
            json!({ "id": "slow-1" })
        );
        assert!(
            slow_started_at.elapsed() >= Duration::from_millis(140),
            "parked invocation wall time should exceed the execution timeout while still succeeding because permit re-acquire wait is paused"
        );
    }

    #[tokio::test]
    async fn tenant_queue_limit_rejections_record_metrics() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            max_active_top_level_invocations_per_tenant: 1,
            max_in_flight_top_level_invocations_per_tenant: 1,
            max_queued_top_level_invocations_per_tenant: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(TenantFairnessHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);

        let slow_request = test_request("slow-1");
        let slow_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&slow_request, "tenant-a", "req-slow-1");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        slow_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(1), host.slow_started.notified())
            .await
            .expect("slow runtime invocation should start");

        let queued_request = test_request("slow-2");
        let queued_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&queued_request, "tenant-a", "req-slow-2");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        queued_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let rejected_request = test_request("slow-3");
        let error = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                bundle.clone(),
                rejected_request.clone(),
                test_context_for_tenant(&rejected_request, "tenant-a", "req-slow-3"),
                None,
            )
            .await
            .expect_err("third tenant-a invocation should be rejected");
        assert!(matches!(
            error,
            NeovexRuntimeError::TenantQueueLimitExceeded {
                ref tenant_label,
                limit: 1,
            } if tenant_label == "tenant-a"
        ));

        let metrics = policy.metrics_snapshot();
        assert_eq!(metrics.rejected_invocations, 1);
        assert_eq!(
            metrics
                .tenants
                .get("tenant-a")
                .expect("tenant metrics should be present")
                .rejected_invocations,
            1
        );

        host.release_slow_job();
        assert_eq!(
            slow_task
                .await
                .expect("slow task should join")
                .expect("slow invocation should succeed"),
            json!({ "id": "slow-1" })
        );
        assert_eq!(
            queued_task
                .await
                .expect("queued task should join")
                .expect("queued invocation should succeed"),
            json!({ "id": "slow-2" })
        );
    }

    #[tokio::test]
    async fn tenant_fairness_prevents_one_tenant_from_starving_another() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 2,
            max_active_top_level_invocations_per_tenant: 1,
            max_in_flight_top_level_invocations_per_tenant: 1,
            max_queued_top_level_invocations_per_tenant: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(TenantFairnessHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);

        let slow_request = test_request("slow-1");
        let slow_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&slow_request, "tenant-a", "req-tenant-a-1");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        slow_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(1), host.slow_started.notified())
            .await
            .expect("slow tenant-a invocation should start");

        let queued_request = test_request("slow-2");
        let queued_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&queued_request, "tenant-a", "req-tenant-a-2");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        queued_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let fast_request = test_request("fast-1");
        let fast_result = tokio::time::timeout(
            Duration::from_secs(1),
            executor.invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                bundle.clone(),
                fast_request.clone(),
                test_context_for_tenant(&fast_request, "tenant-b", "req-tenant-b-1"),
                None,
            ),
        )
        .await
        .expect("tenant-b invocation should not be starved")
        .expect("tenant-b invocation should succeed");
        assert_eq!(fast_result, json!({ "id": "fast-1" }));
        assert!(
            !host.started_ids().iter().any(|id| id == "slow-2"),
            "tenant-a queued invocation should remain queued until tenant-a frees a slot"
        );

        host.release_slow_job();
        assert_eq!(
            slow_task
                .await
                .expect("slow task should join")
                .expect("slow invocation should succeed"),
            json!({ "id": "slow-1" })
        );
        assert_eq!(
            queued_task
                .await
                .expect("queued task should join")
                .expect("queued invocation should succeed"),
            json!({ "id": "slow-2" })
        );
    }
}
