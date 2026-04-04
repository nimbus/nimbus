#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
#[cfg(test)]
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
#[cfg(test)]
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

use crate::context::RuntimeInvocationContext;
use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::runtime::{InvocationRequest, NeovexRuntime, RuntimeBundle, RuntimeInvocationExecution};
use crate::watchdog::WatchdogTimer;
use crate::worker_loop::{RunToCompletionWorkerLoopFactory, WorkerLoopFactory};

mod admission;
mod lifecycle;
mod queue;

pub(crate) use self::admission::SharedInvocationPermit;
use self::admission::{RuntimeExecutorAdmission, RuntimeExecutorAdmissionDecision};
pub(crate) use self::lifecycle::run_invocation_lifecycle;
use self::queue::{RuntimeWorkerJob, RuntimeWorkerQueueController, RuntimeWorkerResultSender};
pub(crate) use self::queue::{RuntimeWorkerQueue, RuntimeWorkerShutdown};

#[derive(Clone)]
pub struct RuntimeExecutor {
    inner: Arc<RuntimeExecutorInner>,
}

struct RuntimeExecutorInner {
    policy: Arc<RuntimePolicy>,
    queue: Arc<RuntimeWorkerQueueController>,
    admission: Arc<RuntimeExecutorAdmission>,
    shutdown: RuntimeWorkerShutdown,
    watchdog: WatchdogTimer,
    worker_count: usize,
    queue_capacity: usize,
    worker_handles: Mutex<Vec<JoinHandle<()>>>,
    #[cfg(test)]
    test_state: Arc<RuntimeExecutorTestState>,
}

#[cfg(test)]
pub(crate) struct RuntimeExecutorTestState {
    next_worker_runtime_id: AtomicUsize,
    worker_runtime_builds: AtomicUsize,
    worker_thread_runtime_ids: Mutex<HashMap<ThreadId, usize>>,
}

#[cfg(test)]
impl RuntimeExecutorTestState {
    fn new() -> Self {
        Self {
            next_worker_runtime_id: AtomicUsize::new(1),
            worker_runtime_builds: AtomicUsize::new(0),
            worker_thread_runtime_ids: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn register_current_worker_runtime(&self) {
        let worker_runtime_id = self.next_worker_runtime_id.fetch_add(1, Ordering::Relaxed);
        self.worker_runtime_builds.fetch_add(1, Ordering::Relaxed);
        self.worker_thread_runtime_ids
            .lock()
            .expect("runtime executor test state lock should not be poisoned")
            .insert(std::thread::current().id(), worker_runtime_id);
    }

    fn worker_runtime_builds(&self) -> usize {
        self.worker_runtime_builds.load(Ordering::Relaxed)
    }

    fn worker_runtime_id_for_current_thread(&self) -> Option<usize> {
        self.worker_thread_runtime_ids
            .lock()
            .expect("runtime executor test state lock should not be poisoned")
            .get(&std::thread::current().id())
            .copied()
    }
}

const BLOCKING_RESULT_POLL_INTERVAL: Duration = Duration::from_millis(1);

impl std::fmt::Debug for RuntimeExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeExecutor")
            .field("worker_count", &self.inner.worker_count)
            .field("queue_capacity", &self.inner.queue_capacity)
            .finish()
    }
}

impl RuntimeExecutor {
    pub fn new(policy: Arc<RuntimePolicy>) -> Self {
        let worker_count = policy.limits().worker_threads.max(1);
        let queue_capacity = worker_count.saturating_mul(4).max(1);
        let (worker_sender, receiver) = mpsc::channel::<RuntimeWorkerJob>(queue_capacity);
        let sender = Arc::new(Mutex::new(Some(worker_sender)));
        let receiver = Arc::new(Mutex::new(receiver));
        let queue = Arc::new(RuntimeWorkerQueueController::new(receiver, sender));
        let admission = Arc::new(RuntimeExecutorAdmission::new(policy.clone()));
        let shutdown = RuntimeWorkerShutdown::new();
        let watchdog = WatchdogTimer::new();
        #[cfg(test)]
        let test_state = Arc::new(RuntimeExecutorTestState::new());
        let worker_loop_factory: Arc<dyn WorkerLoopFactory> = {
            let factory = RunToCompletionWorkerLoopFactory::new(watchdog.clone());
            #[cfg(test)]
            let factory = factory.with_test_state(test_state.clone());
            Arc::new(factory)
        };
        let mut worker_handles = Vec::with_capacity(worker_count);

        for worker_id in 0..worker_count {
            let queue: Arc<dyn RuntimeWorkerQueue> = queue.clone();
            let policy = policy.clone();
            let shutdown = shutdown.clone();
            let worker_loop_factory = worker_loop_factory.clone();
            let handle = std::thread::Builder::new()
                .name(format!("neovex-runtime-worker-{worker_id}"))
                .spawn(move || {
                    let mut worker_loop = worker_loop_factory.create(worker_id, policy);
                    worker_loop.run(queue, shutdown);
                })
                .expect("runtime executor worker thread should start");
            worker_handles.push(handle);
        }

        Self {
            inner: Arc::new(RuntimeExecutorInner {
                policy,
                queue,
                admission,
                shutdown,
                watchdog,
                worker_count,
                queue_capacity,
                worker_handles: Mutex::new(worker_handles),
                #[cfg(test)]
                test_state,
            }),
        }
    }

    pub fn policy(&self) -> Arc<RuntimePolicy> {
        self.inner.policy.clone()
    }

    #[cfg(test)]
    fn test_state(&self) -> Arc<RuntimeExecutorTestState> {
        self.inner.test_state.clone()
    }

    async fn dispatch_admitted_job_async(&self, job: RuntimeWorkerJob) -> Result<()> {
        self.inner.queue.dispatch_job(job).await
    }

    fn dispatch_admitted_job_blocking(&self, job: RuntimeWorkerJob) -> Result<()> {
        self.inner.queue.dispatch_job_blocking(job)
    }

    async fn invoke_job(
        watchdog: WatchdogTimer,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
        queue_started_at: Instant,
    ) -> Result<Value> {
        let policy = runtime.policy();
        let permit = SharedInvocationPermit::new(
            policy.clone(),
            context.tenant_label.clone(),
            None,
            runtime.bypasses_concurrency_limit(),
            cancellation.clone(),
        );
        let (result, _ready_jobs) = run_invocation_lifecycle(
            permit,
            policy,
            context.clone(),
            cancellation.clone(),
            queue_started_at,
            None,
            |permit| async move {
                runtime
                    .invoke_bundle_unmanaged(
                        None,
                        RuntimeInvocationExecution {
                            watchdog: watchdog.clone(),
                            bundle: bundle.clone(),
                            request: request.clone(),
                            context: context.clone(),
                            external_cancellation: cancellation,
                            permit,
                        },
                    )
                    .await
            },
        )
        .await;
        result
    }

    pub async fn invoke(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
    ) -> Result<Value> {
        self.invoke_with_cancellation(runtime, bundle, request, context, None)
            .await
    }

    pub async fn invoke_with_cancellation(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.inner
            .policy
            .metrics()
            .record_request_correlation(&context);
        Self::invoke_job(
            self.inner.watchdog.clone(),
            runtime.into_policy(self.inner.policy.clone()),
            bundle,
            request,
            context,
            cancellation,
            Instant::now(),
        )
        .await
    }

    pub async fn invoke_on_worker(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.inner
            .policy
            .metrics()
            .record_request_correlation(&context);
        if cancellation
            .as_ref()
            .is_some_and(HostCallCancellation::is_cancelled)
        {
            self.inner
                .policy
                .metrics()
                .record_queued_canceled_invocation_for_tenant(
                    context.tenant_label.as_deref(),
                    cancellation.as_ref().and_then(HostCallCancellation::cause),
                );
            return Err(NeovexRuntimeError::Cancelled);
        }

        let (result_tx, result_rx) = oneshot::channel();
        let admission = self.inner.admission.admit_job(RuntimeWorkerJob {
            runtime,
            bundle,
            request,
            context,
            cancellation: cancellation.clone(),
            enqueued_at: Instant::now(),
            result_tx: RuntimeWorkerResultSender::Async(result_tx),
            dispatch_handle: None,
        })?;
        if let RuntimeExecutorAdmissionDecision::Dispatch(job) = admission {
            self.dispatch_admitted_job_async(*job).await?;
        }

        match cancellation {
            Some(cancellation) => {
                tokio::select! {
                    _ = cancellation.cancelled() => Err(NeovexRuntimeError::Cancelled),
                    result = result_rx => result.map_err(|_| {
                        NeovexRuntimeError::Contract(
                            "runtime executor dropped an invocation result".to_string(),
                        )
                    })?,
                }
            }
            None => result_rx.await.map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime executor dropped an invocation result".to_string(),
                )
            })?,
        }
    }

    pub fn invoke_blocking(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
    ) -> Result<Value> {
        self.invoke_blocking_with_cancellation(runtime, bundle, request, context, None)
    }

    pub fn invoke_blocking_with_cancellation(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        let executor = self.clone();
        let invoke = move || {
            executor.invoke_on_worker_blocking(runtime, bundle, request, context, cancellation)
        };

        if tokio::runtime::Handle::try_current().is_ok() {
            std::thread::spawn(invoke).join().map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime executor invocation thread panicked".to_string(),
                )
            })?
        } else {
            invoke()
        }
    }

    fn invoke_on_worker_blocking(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.inner
            .policy
            .metrics()
            .record_request_correlation(&context);
        if cancellation
            .as_ref()
            .is_some_and(HostCallCancellation::is_cancelled)
        {
            self.inner
                .policy
                .metrics()
                .record_queued_canceled_invocation_for_tenant(
                    context.tenant_label.as_deref(),
                    cancellation.as_ref().and_then(HostCallCancellation::cause),
                );
            return Err(NeovexRuntimeError::Cancelled);
        }

        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let admission = self.inner.admission.admit_job(RuntimeWorkerJob {
            runtime,
            bundle,
            request,
            context,
            cancellation: cancellation.clone(),
            enqueued_at: Instant::now(),
            result_tx: RuntimeWorkerResultSender::Blocking(result_tx),
            dispatch_handle: None,
        })?;
        if let RuntimeExecutorAdmissionDecision::Dispatch(job) = admission {
            self.dispatch_admitted_job_blocking(*job)?;
        }

        match cancellation {
            Some(cancellation) => loop {
                if cancellation.is_cancelled() {
                    return Err(NeovexRuntimeError::Cancelled);
                }
                match result_rx.recv_timeout(BLOCKING_RESULT_POLL_INTERVAL) {
                    Ok(result) => return result,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(NeovexRuntimeError::Contract(
                            "runtime executor dropped an invocation result".to_string(),
                        ));
                    }
                }
            },
            None => result_rx.recv().map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime executor dropped an invocation result".to_string(),
                )
            })?,
        }
    }
}

impl Drop for RuntimeExecutorInner {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.queue.close();
        for queued_job in self.admission.drain_queued_jobs() {
            queued_job.result_tx.send(Err(NeovexRuntimeError::Contract(
                "runtime executor unexpectedly closed".to_string(),
            )));
        }
        let mut worker_handles = self
            .worker_handles
            .lock()
            .expect("runtime executor worker handle lock should not be poisoned");
        for handle in worker_handles.drain(..) {
            let _ = handle.join();
        }
        self.watchdog.shutdown();
    }
}

impl Default for RuntimeExecutor {
    fn default() -> Self {
        let policy = Arc::new(RuntimePolicy::default());
        Self::new(policy)
    }
}

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
            Box::pin(async move {
                if document_id.starts_with("slow-") {
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
