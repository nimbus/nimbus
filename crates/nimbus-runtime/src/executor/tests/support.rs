use std::sync::Mutex as StdMutex;
use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};
use tempfile::tempdir;
use tokio::sync::{Mutex as TokioMutex, Notify};

use super::*;
use crate::host::{HostBridge, HostBridgeFuture, HostCallOperation, HostCallRequest};

fn duration_ms_env_or(name: &str, default: Duration) -> Duration {
    let default_ms = default.as_millis().min(u64::MAX as u128) as u64;
    Duration::from_millis(
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(default_ms),
    )
}

fn ci_or_local_duration(local: Duration, ci: Duration) -> Duration {
    if std::env::var_os("CI").is_some() {
        ci
    } else {
        local
    }
}

fn host_start_timeout() -> Duration {
    duration_ms_env_or(
        "NIMBUS_EXECUTOR_HOST_START_TIMEOUT_MS",
        ci_or_local_duration(Duration::from_secs(15), Duration::from_secs(60)),
    )
}

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
        assert_eq!(request.operation, HostCallOperation::DocumentGet);
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
        tokio::time::timeout(host_start_timeout(), async {
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
        Err(NimbusRuntimeError::Contract(
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

#[derive(Clone, Copy)]
pub(super) struct SyntheticAwaitHost {
    delay: Duration,
}

impl SyntheticAwaitHost {
    pub(super) fn new(delay: Duration) -> Self {
        Self { delay }
    }
}

impl HostBridge for SyntheticAwaitHost {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(format!(
            "synthetic-await host expects async db.get path: {}",
            request.operation
        )))
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        _cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let delay = self.delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(json!({
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

    pub(super) async fn wait_until_slow_started(&self) {
        tokio::time::timeout(host_start_timeout(), self.slow_started.notified())
            .await
            .expect("slow tenant fairness host request should start");
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
        Err(NimbusRuntimeError::Contract(
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
pub(super) struct StepControlledAsyncGetHost {
    state: Arc<StdMutex<StepControlledAsyncGetHostState>>,
    started_notify: Arc<Notify>,
}

#[derive(Default)]
struct StepControlledAsyncGetHostState {
    started_ids: Vec<String>,
    release_by_id: std::collections::HashMap<String, Arc<Notify>>,
    released_ids: std::collections::HashSet<String>,
    active_host_calls: usize,
    max_active_host_calls: usize,
}

impl StepControlledAsyncGetHost {
    pub(super) async fn wait_until_started(&self, document_id: &str) {
        tokio::time::timeout(host_start_timeout(), async {
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

    pub(super) fn release(&self, document_id: &str) {
        let notify = {
            let mut state = self
                .state
                .lock()
                .expect("step-controlled async host lock should not be poisoned");
            state.released_ids.insert(document_id.to_string());
            state.release_by_id.get(document_id).cloned()
        };
        if let Some(notify) = notify {
            notify.notify_waiters();
        }
    }

    pub(super) fn max_active_host_calls(&self) -> usize {
        self.state
            .lock()
            .expect("step-controlled async host lock should not be poisoned")
            .max_active_host_calls
    }

    fn started_ids(&self) -> Vec<String> {
        self.state
            .lock()
            .expect("step-controlled async host lock should not be poisoned")
            .started_ids
            .clone()
    }
}

struct StepControlledActiveHostCall {
    state: Arc<StdMutex<StepControlledAsyncGetHostState>>,
}

impl Drop for StepControlledActiveHostCall {
    fn drop(&mut self) {
        let mut state = self
            .state
            .lock()
            .expect("step-controlled async host lock should not be poisoned");
        state.active_host_calls = state.active_host_calls.saturating_sub(1);
    }
}

impl HostBridge for StepControlledAsyncGetHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "step-controlled async host expects async db.get path".to_string(),
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
        let state = self.state.clone();
        let release = {
            let mut state = self
                .state
                .lock()
                .expect("step-controlled async host lock should not be poisoned");
            state.started_ids.push(document_id.clone());
            state.active_host_calls += 1;
            state.max_active_host_calls = state.max_active_host_calls.max(state.active_host_calls);
            if state.released_ids.contains(&document_id) {
                None
            } else {
                Some(
                    state
                        .release_by_id
                        .entry(document_id.clone())
                        .or_insert_with(|| Arc::new(Notify::new()))
                        .clone(),
                )
            }
        };
        self.started_notify.notify_waiters();
        Box::pin(async move {
            let _active = StepControlledActiveHostCall {
                state: state.clone(),
            };
            if let Some(release) = release {
                release.notified().await;
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
        tokio::time::timeout(host_start_timeout(), async {
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
        Err(NimbusRuntimeError::Contract(
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

pub(super) struct RejectingAsyncGetHost {
    reject_document_id: &'static str,
}

impl RejectingAsyncGetHost {
    pub(super) fn new(reject_document_id: &'static str) -> Self {
        Self { reject_document_id }
    }
}

impl HostBridge for RejectingAsyncGetHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "rejecting async host expects async db.get path".to_string(),
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
        let reject_document_id = self.reject_document_id;
        Box::pin(async move {
            if document_id == reject_document_id {
                return Err(NimbusRuntimeError::Contract(format!(
                    "background document get rejected for {document_id}"
                )));
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
        tokio::time::timeout(host_start_timeout(), self.started.notified())
            .await
            .expect("slow sync query host should start");
    }
}

impl HostBridge for SlowSyncQueryHost {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        assert_eq!(request.operation, HostCallOperation::QueryBuilderStart);
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
            Err(NimbusRuntimeError::Contract(
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
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
  });
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
    )
    .expect("bundle should write");
    (bundle_dir, bundle_path)
}

pub(super) fn write_node_fs_policy_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
    let bundle_dir = tempdir().expect("tempdir should build");
    let generated_root = bundle_dir.path().join("app/.nimbus/convex");
    std::fs::create_dir_all(&generated_root).expect("generated root should create");
    let bundle_path = generated_root.join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
import path from "node:path";

globalThis.__nimbusInvoke = function (request) {
  if (request.function_name === "messages:warmNoop") {
    return "warm-noop";
  }
  const localDir = path.dirname(new URL(import.meta.url).pathname);
  const directory = path.join(localDir, "worker-policy-write");
  Deno.mkdirSync(directory, { recursive: true });
  return Deno.statSync(directory).isDirectory ? "job-policy-fs" : "missing";
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
globalThis.__nimbusInvoke = function () {
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
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
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
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
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
globalThis.__nimbusInvoke = async function () {
  return "ok";
};

export {};
"#,
    )
    .expect("bundle should write");
    (bundle_dir, bundle_path)
}

pub(super) fn write_wait_until_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
    let bundle_dir = tempdir().expect("tempdir should build");
    let bundle_path = bundle_dir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
  });
  await ctx.db.get("messages", "response");
  globalThis.__nimbusWaitUntil(ctx.db.get("messages", "slow-background"));
  return { responseReady: true };
};

export {};
"#,
    )
    .expect("bundle should write");
    (bundle_dir, bundle_path)
}

pub(super) fn write_rejected_wait_until_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
    let bundle_dir = tempdir().expect("tempdir should build");
    let bundle_path = bundle_dir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
  });
  globalThis.__nimbusWaitUntil(ctx.db.get("messages", "reject-background"));
  return { responseReady: true };
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
    type TestAuthority = (
        crate::RuntimeOwnerLease,
        crate::RuntimeDeploymentAuthorityLease,
    );
    static AUTHORITIES: OnceLock<StdMutex<std::collections::HashMap<String, TestAuthority>>> =
        OnceLock::new();
    let authorities = AUTHORITIES.get_or_init(|| StdMutex::new(std::collections::HashMap::new()));
    let (owner, deployment) = authorities
        .lock()
        .expect("executor test authority registry should not be poisoned")
        .entry(tenant_label.to_string())
        .or_insert_with(|| {
            let owner_id = crate::RuntimeOwnerId::tenant(
                format!("executor-test:{tenant_label}"),
                std::num::NonZeroU64::new(1).expect("test incarnation is nonzero"),
                Some(tenant_label),
            )
            .expect("executor test owner should be valid");
            let (owner, _) = crate::RuntimeOwnerLeaseIssuer.issue(owner_id);
            let deployment_id = crate::RuntimeDeploymentAuthorityId::new(
                "executor-test-deployment",
                std::num::NonZeroU64::new(1).expect("test deployment generation is nonzero"),
            )
            .expect("executor test deployment authority should be valid");
            let (deployment, _) = crate::RuntimeDeploymentAuthorityLeaseIssuer.issue(deployment_id);
            (owner, deployment)
        })
        .clone();
    RuntimeInvocationContext::top_level_for_tenant_and_request_with_owner(
        request,
        tenant_label,
        owner,
        request_id,
    )
    .with_deployment_authority(deployment)
}

pub(super) fn test_context(
    request: &InvocationRequest,
    request_id: &str,
) -> RuntimeInvocationContext {
    test_context_for_tenant(request, "demo", request_id)
}

pub(super) fn test_context_without_tenant(request: &InvocationRequest) -> RuntimeInvocationContext {
    static OWNER: OnceLock<crate::RuntimeOwnerLease> = OnceLock::new();
    let owner = OWNER.get_or_init(|| {
        let owner_id = crate::RuntimeOwnerId::trusted_session(
            crate::RuntimeOwnerClass::Tooling,
            "executor-test:unscoped-routing",
            std::num::NonZeroU64::new(1).expect("test incarnation is nonzero"),
            Some("unscoped-routing"),
        )
        .expect("executor test tooling owner should be valid");
        crate::RuntimeOwnerLeaseIssuer.issue(owner_id).0
    });
    RuntimeInvocationContext::top_level_with_owner(request, owner.clone())
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
