use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};

use deno_fs::sync::MaybeArc;
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::time::Instant;

use super::*;
use crate::host::{HostBridgeFuture, HostCallCancellation, HostCallOperation, HostCallRequest};
use crate::limits::{RuntimeLimits, RuntimePolicy};

pub(super) use tempfile::tempdir;

pub(super) fn init_test_tracing() {
    static TRACING_INIT: OnceLock<()> = OnceLock::new();
    TRACING_INIT.get_or_init(|| {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::DEBUG)
            .without_time()
            .try_init();
    });
}

pub(super) fn usize_env_or(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

pub(super) fn duration_ms_env_or(name: &str, default: std::time::Duration) -> std::time::Duration {
    let default_ms = default.as_millis().min(u64::MAX as u128) as u64;
    std::time::Duration::from_millis(
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(default_ms),
    )
}

pub(super) fn ci_or_local_duration(
    local: std::time::Duration,
    ci: std::time::Duration,
) -> std::time::Duration {
    if std::env::var_os("CI").is_some() {
        ci
    } else {
        local
    }
}

pub(super) fn runtime_test_policy_with_real_fs(limits: RuntimeLimits) -> Arc<RuntimePolicy> {
    Arc::new(RuntimePolicy::new(limits).clone_with_file_system(MaybeArc::new(deno_fs::RealFs)))
}

pub(super) async fn wait_for_condition<F, Fut>(
    description: &str,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
    condition: F,
) where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    wait_for_value(description, timeout, poll_interval, condition, |ready| {
        *ready
    })
    .await;
}

pub(super) async fn wait_for_value<T, F, Fut, P>(
    description: &str,
    timeout: std::time::Duration,
    poll_interval: std::time::Duration,
    mut load: F,
    mut predicate: P,
) -> T
where
    F: FnMut() -> Fut,
    Fut: Future<Output = T>,
    P: FnMut(&T) -> bool,
{
    let started_at = Instant::now();
    let mut attempts = 0_u64;
    loop {
        attempts += 1;
        let value = load().await;
        if predicate(&value) {
            return value;
        }
        let elapsed = started_at.elapsed();
        if elapsed >= timeout {
            panic!(
                "timed out waiting for {description} after {elapsed:?} (budget {timeout:?}, poll interval {poll_interval:?}, attempts {attempts})"
            );
        }
        if poll_interval.is_zero() {
            tokio::task::yield_now().await;
        } else {
            tokio::time::sleep(poll_interval).await;
        }
    }
}

#[derive(Default)]
pub(super) struct RecordingHost {
    pub(super) calls: Mutex<Vec<HostCallRequest>>,
    // When set, answers the callee-lane oracle (`CtxResolveCalleeLane`) with this
    // lane for every callee — the host standing in for a registry whose
    // functions all share this isolate's lane, so same-isolate local dispatch is
    // taken. `None` reports every callee as unresolved (null), which fails safe
    // to host dispatch.
    resolve_lane: Option<String>,
}

impl RecordingHost {
    /// A recording host that resolves every nested callee to `lane`, so
    /// same-lane nested `ctx.run*` calls take the local-dispatch fast path.
    pub(super) fn resolving_lane(lane: impl Into<String>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            resolve_lane: Some(lane.into()),
        }
    }
}

impl HostBridge for RecordingHost {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        self.calls
            .lock()
            .expect("recording host lock should not be poisoned")
            .push(request.clone());
        if request.operation == HostCallOperation::CtxResolveCalleeLane {
            let value = match &self.resolve_lane {
                Some(lane) => Value::String(lane.clone()),
                None => Value::Null,
            };
            return Ok(serde_json::json!({ "status": "ok", "value": value }));
        }
        Ok(serde_json::json!({
            "operation": request.operation,
            "payload": request.payload,
        }))
    }
}

pub(super) struct DelayedAsyncEnvelopeHost {
    pub(super) delay: std::time::Duration,
}

impl HostBridge for DelayedAsyncEnvelopeHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "sync host bridge path should not be used for delayed async ops".to_string(),
        ))
    }

    fn call_async(
        &self,
        _request: HostCallRequest,
        _cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let delay = self.delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(serde_json::json!({
                "status": "ok",
                "value": Value::Null,
            }))
        })
    }
}

pub(super) struct CountingDelayedAsyncEnvelopeHost {
    delay: std::time::Duration,
    calls: Mutex<usize>,
}

impl CountingDelayedAsyncEnvelopeHost {
    pub(super) fn new(delay: std::time::Duration) -> Self {
        Self {
            delay,
            calls: Mutex::new(0),
        }
    }

    pub(super) fn calls(&self) -> usize {
        *self
            .calls
            .lock()
            .expect("counting delayed host lock should not be poisoned")
    }
}

impl HostBridge for CountingDelayedAsyncEnvelopeHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "sync host bridge path should not be used for counting delayed async ops".to_string(),
        ))
    }

    fn call_async(
        &self,
        _request: HostCallRequest,
        _cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        *self
            .calls
            .lock()
            .expect("counting delayed host lock should not be poisoned") += 1;
        let delay = self.delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(serde_json::json!({
                "status": "ok",
                "value": Value::Null,
            }))
        })
    }
}

pub(super) struct AsyncOnlyHost;

impl HostBridge for AsyncOnlyHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
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

pub(super) struct AsyncEchoHost;

impl HostBridge for AsyncEchoHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
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
pub(super) struct DelayedAsyncEchoHost {
    delay: std::time::Duration,
}

impl DelayedAsyncEchoHost {
    pub(super) fn new(delay: std::time::Duration) -> Self {
        Self { delay }
    }
}

impl HostBridge for DelayedAsyncEchoHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
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
pub(super) struct DeferredAsyncHost {
    release: Arc<Notify>,
    calls: Mutex<Vec<HostCallRequest>>,
}

impl DeferredAsyncHost {
    pub(super) fn release(&self) {
        self.release.notify_waiters();
    }
}

impl HostBridge for DeferredAsyncHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
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
pub(super) struct PaginateHost {
    pub(super) sync_calls: Mutex<Vec<HostCallRequest>>,
    pub(super) async_calls: Mutex<Vec<HostCallRequest>>,
}

impl HostBridge for PaginateHost {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        self.sync_calls
            .lock()
            .expect("paginate host sync lock should not be poisoned")
            .push(request.clone());
        let value = match request.operation {
            HostCallOperation::QueryBuilderStart => Value::String("builder-1".to_string()),
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
pub(super) struct PaginateContinuationHost;

impl HostBridge for PaginateContinuationHost {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        let value = match request.operation {
            HostCallOperation::QueryBuilderStart => Value::String("builder-1".to_string()),
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
pub(super) struct SyncOnlyHost {
    pub(super) calls: Mutex<Vec<HostCallRequest>>,
}

impl HostBridge for SyncOnlyHost {
    fn call(&self, request: HostCallRequest) -> Result<Value> {
        self.calls
            .lock()
            .expect("sync-only host lock should not be poisoned")
            .push(request.clone());
        let value = match request.operation {
            HostCallOperation::QueryBuilderStart => Value::String("builder-1".to_string()),
            // Resolve every nested callee to the default lane so a same-lane
            // nested ctx.run* in these default-isolate tests takes local dispatch.
            HostCallOperation::CtxResolveCalleeLane => Value::String("default".to_string()),
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
            Err(NimbusRuntimeError::Contract(
                "async host bridge path should not be used for sync ops".to_string(),
            ))
        })
    }
}

pub(super) async fn invoke_on_single_worker(
    executor: &RuntimeExecutor,
    runtime: NimbusRuntime,
    bundle: &RuntimeBundle,
    request: InvocationRequest,
) -> Result<Value> {
    executor
        .invoke_on_worker(
            runtime,
            bundle.clone(),
            request.clone(),
            RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a"),
            None,
        )
        .await
}

pub(super) fn test_invocation_auth(token_identifier: &str) -> Value {
    json!({
        "identity": {
            "tokenIdentifier": token_identifier,
            "subject": token_identifier,
            "issuer": "https://issuer.example.com",
        },
        "throw_on_missing_identity": false,
    })
}
