use std::sync::Mutex as StdMutex;
use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};
use tempfile::tempdir;
use tokio::sync::{Mutex as TokioMutex, Notify};

use super::*;
use crate::host::{HostBridge, HostBridgeFuture, HostCallOperation, HostCallRequest};

pub(super) struct NoopHost;

impl HostBridge for NoopHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Ok(Value::Null)
    }
}

pub(super) struct WorkerRuntimeIdHost {
    pub(super) test_state: Arc<RuntimeExecutorTestState>,
}

impl HostBridge for WorkerRuntimeIdHost {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        assert_eq!(request.operation, HostCallOperation::CtxDbGet);
        Ok(json!({
            "workerRuntimeId": self.test_state.worker_runtime_id_for_current_thread(),
        }))
    }
}

pub(super) struct ControlledAsyncWorkerRuntimeIdHost {
    test_state: Arc<RuntimeExecutorTestState>,
    started: StdMutex<std::collections::HashMap<String, usize>>,
    started_notify: Arc<Notify>,
    release_slow: Arc<Notify>,
    release_slow_flag: Arc<std::sync::atomic::AtomicBool>,
}

impl ControlledAsyncWorkerRuntimeIdHost {
    pub(super) fn new(test_state: Arc<RuntimeExecutorTestState>) -> Self {
        Self {
            test_state,
            started: StdMutex::new(std::collections::HashMap::new()),
            started_notify: Arc::new(Notify::new()),
            release_slow: Arc::new(Notify::new()),
            release_slow_flag: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub(super) async fn wait_until_started(&self, document_id: &str) {
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

    pub(super) fn started_runtime_id(&self, document_id: &str) -> Option<usize> {
        self.started
            .lock()
            .expect("controlled runtime-id host lock should not be poisoned")
            .get(document_id)
            .copied()
    }

    pub(super) fn release_slow_jobs(&self) {
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
pub(super) struct TenantFairnessHost {
    started_ids: StdMutex<Vec<String>>,
    started_notify: Arc<Notify>,
    pub(super) slow_started: Arc<Notify>,
    release_slow: Arc<Notify>,
}

impl TenantFairnessHost {
    pub(super) fn started_ids(&self) -> Vec<String> {
        self.started_ids
            .lock()
            .expect("tenant fairness host lock should not be poisoned")
            .clone()
    }

    pub(super) async fn assert_not_started_within(&self, document_id: &str, duration: Duration) {
        let deadline = tokio::time::Instant::now() + duration;
        loop {
            assert!(
                !self
                    .started_ids()
                    .iter()
                    .any(|started| started == document_id),
                "host request {document_id} should remain queued"
            );
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return;
            }
            let notified = self.started_notify.notified();
            if tokio::time::timeout(deadline.saturating_duration_since(now), notified)
                .await
                .is_err()
            {
                return;
            }
        }
    }

    pub(super) fn release_slow_job(&self) {
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
        self.started_notify.notify_waiters();
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
pub(super) struct ControlledAsyncGetHost {
    started_ids: StdMutex<Vec<String>>,
    started_notify: Arc<Notify>,
    release_slow: Arc<Notify>,
    release_slow_flag: Arc<std::sync::atomic::AtomicBool>,
}

impl ControlledAsyncGetHost {
    pub(super) fn started_ids(&self) -> Vec<String> {
        self.started_ids
            .lock()
            .expect("controlled async host lock should not be poisoned")
            .clone()
    }

    pub(super) async fn wait_until_started(&self, document_id: &str) {
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

    pub(super) async fn assert_not_started_within(&self, document_id: &str, duration: Duration) {
        let deadline = tokio::time::Instant::now() + duration;
        loop {
            assert!(
                !self
                    .started_ids()
                    .iter()
                    .any(|started| started == document_id),
                "host request {document_id} should remain queued"
            );
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return;
            }
            let notified = self.started_notify.notified();
            if tokio::time::timeout(deadline.saturating_duration_since(now), notified)
                .await
                .is_err()
            {
                return;
            }
        }
    }

    pub(super) fn release_slow_jobs(&self) {
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

pub(super) struct SlowSyncQueryHost {
    delay: Duration,
    started: Arc<Notify>,
}

impl SlowSyncQueryHost {
    pub(super) fn new(delay: Duration) -> Self {
        Self {
            delay,
            started: Arc::new(Notify::new()),
        }
    }

    pub(super) async fn wait_until_started(&self) {
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

pub(super) fn write_runtime_id_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
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

pub(super) fn write_busy_loop_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
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

pub(super) fn write_function_named_get_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
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

pub(super) fn write_sync_query_builder_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
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

pub(super) fn write_constant_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
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

pub(super) fn test_request(function_name: &str) -> InvocationRequest {
    InvocationRequest {
        kind: crate::runtime::InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

pub(super) fn test_context_for_tenant(
    request: &InvocationRequest,
    tenant_label: &str,
    request_id: &str,
) -> RuntimeInvocationContext {
    RuntimeInvocationContext::top_level_for_tenant_and_request(request, tenant_label, request_id)
}

pub(super) fn test_context(
    request: &InvocationRequest,
    request_id: &str,
) -> RuntimeInvocationContext {
    test_context_for_tenant(request, "demo", request_id)
}

pub(super) fn worker_runtime_id(result: &Value) -> usize {
    result
        .get("workerRuntimeId")
        .and_then(Value::as_u64)
        .map(|id| id as usize)
        .expect("result should include a workerRuntimeId")
}

pub(super) fn runtime_executor_test_lock() -> &'static TokioMutex<()> {
    static RUNTIME_EXECUTOR_TEST_LOCK: OnceLock<TokioMutex<()>> = OnceLock::new();
    RUNTIME_EXECUTOR_TEST_LOCK.get_or_init(|| TokioMutex::new(()))
}
