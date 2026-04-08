#[cfg(test)]
use std::rc::Rc;
use std::sync::Arc;
use std::sync::OnceLock;

#[cfg(test)]
use deno_core::{CreateRealmOptions, JsRuntime, PollEventLoopOptions};
#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use crate::RuntimeInvocationContext;
#[cfg(test)]
use crate::error::{NeovexRuntimeError, Result};
use crate::executor::RuntimeExecutor;
#[cfg(test)]
use crate::executor::SharedInvocationPermit;
use crate::host::HostBridge;
#[cfg(test)]
use crate::limits::RuntimeLimits;
use crate::limits::RuntimePolicy;
#[cfg(test)]
use crate::module_loader::SandboxedModuleLoader;
#[cfg(test)]
use crate::watchdog::WatchdogTimer;

mod bootstrap;
mod bundle;
mod cooperative;
mod driver;
mod facade;
mod helpers;
mod invocation;

#[cfg(test)]
use self::bootstrap::RuntimeCancellationState;
pub(crate) use self::bootstrap::{
    ReusableRuntime, RuntimeConstructionMode, RuntimeInvocationTimeoutController,
    RuntimeWorkerIsolatePool,
};
pub use self::bundle::RuntimeBundle;
#[cfg(test)]
use self::helpers::deserialize_json_value;
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
    #[cfg(test)]
    retained_runtime_construction_mode_for_test: RuntimeConstructionMode,
}

pub(crate) use self::cooperative::{
    CooperativeLockerRuntimeSlot, CooperativeRuntimeSlotPoll, CooperativeRuntimeSlotStart,
    RuntimeInvocationExecution,
};

use self::driver::RuntimeInvocationDriver;

/// Legacy alias for Convex-shaped integrations.
pub type ConvexRuntime = NeovexRuntime;

#[cfg(test)]
pub(crate) fn bootstrap_snapshot_build_count_for_test() -> usize {
    self::driver::snapshot_build_count_for_test()
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    #[cfg(unix)]
    use std::os::fd::AsRawFd;
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

    use serde_json::Map;
    use tempfile::tempdir;
    use tokio::sync::Notify;

    use super::*;
    use crate::host::{HostBridgeFuture, HostCallCancellation, HostCallOperation, HostCallRequest};

    fn init_test_tracing() {
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

    fn acquire_runtime_suite_lock() -> MutexGuard<'static, ()> {
        static IN_PROCESS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        IN_PROCESS_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("runtime test lock should not be poisoned")
    }

    struct SnapshotResetTestLockGuard {
        _in_process_guard: MutexGuard<'static, ()>,
        #[cfg(unix)]
        file: std::fs::File,
    }

    fn acquire_snapshot_reset_test_lock() -> SnapshotResetTestLockGuard {
        static IN_PROCESS_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let in_process_guard = IN_PROCESS_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("snapshot reset test lock should not be poisoned");

        #[cfg(unix)]
        {
            const LOCK_EX: i32 = 2;

            unsafe extern "C" {
                fn flock(fd: i32, operation: i32) -> i32;
            }

            let path = std::env::temp_dir().join("neovex-runtime-snapshot-reset-test.lock");
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(path)
                .expect("snapshot reset test lockfile should open");
            let status = unsafe { flock(file.as_raw_fd(), LOCK_EX) };
            assert_eq!(
                status, 0,
                "snapshot reset test lock should acquire successfully"
            );
            SnapshotResetTestLockGuard {
                _in_process_guard: in_process_guard,
                file,
            }
        }

        #[cfg(not(unix))]
        {
            SnapshotResetTestLockGuard {
                _in_process_guard: in_process_guard,
            }
        }
    }

    #[cfg(unix)]
    impl Drop for SnapshotResetTestLockGuard {
        fn drop(&mut self) {
            const LOCK_UN: i32 = 8;

            unsafe extern "C" {
                fn flock(fd: i32, operation: i32) -> i32;
            }

            let _ = unsafe { flock(self.file.as_raw_fd(), LOCK_UN) };
        }
    }

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

    #[derive(Clone, Copy)]
    struct DelayedAsyncEchoHost {
        delay: std::time::Duration,
    }

    impl DelayedAsyncEchoHost {
        fn new(delay: std::time::Duration) -> Self {
            Self { delay }
        }
    }

    impl HostBridge for DelayedAsyncEchoHost {
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
            let delay = self.delay;
            Box::pin(async move {
                tokio::time::sleep(delay).await;
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
    struct DeferredAsyncHost {
        release: Arc<Notify>,
        calls: Mutex<Vec<HostCallRequest>>,
    }

    impl DeferredAsyncHost {
        fn release(&self) {
            self.release.notify_waiters();
        }
    }

    impl HostBridge for DeferredAsyncHost {
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
            self.calls
                .lock()
                .expect("deferred async host lock should not be poisoned")
                .push(request.clone());
            let release = self.release.clone();
            Box::pin(async move {
                release.notified().await;
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

    mod cooperative;
    mod locker;
    mod retained_pool;

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
        assert_eq!(metrics.retained_runtime_main_realm_resets, 0);
        assert_eq!(metrics.retained_runtime_main_realm_reset_nanos_total, 0);
        assert_eq!(metrics.retained_runtime_bootstrap_replays, 0);
        assert_eq!(metrics.retained_runtime_bootstrap_replay_nanos_total, 0);
        assert_eq!(metrics.bundle_loads, 2);
        assert!(metrics.bundle_load_nanos_total > 0);
        assert_eq!(metrics.bundle_module_loads, 2);
        assert!(metrics.bundle_module_load_nanos_total > 0);
        assert_eq!(metrics.bundle_evaluations, 2);
        assert!(metrics.bundle_evaluation_nanos_total > 0);
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
    async fn reused_runtime_refreshes_invocation_cancellation_state_before_next_invoke() {
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
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
        )
        .expect("bundle should write");

        let bundle = RuntimeBundle::new(&bundle_path);
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:get".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        };
        let runtime_owner = NeovexRuntime::new(Arc::new(AsyncEchoHost));
        let mut isolate_pool = RuntimeWorkerIsolatePool::new();
        let mut runtime = isolate_pool
            .take_runtime(&runtime_owner, &bundle)
            .expect("runtime should build from snapshot")
            .runtime;
        runtime_owner
            .load_bundle(&mut runtime, &bundle)
            .await
            .expect("bundle should load");

        let previous_cancel_handle = {
            let op_state = runtime.op_state();
            let state = op_state.borrow();
            let cancellation_state = state.borrow::<RuntimeCancellationState>();
            cancellation_state.signal.cancel();
            assert!(
                cancellation_state.signal.is_cancelled(),
                "test should poison the previous invocation state"
            );
            cancellation_state.cancel_handle.clone()
        };

        let watchdog = WatchdogTimer::new();
        let mut permit =
            SharedInvocationPermit::new(runtime_owner.policy(), None, None, false, None);
        permit
            .acquire_initial(std::time::Instant::now())
            .await
            .expect("permit should admit invocation");

        let mut driver = runtime_owner
            .prepare_runtime_invocation_driver(
                ReusableRuntime::fresh(runtime, RuntimeConstructionMode::StartupSnapshot),
                watchdog.clone(),
                None,
                permit.clone(),
                false,
            )
            .expect("driver preparation should reset invocation state");

        {
            let op_state = driver.runtime.op_state();
            let state = op_state.borrow();
            let cancellation_state = state.borrow::<RuntimeCancellationState>();
            assert!(
                !cancellation_state.signal.is_cancelled(),
                "fresh invocation state should not inherit the previous cancelled signal"
            );
            assert!(
                !Rc::ptr_eq(&previous_cancel_handle, &cancellation_state.cancel_handle),
                "fresh invocation state should replace the previous cancel handle"
            );
        }

        let result = runtime_owner
            .invoke_loaded_bundle(&mut driver.runtime, &request)
            .await
            .expect("fresh invocation state should allow async host work to complete");
        let result = driver
            .finalize(Ok(result))
            .await
            .expect("result should finalize");
        let ready_jobs = permit.finish_invocation().await;

        assert!(ready_jobs.is_empty());
        assert_eq!(
            result,
            serde_json::json!({
                "operation": "convex.ctx.db.get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "query:messages:get",
                },
            })
        );
        watchdog.shutdown();
    }

    #[tokio::test]
    async fn reused_runtime_refreshes_bootstrap_session_state_before_next_invoke() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};").expect("bundle should write");

        let bundle = RuntimeBundle::new(&bundle_path);
        let runtime_owner = NeovexRuntime::new(Arc::new(AsyncEchoHost));
        let mut isolate_pool = RuntimeWorkerIsolatePool::new();
        let mut runtime = isolate_pool
            .take_runtime(&runtime_owner, &bundle)
            .expect("runtime should build from snapshot")
            .runtime;

        async fn issue_default_context_get(runtime: &mut JsRuntime) -> Value {
            let value = runtime
                .execute_script(
                    "<neovex-runtime:test-default-context-get>",
                    r#"(async () => {
  const ctx = globalThis.__neovexCreateContext();
  return await ctx.db.get("messages", "doc-1");
})()"#,
                )
                .expect("test script should execute");
            let resolve = runtime.resolve(value);
            let value = runtime
                .with_event_loop_promise(resolve, PollEventLoopOptions::default())
                .await
                .expect("promise should resolve");
            deserialize_json_value(runtime, value).expect("result should deserialize")
        }

        let first = issue_default_context_get(&mut runtime).await;
        let second_without_reset = issue_default_context_get(&mut runtime).await;

        bootstrap::reset_bootstrap_invocation_state(&mut runtime)
            .expect("bootstrap reset should succeed on reused runtime");

        let third_after_reset = issue_default_context_get(&mut runtime).await;

        assert_eq!(
            first,
            serde_json::json!({
                "operation": "convex.ctx.db.get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "session-1",
                },
            })
        );
        assert_eq!(
            second_without_reset,
            serde_json::json!({
                "operation": "convex.ctx.db.get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "session-2",
                },
            })
        );
        assert_eq!(
            third_after_reset,
            serde_json::json!({
                "operation": "convex.ctx.db.get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "session-1",
                },
            })
        );
    }

    #[tokio::test]
    async fn snapshot_born_runtime_reset_with_full_bootstrap_replay_is_not_supported() {
        init_test_tracing();
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
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
        )
        .expect("bundle should write");

        let bundle = RuntimeBundle::new(&bundle_path);
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:get".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        };
        let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
            &request,
            "snapshot-control",
            "req-snapshot-born-unsupported",
        );
        let runtime_owner = NeovexRuntime::new(Arc::new(AsyncEchoHost));
        let snapshot = runtime_owner
            .bootstrap_snapshot()
            .expect("bootstrap snapshot should build");
        let mut runtime = runtime_owner
            .create_runtime(&bundle, Some(snapshot), false)
            .expect("snapshot-born runtime should build");

        runtime_owner
            .load_bundle_with_trace(
                &mut runtime,
                &bundle,
                RuntimeConstructionMode::StartupSnapshot,
                Some(&context),
                Some(&request),
            )
            .await
            .expect("bundle should load on the snapshot-born runtime");
        runtime_owner
            .invoke_loaded_bundle_with_trace(
                &mut runtime,
                &request,
                Some(&bundle),
                RuntimeConstructionMode::StartupSnapshot,
                Some(&context),
            )
            .await
            .expect("first async host invocation should succeed on the snapshot-born runtime");

        let error = runtime_owner
            .reset_retained_runtime(
                &mut runtime,
                &bundle,
                RuntimeConstructionMode::Unsnapshotted,
            )
            .expect_err(
                "snapshot-born runtimes should not use the unsnapshotted bootstrap replay path",
            );
        let message = error.to_string();
        assert!(
            message.contains("__neovexCoreOps"),
            "reset failure should explain that BOOTSTRAP_SOURCE was replayed into a snapshot-born realm: {message}"
        );
    }

    #[tokio::test]
    async fn snapshot_born_runtime_supports_async_host_after_snapshot_aware_reset() {
        init_test_tracing();
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
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
        )
        .expect("bundle should write");

        let bundle = RuntimeBundle::new(&bundle_path);
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:get".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        };
        let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
            &request,
            "snapshot-control",
            "req-snapshot-born-supported",
        );
        let runtime_owner = NeovexRuntime::new(Arc::new(AsyncEchoHost));
        let snapshot = runtime_owner
            .bootstrap_snapshot()
            .expect("bootstrap snapshot should build");
        let mut runtime = runtime_owner
            .create_runtime(&bundle, Some(snapshot), false)
            .expect("snapshot-born runtime should build");

        runtime_owner
            .load_bundle_with_trace(
                &mut runtime,
                &bundle,
                RuntimeConstructionMode::StartupSnapshot,
                Some(&context),
                Some(&request),
            )
            .await
            .expect("bundle should load on the snapshot-born runtime");
        let first = runtime_owner
            .invoke_loaded_bundle_with_trace(
                &mut runtime,
                &request,
                Some(&bundle),
                RuntimeConstructionMode::StartupSnapshot,
                Some(&context),
            )
            .await
            .expect("first async host invocation should succeed on the snapshot-born runtime");

        let options = CreateRealmOptions {
            module_loader: Some(Rc::new(SandboxedModuleLoader::new(
                bundle.module_root().expect("bundle root should resolve"),
                bundle.module_code_cache(),
            ))),
        };
        runtime
            .reset_main_realm(options)
            .expect("snapshot-born runtime should reset its main realm");
        runtime_owner.initialize_runtime_state(&mut runtime);
        NeovexRuntime::finalize_bootstrap(&mut runtime)
            .expect("snapshot-aware reset should only need finalize_bootstrap");

        runtime_owner
            .load_bundle_with_trace(
                &mut runtime,
                &bundle,
                RuntimeConstructionMode::StartupSnapshot,
                Some(&context),
                Some(&request),
            )
            .await
            .expect("bundle should load after the snapshot-aware reset");
        let second = runtime_owner
            .invoke_loaded_bundle_with_trace(
                &mut runtime,
                &request,
                Some(&bundle),
                RuntimeConstructionMode::StartupSnapshot,
                Some(&context),
            )
            .await
            .expect("second async host invocation should succeed after the snapshot-aware reset");

        let expected = serde_json::json!({
            "operation": "convex.ctx.db.get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "session_id": "query:messages:get",
            }
        });
        assert_eq!(first, expected);
        assert_eq!(second, expected);
    }

    #[tokio::test]
    async fn snapshot_born_runtime_survives_repeated_snapshot_aware_reset_async_host_cycles() {
        init_test_tracing();
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
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
        )
        .expect("bundle should write");

        let bundle = RuntimeBundle::new(&bundle_path);
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:get".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        };
        let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
            &request,
            "snapshot-control",
            "req-snapshot-born-repeated",
        );
        let runtime_owner = NeovexRuntime::new(Arc::new(AsyncEchoHost));
        let snapshot = runtime_owner
            .bootstrap_snapshot()
            .expect("bootstrap snapshot should build");
        let mut runtime = runtime_owner
            .create_runtime(&bundle, Some(snapshot), false)
            .expect("snapshot-born runtime should build");
        let expected = serde_json::json!({
            "operation": "convex.ctx.db.get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "session_id": "query:messages:get",
            }
        });
        let cycles = stress_env_usize("NEOVEX_SNAPSHOT_AWARE_RESET_CYCLES", 32);

        for cycle in 0..cycles {
            if cycle > 0 {
                let options = CreateRealmOptions {
                    module_loader: Some(Rc::new(SandboxedModuleLoader::new(
                        bundle.module_root().expect("bundle root should resolve"),
                        bundle.module_code_cache(),
                    ))),
                };
                runtime
                    .reset_main_realm(options)
                    .expect("snapshot-born runtime should reset its main realm");
                runtime_owner.initialize_runtime_state(&mut runtime);
                NeovexRuntime::finalize_bootstrap(&mut runtime)
                    .expect("snapshot-aware reset should only need finalize_bootstrap");
            }

            runtime_owner
                .load_bundle_with_trace(
                    &mut runtime,
                    &bundle,
                    RuntimeConstructionMode::StartupSnapshot,
                    Some(&context),
                    Some(&request),
                )
                .await
                .expect("bundle should load after each snapshot-aware reset");
            let result = runtime_owner
                .invoke_loaded_bundle_with_trace(
                    &mut runtime,
                    &request,
                    Some(&bundle),
                    RuntimeConstructionMode::StartupSnapshot,
                    Some(&context),
                )
                .await
                .expect("snapshot-aware reset cycle should complete async host work");
            assert_eq!(
                result, expected,
                "snapshot-aware reset cycle {cycle} should preserve the expected async-host result"
            );
        }
    }

    #[tokio::test]
    async fn snapshot_seeded_runtime_driver_cycles_survive_repeated_async_host_invocations() {
        init_test_tracing();
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
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
        )
        .expect("bundle should write");

        let bundle = RuntimeBundle::new(&bundle_path);
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:get".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        };
        let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
            &request,
            "snapshot-control",
            "req-snapshot-driver-cycles",
        );
        let runtime_owner = NeovexRuntime::new(Arc::new(AsyncEchoHost));
        let snapshot = runtime_owner
            .bootstrap_snapshot()
            .expect("bootstrap snapshot should build");
        let runtime = runtime_owner
            .create_runtime(&bundle, Some(snapshot), false)
            .expect("snapshot-born runtime should build");
        let expected = serde_json::json!({
            "operation": "convex.ctx.db.get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "session_id": "query:messages:get",
            }
        });
        let cycles = stress_env_usize("NEOVEX_SNAPSHOT_DRIVER_CYCLES", 32);
        let watchdog = WatchdogTimer::new();
        let mut reusable_runtime =
            ReusableRuntime::fresh(runtime, RuntimeConstructionMode::StartupSnapshot);

        for cycle in 0..cycles {
            let mut permit = SharedInvocationPermit::new(
                runtime_owner.policy(),
                context.tenant_label.clone(),
                None,
                false,
                None,
            );
            permit
                .acquire_initial(std::time::Instant::now())
                .await
                .expect("permit should admit invocation");

            let mut driver = runtime_owner
                .prepare_runtime_invocation_driver(
                    reusable_runtime,
                    watchdog.clone(),
                    None,
                    permit.clone(),
                    false,
                )
                .expect("driver preparation should succeed for snapshot-seeded runtime");

            runtime_owner
                .load_bundle_with_trace(
                    &mut driver.runtime,
                    &bundle,
                    driver.construction_mode,
                    Some(&context),
                    Some(&request),
                )
                .await
                .expect("bundle should load during each direct driver cycle");
            let value = runtime_owner
                .invoke_loaded_bundle_with_trace(
                    &mut driver.runtime,
                    &request,
                    Some(&bundle),
                    driver.construction_mode,
                    Some(&context),
                )
                .await
                .expect("direct driver cycle should complete async host work");
            let (result, returned_runtime) = driver.finalize_with_runtime(Ok(value)).await;
            let ready_jobs = permit.finish_invocation().await;
            assert!(ready_jobs.is_empty(), "no extra jobs should be scheduled");
            assert_eq!(
                result.expect("direct driver cycle should finalize cleanly"),
                expected,
                "driver cycle {cycle} should preserve the expected async-host result"
            );
            reusable_runtime = returned_runtime
                .expect("successful direct driver cycle should return the runtime for reuse");
        }

        watchdog.shutdown();
    }

    #[tokio::test]
    async fn snapshot_seeded_runtime_driver_cycles_survive_with_fresh_runtime_owner_each_cycle() {
        init_test_tracing();
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
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
        )
        .expect("bundle should write");

        let bundle = RuntimeBundle::new(&bundle_path);
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:get".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        };
        let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
            &request,
            "snapshot-control",
            "req-snapshot-driver-fresh-owner",
        );
        let host = Arc::new(AsyncEchoHost);
        let initial_owner = NeovexRuntime::new(host.clone());
        let snapshot = initial_owner
            .bootstrap_snapshot()
            .expect("bootstrap snapshot should build");
        let runtime = initial_owner
            .create_runtime(&bundle, Some(snapshot), false)
            .expect("snapshot-born runtime should build");
        let expected = serde_json::json!({
            "operation": "convex.ctx.db.get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "session_id": "query:messages:get",
            }
        });
        let cycles = stress_env_usize("NEOVEX_SNAPSHOT_DRIVER_FRESH_OWNER_CYCLES", 32);
        let watchdog = WatchdogTimer::new();
        let mut reusable_runtime =
            ReusableRuntime::fresh(runtime, RuntimeConstructionMode::StartupSnapshot);

        for cycle in 0..cycles {
            let runtime_owner = NeovexRuntime::new(host.clone());
            let mut permit = SharedInvocationPermit::new(
                runtime_owner.policy(),
                context.tenant_label.clone(),
                None,
                false,
                None,
            );
            permit
                .acquire_initial(std::time::Instant::now())
                .await
                .expect("permit should admit invocation");

            let mut driver = runtime_owner
                .prepare_runtime_invocation_driver(
                    reusable_runtime,
                    watchdog.clone(),
                    None,
                    permit.clone(),
                    false,
                )
                .expect("driver preparation should succeed for snapshot-seeded runtime");

            runtime_owner
                .load_bundle_with_trace(
                    &mut driver.runtime,
                    &bundle,
                    driver.construction_mode,
                    Some(&context),
                    Some(&request),
                )
                .await
                .expect("bundle should load during each fresh-owner driver cycle");
            let value = runtime_owner
                .invoke_loaded_bundle_with_trace(
                    &mut driver.runtime,
                    &request,
                    Some(&bundle),
                    driver.construction_mode,
                    Some(&context),
                )
                .await
                .expect("fresh-owner driver cycle should complete async host work");
            let (result, returned_runtime) = driver.finalize_with_runtime(Ok(value)).await;
            let ready_jobs = permit.finish_invocation().await;
            assert!(ready_jobs.is_empty(), "no extra jobs should be scheduled");
            assert_eq!(
                result.expect("fresh-owner driver cycle should finalize cleanly"),
                expected,
                "fresh-owner driver cycle {cycle} should preserve the expected async-host result"
            );
            reusable_runtime = returned_runtime
                .expect("successful fresh-owner driver cycle should return the runtime for reuse");
        }

        watchdog.shutdown();
    }

    #[test]
    fn snapshot_seeded_runtime_driver_cycles_survive_on_current_thread_runtime_with_delayed_async_host()
     {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let worker_thread = std::thread::spawn(move || {
            let worker_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("current-thread runtime should build");
            worker_runtime.block_on(async {
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
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
                )
                .expect("bundle should write");

                let bundle = RuntimeBundle::new(&bundle_path);
                let request = InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                };
                let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
                    &request,
                    "snapshot-control",
                    "req-snapshot-driver-current-thread-delayed",
                );
                let runtime_owner =
                    NeovexRuntime::new(Arc::new(DelayedAsyncEchoHost::new(
                        std::time::Duration::from_millis(1),
                    )));
                let snapshot = runtime_owner
                    .bootstrap_snapshot()
                    .expect("bootstrap snapshot should build");
                let runtime = runtime_owner
                    .create_runtime(&bundle, Some(snapshot), false)
                    .expect("snapshot-born runtime should build");
                let expected = serde_json::json!({
                    "operation": "convex.ctx.db.get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "query:messages:get",
                    }
                });
                let cycles =
                    stress_env_usize("NEOVEX_SNAPSHOT_DRIVER_CURRENT_THREAD_CYCLES", 32);
                let watchdog = WatchdogTimer::new();
                let mut reusable_runtime =
                    ReusableRuntime::fresh(runtime, RuntimeConstructionMode::StartupSnapshot);

                for cycle in 0..cycles {
                    let mut permit = SharedInvocationPermit::new(
                        runtime_owner.policy(),
                        context.tenant_label.clone(),
                        None,
                        false,
                        None,
                    );
                    permit
                        .acquire_initial(std::time::Instant::now())
                        .await
                        .expect("permit should admit invocation");

                    let mut driver = runtime_owner
                        .prepare_runtime_invocation_driver(
                            reusable_runtime,
                            watchdog.clone(),
                            None,
                            permit.clone(),
                            false,
                        )
                        .expect(
                            "driver preparation should succeed for snapshot-seeded delayed runtime",
                        );

                    runtime_owner
                        .load_bundle_with_trace(
                            &mut driver.runtime,
                            &bundle,
                            driver.construction_mode,
                            Some(&context),
                            Some(&request),
                        )
                        .await
                        .expect(
                            "bundle should load during each delayed current-thread driver cycle",
                        );
                    let value = runtime_owner
                        .invoke_loaded_bundle_with_trace(
                            &mut driver.runtime,
                            &request,
                            Some(&bundle),
                            driver.construction_mode,
                            Some(&context),
                        )
                        .await
                        .expect(
                            "delayed current-thread driver cycle should complete async host work",
                        );
                    let (result, returned_runtime) = driver.finalize_with_runtime(Ok(value)).await;
                    let ready_jobs = permit.finish_invocation().await;
                    assert!(ready_jobs.is_empty(), "no extra jobs should be scheduled");
                    assert_eq!(
                        result.expect(
                            "delayed current-thread driver cycle should finalize cleanly"
                        ),
                        expected,
                        "delayed current-thread driver cycle {cycle} should preserve the expected async-host result"
                    );
                    reusable_runtime = returned_runtime.expect(
                        "successful delayed current-thread driver cycle should return the runtime for reuse",
                    );
                }

                watchdog.shutdown();
            });
        });

        worker_thread
            .join()
            .expect("current-thread worker thread should not panic");
    }

    #[test]
    fn snapshot_born_reset_cycles_with_delayed_async_host() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let worker_thread = std::thread::spawn(move || {
            let worker_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("current-thread runtime should build");
            worker_runtime.block_on(async {
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
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
                )
                .expect("bundle should write");

                let bundle = RuntimeBundle::new(&bundle_path);
                let request = InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                };
                let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
                    &request,
                    "snapshot-control",
                    "req-snapshot-aware-reset-current-thread-delayed",
                );
                let runtime_owner =
                    NeovexRuntime::new(Arc::new(DelayedAsyncEchoHost::new(
                        std::time::Duration::from_millis(1),
                    )));
                let snapshot = runtime_owner
                    .bootstrap_snapshot()
                    .expect("bootstrap snapshot should build");
                let mut runtime = runtime_owner
                    .create_runtime(&bundle, Some(snapshot), false)
                    .expect("snapshot-born runtime should build");
                let expected = serde_json::json!({
                    "operation": "convex.ctx.db.get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "query:messages:get",
                    }
                });
                let cycles = stress_env_usize(
                    "NEOVEX_SNAPSHOT_AWARE_RESET_CURRENT_THREAD_DELAYED_CYCLES",
                    32,
                );

                for cycle in 0..cycles {
                    if cycle > 0 {
                        let options = CreateRealmOptions {
                            module_loader: Some(Rc::new(SandboxedModuleLoader::new(
                                bundle.module_root().expect("bundle root should resolve"),
                                bundle.module_code_cache(),
                            ))),
                        };
                        runtime
                            .reset_main_realm(options)
                            .expect("snapshot-born runtime should reset its main realm");
                        runtime_owner.initialize_runtime_state(&mut runtime);
                        NeovexRuntime::finalize_bootstrap(&mut runtime)
                            .expect("snapshot-aware reset should only need finalize_bootstrap");
                    }

                    runtime_owner
                        .load_bundle_with_trace(
                            &mut runtime,
                            &bundle,
                            RuntimeConstructionMode::StartupSnapshot,
                            Some(&context),
                            Some(&request),
                        )
                        .await
                        .expect(
                            "bundle should load after each delayed snapshot-aware reset cycle",
                        );
                    let result = runtime_owner
                        .invoke_loaded_bundle_with_trace(
                            &mut runtime,
                            &request,
                            Some(&bundle),
                            RuntimeConstructionMode::StartupSnapshot,
                            Some(&context),
                        )
                        .await
                        .expect(
                            "delayed snapshot-aware reset cycle should complete async host work",
                        );
                    assert_eq!(
                        result, expected,
                        "delayed snapshot-aware reset cycle {cycle} should preserve the expected async-host result"
                    );
                }
            });
        });

        worker_thread
            .join()
            .expect("current-thread snapshot-aware delayed async host thread should not panic");
    }

    #[test]
    fn snapshot_born_reset_cycles_with_delayed_async_script() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let worker_thread = std::thread::spawn(|| {
            let worker_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("current-thread runtime should build");
            worker_runtime.block_on(async {
                let tempdir = tempdir().expect("tempdir should build");
                let bundle_path = tempdir.path().join("bundle.mjs");
                std::fs::write(&bundle_path, "export {};").expect("bundle should write");

                let bundle = RuntimeBundle::new(&bundle_path);
                let request = InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                };
                let runtime_owner =
                    NeovexRuntime::new(Arc::new(DelayedAsyncEchoHost::new(
                        std::time::Duration::from_millis(1),
                    )));
                let snapshot = runtime_owner
                    .bootstrap_snapshot()
                    .expect("bootstrap snapshot should build");
                let mut runtime = runtime_owner
                    .create_runtime(&bundle, Some(snapshot), false)
                    .expect("snapshot-born runtime should build");
                let request_json =
                    serde_json::to_string(&request).expect("request should serialize");
                let expression = format!(
                    r#"(async () => {{
  const ctx = globalThis.__neovexCreateContext({{
    request: {request_json},
    sessionId: "query:messages:get",
  }});
  return await ctx.db.get("messages", "doc-1");
}})()"#
                );
                let expected = serde_json::json!({
                    "operation": "convex.ctx.db.get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "query:messages:get",
                    }
                });
                let cycles = stress_env_usize(
                    "NEOVEX_SNAPSHOT_AWARE_RESET_CURRENT_THREAD_DELAYED_SCRIPT_CYCLES",
                    32,
                );

                for cycle in 0..cycles {
                    if cycle > 0 {
                        runtime
                            .reset_main_realm(CreateRealmOptions::default())
                            .expect("snapshot-born runtime should reset its main realm");
                        runtime_owner.initialize_runtime_state(&mut runtime);
                        NeovexRuntime::finalize_bootstrap(&mut runtime)
                            .expect("snapshot-aware reset should only need finalize_bootstrap");
                    }

                    let value = runtime
                        .execute_script(
                            "<neovex-runtime:reset-delayed-script>",
                            expression.clone(),
                        )
                        .expect("reset-delayed script should execute");
                    let resolve = runtime.resolve(value);
                    let value = runtime
                        .with_event_loop_promise(resolve, PollEventLoopOptions::default())
                        .await
                        .expect("reset-delayed script should resolve its async host work");
                    let result = deserialize_json_value(&mut runtime, value)
                        .expect("reset-delayed script result should deserialize");
                    assert_eq!(
                        result, expected,
                        "delayed snapshot-aware reset script cycle {cycle} should preserve the expected async-host result"
                    );
                }
            });
        });

        worker_thread
            .join()
            .expect("current-thread snapshot-aware delayed script thread should not panic");
    }

    fn run_snapshot_born_reset_cycles_with_bundle_load(
        cycles: usize,
        fresh_bundle_each_cycle: bool,
        yield_before_reset: bool,
        settle_before_reset: bool,
        settle_after_reset: bool,
    ) {
        let worker_thread = std::thread::spawn(move || {
            let worker_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("current-thread runtime should build");
            worker_runtime.block_on(async {
                let tempdir = tempdir().expect("tempdir should build");
                let bundle_path = tempdir.path().join("bundle.mjs");
                std::fs::write(
                    &bundle_path,
                    r#"
globalThis.__bundleLoaded = (globalThis.__bundleLoaded ?? 0) + 1;
export {};
"#,
                )
                .expect("bundle should write");

                let bundle = RuntimeBundle::new(&bundle_path);
                let request = InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                };
                let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
                    &request,
                    "snapshot-control",
                    "req-snapshot-aware-reset-bundle-then-script",
                );
                let runtime_owner =
                    NeovexRuntime::new(Arc::new(DelayedAsyncEchoHost::new(
                        std::time::Duration::from_millis(1),
                    )));
                let snapshot = runtime_owner
                    .bootstrap_snapshot()
                    .expect("bootstrap snapshot should build");
                let mut runtime = runtime_owner
                    .create_runtime(&bundle, Some(snapshot), false)
                    .expect("snapshot-born runtime should build");
                let request_json =
                    serde_json::to_string(&request).expect("request should serialize");
                let expression = format!(
                    r#"(async () => {{
  const ctx = globalThis.__neovexCreateContext({{
    request: {request_json},
    sessionId: "query:messages:get",
  }});
  return await ctx.db.get("messages", "doc-1");
}})()"#
                );
                let expected = serde_json::json!({
                    "operation": "convex.ctx.db.get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "query:messages:get",
                    }
                });
                for cycle in 0..cycles {
                    let cycle_bundle = if fresh_bundle_each_cycle {
                        RuntimeBundle::new(&bundle_path)
                    } else {
                        bundle.clone()
                    };
                    if cycle > 0 {
                        if yield_before_reset {
                            tokio::task::yield_now().await;
                        }
                        if settle_before_reset {
                            runtime
                                .run_event_loop(Default::default())
                                .await
                                .expect("pre-reset settle should succeed");
                        }
                        // Drain the event loop before reset to clean up pending
                        // async operations from the previous cycle. Without this,
                        // V8's internal promise queue state corrupts the reset.
                        runtime
                            .run_event_loop(Default::default())
                            .await
                            .expect("pre-reset event loop drain should succeed");
                        let options = CreateRealmOptions {
                            module_loader: Some(Rc::new(SandboxedModuleLoader::new(
                                cycle_bundle
                                    .module_root()
                                    .expect("bundle root should resolve"),
                                cycle_bundle.module_code_cache(),
                            ))),
                        };
                        runtime
                            .reset_main_realm(options)
                            .expect("snapshot-born runtime should reset its main realm");
                        runtime_owner.initialize_runtime_state(&mut runtime);
                        NeovexRuntime::finalize_bootstrap(&mut runtime)
                            .expect("snapshot-aware reset should only need finalize_bootstrap");
                        if settle_after_reset {
                            runtime
                                .run_event_loop(Default::default())
                                .await
                                .expect("post-reset settle should succeed");
                        }
                    }

                    runtime_owner
                        .load_bundle_with_trace(
                            &mut runtime,
                            &cycle_bundle,
                            RuntimeConstructionMode::StartupSnapshot,
                            Some(&context),
                            Some(&request),
                        )
                        .await
                        .expect("bundle should load before the delayed script");

                    let value = runtime
                        .execute_script(
                            "<neovex-runtime:reset-delayed-script-after-bundle-load>",
                            expression.clone(),
                        )
                        .expect("reset-delayed script should execute after bundle load");
                    let resolve = runtime.resolve(value);
                    let value = runtime
                        .with_event_loop_promise(resolve, PollEventLoopOptions::default())
                        .await
                        .expect("reset-delayed script should resolve its async host work");
                    let result = deserialize_json_value(&mut runtime, value)
                        .expect("reset-delayed script result should deserialize");
                    assert_eq!(
                        result, expected,
                        "delayed snapshot-aware reset bundle-then-script cycle {cycle} should preserve the expected async-host result"
                    );
                }
            });
        });

        worker_thread.join().expect(
            "current-thread snapshot-aware delayed bundle-then-script thread should not panic",
        );
    }

    #[test]
    fn snapshot_born_reset_cycles_with_bundle_load() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let cycles = stress_env_usize(
            "NEOVEX_SNAPSHOT_AWARE_RESET_CURRENT_THREAD_DELAYED_BUNDLE_THEN_SCRIPT_CYCLES",
            8,
        );
        run_snapshot_born_reset_cycles_with_bundle_load(cycles, false, false, false, false);
    }

    #[test]
    fn snapshot_born_reset_cycles_with_bundle_load_stress() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let cycles = stress_env_usize(
            "NEOVEX_SNAPSHOT_AWARE_RESET_CURRENT_THREAD_DELAYED_BUNDLE_THEN_SCRIPT_STRESS_CYCLES",
            32,
        );
        run_snapshot_born_reset_cycles_with_bundle_load(cycles, false, false, false, false);
    }

    #[test]
    fn snapshot_born_reset_cycles_with_fresh_bundle_cache() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let cycles = stress_env_usize(
            "NEOVEX_SNAPSHOT_AWARE_RESET_CURRENT_THREAD_DELAYED_BUNDLE_THEN_SCRIPT_FRESH_CACHE_CYCLES",
            32,
        );
        run_snapshot_born_reset_cycles_with_bundle_load(cycles, true, false, false, false);
    }

    #[test]
    fn snapshot_born_reset_cycles_with_pre_reset_settle() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let cycles = stress_env_usize(
            "NEOVEX_SNAPSHOT_AWARE_RESET_CURRENT_THREAD_DELAYED_BUNDLE_THEN_SCRIPT_PRE_RESET_SETTLE_CYCLES",
            32,
        );
        run_snapshot_born_reset_cycles_with_bundle_load(cycles, false, false, true, false);
    }

    #[test]
    fn snapshot_born_reset_cycles_with_pre_reset_yield_and_settle() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let cycles = stress_env_usize(
            "NEOVEX_SNAPSHOT_AWARE_RESET_CURRENT_THREAD_DELAYED_BUNDLE_THEN_SCRIPT_PRE_RESET_YIELD_AND_SETTLE_CYCLES",
            32,
        );
        run_snapshot_born_reset_cycles_with_bundle_load(cycles, false, true, true, false);
    }

    #[test]
    fn snapshot_born_reset_cycles_with_post_reset_settle() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let cycles = stress_env_usize(
            "NEOVEX_SNAPSHOT_AWARE_RESET_CURRENT_THREAD_DELAYED_BUNDLE_THEN_SCRIPT_POST_RESET_SETTLE_CYCLES",
            32,
        );
        run_snapshot_born_reset_cycles_with_bundle_load(cycles, false, false, false, true);
    }

    #[test]
    fn snapshot_born_reset_cycles_with_pre_and_post_reset_settle() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let cycles = stress_env_usize(
            "NEOVEX_SNAPSHOT_AWARE_RESET_CURRENT_THREAD_DELAYED_BUNDLE_THEN_SCRIPT_PRE_AND_POST_RESET_SETTLE_CYCLES",
            32,
        );
        run_snapshot_born_reset_cycles_with_bundle_load(cycles, false, false, true, true);
    }

    #[test]
    fn snapshot_born_reset_cycles_with_extra_drain() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let worker_thread = std::thread::spawn(|| {
            let worker_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("current-thread runtime should build");
            worker_runtime.block_on(async {
                let tempdir = tempdir().expect("tempdir should build");
                let bundle_path = tempdir.path().join("bundle.mjs");
                std::fs::write(
                    &bundle_path,
                    r#"
globalThis.__bundleLoaded = (globalThis.__bundleLoaded ?? 0) + 1;
export {};
"#,
                )
                .expect("bundle should write");

                let bundle = RuntimeBundle::new(&bundle_path);
                let request = InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                };
                let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
                    &request,
                    "snapshot-control",
                    "req-snapshot-aware-reset-bundle-then-script-extra-drain",
                );
                let runtime_owner =
                    NeovexRuntime::new(Arc::new(DelayedAsyncEchoHost::new(
                        std::time::Duration::from_millis(1),
                    )));
                let snapshot = runtime_owner
                    .bootstrap_snapshot()
                    .expect("bootstrap snapshot should build");
                let mut runtime = runtime_owner
                    .create_runtime(&bundle, Some(snapshot), false)
                    .expect("snapshot-born runtime should build");
                let request_json =
                    serde_json::to_string(&request).expect("request should serialize");
                let expression = format!(
                    r#"(async () => {{
  const ctx = globalThis.__neovexCreateContext({{
    request: {request_json},
    sessionId: "query:messages:get",
  }});
  return await ctx.db.get("messages", "doc-1");
}})()"#
                );
                let expected = serde_json::json!({
                    "operation": "convex.ctx.db.get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "query:messages:get",
                    }
                });
                let cycles = stress_env_usize(
                    "NEOVEX_SNAPSHOT_AWARE_RESET_CURRENT_THREAD_DELAYED_BUNDLE_THEN_SCRIPT_EXTRA_DRAIN_CYCLES",
                    32,
                );

                for cycle in 0..cycles {
                    if cycle > 0 {
                        let options = CreateRealmOptions {
                            module_loader: Some(Rc::new(SandboxedModuleLoader::new(
                                bundle.module_root().expect("bundle root should resolve"),
                                bundle.module_code_cache(),
                            ))),
                        };
                        runtime
                            .reset_main_realm(options)
                            .expect("snapshot-born runtime should reset its main realm");
                        runtime_owner.initialize_runtime_state(&mut runtime);
                        NeovexRuntime::finalize_bootstrap(&mut runtime)
                            .expect("snapshot-aware reset should only need finalize_bootstrap");
                    }

                    runtime_owner
                        .load_bundle_for_bypass_repro_without_post_return_settle(
                            &mut runtime,
                            &bundle,
                            RuntimeConstructionMode::StartupSnapshot,
                            Some(&context),
                            Some(&request),
                        )
                        .await
                        .expect("bundle should load before the delayed script");

                    runtime
                        .run_event_loop(Default::default())
                        .await
                        .expect("extra post-bundle drain should succeed");

                    let value = runtime
                        .execute_script(
                            "<neovex-runtime:reset-delayed-script-after-bundle-load-extra-drain>",
                            expression.clone(),
                        )
                        .expect("reset-delayed script should execute after bundle load");
                    let resolve = runtime.resolve(value);
                    let value = runtime
                        .with_event_loop_promise(resolve, PollEventLoopOptions::default())
                        .await
                        .expect("reset-delayed script should resolve its async host work");
                    let result = deserialize_json_value(&mut runtime, value)
                        .expect("reset-delayed script result should deserialize");
                    assert_eq!(
                        result, expected,
                        "delayed snapshot-aware reset bundle-then-script extra-drain cycle {cycle} should preserve the expected async-host result"
                    );
                }
            });
        });

        worker_thread.join().expect(
            "current-thread snapshot-aware delayed bundle-then-script extra-drain thread should not panic",
        );
    }

    #[test]
    fn snapshot_born_reset_cycles_with_tokio_yield() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();

        let worker_thread = std::thread::spawn(|| {
            let tokio_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("current-thread tokio runtime should build");

            tokio_runtime.block_on(async {
                let bundle_dir = tempfile::tempdir().expect("bundle tempdir should exist");
                let bundle_path = bundle_dir.path().join("bundle.mjs");
                std::fs::write(
                    &bundle_path,
                    r#"
                    export async function run(ctx) {
                      return await ctx.db.get("messages", "doc-1");
                    }
                    "#,
                )
                .expect("bundle source should write");

                let bundle = RuntimeBundle::new(&bundle_path);
                let request = InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                };
                let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
                    &request,
                    "snapshot-control",
                    "req-snapshot-aware-reset-bundle-then-script-tokio-yield",
                );
                let runtime_owner =
                    NeovexRuntime::new(Arc::new(DelayedAsyncEchoHost::new(
                        std::time::Duration::from_millis(1),
                    )));
                let snapshot = runtime_owner
                    .bootstrap_snapshot()
                    .expect("bootstrap snapshot should build");
                let mut runtime = runtime_owner
                    .create_runtime(&bundle, Some(snapshot), false)
                    .expect("snapshot-born runtime should build");
                let request_json =
                    serde_json::to_string(&request).expect("request should serialize");
                let expression = format!(
                    r#"(async () => {{
  const ctx = globalThis.__neovexCreateContext({{
    request: {request_json},
    sessionId: "query:messages:get",
  }});
  return await ctx.db.get("messages", "doc-1");
}})()"#
                );
                let expected = serde_json::json!({
                    "operation": "convex.ctx.db.get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "query:messages:get",
                    }
                });
                let cycles = stress_env_usize(
                    "NEOVEX_SNAPSHOT_AWARE_RESET_CURRENT_THREAD_DELAYED_BUNDLE_THEN_SCRIPT_TOKIO_YIELD_CYCLES",
                    32,
                );

                for cycle in 0..cycles {
                    if cycle > 0 {
                        let options = CreateRealmOptions {
                            module_loader: Some(Rc::new(SandboxedModuleLoader::new(
                                bundle.module_root().expect("bundle root should resolve"),
                                bundle.module_code_cache(),
                            ))),
                        };
                        runtime
                            .reset_main_realm(options)
                            .expect("snapshot-born runtime should reset its main realm");
                        runtime_owner.initialize_runtime_state(&mut runtime);
                        NeovexRuntime::finalize_bootstrap(&mut runtime)
                            .expect("snapshot-aware reset should only need finalize_bootstrap");
                    }

                    runtime_owner
                        .load_bundle_for_bypass_repro_without_post_return_settle(
                            &mut runtime,
                            &bundle,
                            RuntimeConstructionMode::StartupSnapshot,
                            Some(&context),
                            Some(&request),
                        )
                        .await
                        .expect("bundle should load before the delayed script");

                    tokio::task::yield_now().await;

                    let value = runtime
                        .execute_script(
                            "<neovex-runtime:reset-delayed-script-after-bundle-load-tokio-yield>",
                            expression.clone(),
                        )
                        .expect("reset-delayed script should execute after bundle load");
                    let resolve = runtime.resolve(value);
                    let value = runtime
                        .with_event_loop_promise(resolve, PollEventLoopOptions::default())
                        .await
                        .expect("reset-delayed script should resolve its async host work");
                    let result = deserialize_json_value(&mut runtime, value)
                        .expect("reset-delayed script result should deserialize");
                    assert_eq!(
                        result, expected,
                        "delayed snapshot-aware reset bundle-then-script tokio-yield cycle {cycle} should preserve the expected async-host result"
                    );
                }
            });
        });

        worker_thread.join().expect(
            "current-thread snapshot-aware delayed bundle-then-script tokio-yield thread should not panic",
        );
    }

    #[test]
    fn snapshot_seeded_retained_pool_multi_tenant_reset_cycles() {
        init_test_tracing();
        let _test_lock = acquire_snapshot_reset_test_lock();
        let worker_thread = std::thread::spawn(|| {
            let worker_runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("current-thread runtime should build");
            worker_runtime.block_on(async {
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
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
                )
                .expect("bundle should write");

                let bundle = RuntimeBundle::new(&bundle_path);
                let request = InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                };
                let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
                    runtime_pool_kind: crate::limits::RuntimePoolKind::RetainedJsRuntimePool,
                    routing_affinity: crate::limits::RuntimeRoutingAffinity::Tenant,
                    max_concurrent_isolates: 1,
                    worker_threads: 1,
                    max_retained_runtimes_per_worker: 4,
                    max_retained_runtimes_per_affinity_key_per_worker: 1,
                    ..RuntimeLimits::default()
                }));
                let runtime_owner = NeovexRuntime::with_policy(
                    Arc::new(DelayedAsyncEchoHost::new(std::time::Duration::from_millis(
                        1,
                    ))),
                    policy.clone(),
                )
                .with_retained_runtime_construction_mode_for_test(
                    RuntimeConstructionMode::StartupSnapshot,
                );
                let expected = serde_json::json!({
                    "operation": "convex.ctx.db.get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "query:messages:get",
                    }
                });
                let cycles = stress_env_usize(
                    "NEOVEX_SNAPSHOT_MULTI_RUNTIME_CURRENT_THREAD_CYCLES",
                    16,
                );
                let tenants = ["tenant-a", "tenant-b", "tenant-c", "tenant-d"];
                let watchdog = WatchdogTimer::new();
                let mut isolate_pool = RuntimeWorkerIsolatePool::new();

                for cycle in 0..cycles {
                    for tenant in tenants {
                        let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
                            &request,
                            tenant,
                            format!(
                                "req-snapshot-multi-runtime-current-thread-{cycle}-{tenant}"
                            ),
                        );
                        let reusable_runtime = isolate_pool
                            .take_runtime_for_invocation(
                                &runtime_owner,
                                &bundle,
                                Some(&context),
                            )
                            .expect(
                                "snapshot-seeded multi-tenant retained runtime should build or reuse",
                            );
                        let mut permit = SharedInvocationPermit::new(
                            runtime_owner.policy(),
                            context.tenant_label.clone(),
                            None,
                            false,
                            None,
                        );
                        permit
                            .acquire_initial(std::time::Instant::now())
                            .await
                            .expect("permit should admit invocation");

                        let mut driver = runtime_owner
                            .prepare_runtime_invocation_driver(
                                reusable_runtime,
                                watchdog.clone(),
                                None,
                                permit.clone(),
                                false,
                            )
                            .expect(
                                "driver preparation should succeed for snapshot-seeded retained runtime",
                            );

                        runtime_owner
                            .load_bundle_for_bypass_repro_without_post_return_settle(
                                &mut driver.runtime,
                                &bundle,
                                driver.construction_mode,
                                Some(&context),
                                Some(&request),
                            )
                            .await
                            .expect(
                                "bundle should load during each multi-tenant retained cycle",
                            );
                        let value = runtime_owner
                            .invoke_loaded_bundle_with_trace(
                                &mut driver.runtime,
                                &request,
                                Some(&bundle),
                                driver.construction_mode,
                                Some(&context),
                            )
                            .await
                            .expect(
                                "multi-tenant retained cycle should complete async host work",
                            );
                        let (result, returned_runtime) =
                            driver.finalize_with_runtime(Ok(value)).await;
                        let ready_jobs = permit.finish_invocation().await;
                        assert!(ready_jobs.is_empty(), "no extra jobs should be scheduled");
                        assert_eq!(
                            result.expect(
                                "multi-tenant retained cycle should finalize cleanly"
                            ),
                            expected,
                            "multi-tenant retained cycle {cycle} for {tenant} should preserve the expected async-host result",
                        );
                        isolate_pool.return_runtime_for_invocation(
                            &runtime_owner,
                            &bundle,
                            Some(&context),
                            returned_runtime.expect(
                                "successful multi-tenant retained cycle should return the runtime for reuse",
                            ),
                        );
                    }
                }

                watchdog.shutdown();
            });
        });

        worker_thread
            .join()
            .expect("current-thread worker multi-tenant thread should not panic");
    }

    #[tokio::test]
    async fn reused_runtime_still_leaks_user_module_state_after_current_resets() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__userCounter = globalThis.__userCounter ?? 0;

globalThis.__neovexInvoke = function () {
  globalThis.__userCounter += 1;
  return { counter: globalThis.__userCounter };
};

export {};
"#,
        )
        .expect("bundle should write");

        let bundle = RuntimeBundle::new(&bundle_path);
        let runtime_owner = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let mut isolate_pool = RuntimeWorkerIsolatePool::new();
        let mut runtime = isolate_pool
            .take_runtime(&runtime_owner, &bundle)
            .expect("runtime should build from snapshot")
            .runtime;
        runtime_owner
            .load_bundle(&mut runtime, &bundle)
            .await
            .expect("bundle should load");

        async fn invoke_with_current_reset_contract(
            runtime_owner: &NeovexRuntime,
            runtime: &mut JsRuntime,
        ) -> Value {
            bootstrap::reset_runtime_invocation_state(
                runtime,
                SharedInvocationPermit::new(runtime_owner.policy(), None, None, true, None),
            );
            bootstrap::reset_bootstrap_invocation_state(runtime)
                .expect("bootstrap invocation reset should succeed");
            runtime_owner
                .invoke_loaded_bundle(
                    runtime,
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
                .expect("invocation should succeed")
        }

        let first = invoke_with_current_reset_contract(&runtime_owner, &mut runtime).await;
        let second = invoke_with_current_reset_contract(&runtime_owner, &mut runtime).await;

        assert_eq!(first, serde_json::json!({ "counter": 1 }));
        assert_eq!(
            second,
            serde_json::json!({ "counter": 2 }),
            "user-module/global state still persists on a reused loaded runtime even after the current Rust-side and bootstrap-state resets"
        );
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

    #[tokio::test]
    async fn startup_snapshot_runtime_populates_and_reuses_bundle_module_code_cache() {
        let tempdir = tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        let dep_path = tempdir.path().join("dep.mjs");
        std::fs::write(
            &dep_path,
            r#"
export function value() {
  return "cached";
}
"#,
        )
        .expect("dependency should write");
        std::fs::write(
            &bundle_path,
            r#"
import { value } from "./dep.mjs";

globalThis.__neovexInvoke = async function () {
  return { value: value() };
};

export {};
"#,
        )
        .expect("bundle should write");

        let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
        let bundle = RuntimeBundle::new(&bundle_path);
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "messages:list".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        };

        assert_eq!(bundle.module_code_cache_entry_count(), 0);
        assert_eq!(bundle.module_code_cache_write_count(), 0);

        let first = runtime
            .invoke_bundle(&bundle, &request)
            .await
            .expect("first invocation should succeed");
        assert_eq!(first, serde_json::json!({ "value": "cached" }));

        let first_entry_count = bundle.module_code_cache_entry_count();
        let first_write_count = bundle.module_code_cache_write_count();
        assert!(
            first_entry_count >= 2,
            "expected main module and dependency to populate cache"
        );
        assert!(
            first_write_count >= first_entry_count,
            "expected at least one cache write per populated module"
        );

        let second = runtime
            .invoke_bundle(&bundle, &request)
            .await
            .expect("second invocation should succeed");
        assert_eq!(second, serde_json::json!({ "value": "cached" }));
        assert_eq!(bundle.module_code_cache_entry_count(), first_entry_count);
        assert_eq!(bundle.module_code_cache_write_count(), first_write_count);
        let metrics = runtime.policy.metrics_snapshot();
        assert_eq!(metrics.bundle_loads, 2);
        assert!(metrics.bundle_load_nanos_total > 0);
        assert_eq!(metrics.bundle_module_loads, 2);
        assert!(metrics.bundle_module_load_nanos_total > 0);
        assert_eq!(metrics.bundle_evaluations, 2);
        assert!(metrics.bundle_evaluation_nanos_total > 0);
        assert_eq!(metrics.retained_runtime_main_realm_resets, 0);
        assert_eq!(metrics.retained_runtime_bootstrap_replays, 0);
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
        assert_eq!(
            bundle
                .module_root()
                .expect("bundle root should resolve from cached metadata"),
            canonical_bundle_path
                .parent()
                .expect("bundle root should exist")
                .to_path_buf()
        );
        assert_eq!(
            bundle
                .module_specifier()
                .expect("bundle specifier should resolve from cached metadata")
                .as_str(),
            deno_core::ModuleSpecifier::from_file_path(&canonical_bundle_path)
                .expect("canonical bundle path should convert to a file url")
                .as_str()
        );
        assert_eq!(
            cloned
                .module_root()
                .expect("cloned bundle should share cached root metadata"),
            canonical_bundle_path
                .parent()
                .expect("bundle root should exist")
                .to_path_buf()
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
