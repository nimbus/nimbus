use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{Map, Value};
use tokio::sync::Notify;

use super::*;
use crate::host::{HostBridgeFuture, HostCallCancellation, HostCallOperation, HostCallRequest};

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

pub(super) fn stress_env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

pub(super) fn stress_env_duration_ms(
    name: &str,
    default: std::time::Duration,
) -> std::time::Duration {
    let default_ms = default.as_millis().min(u64::MAX as u128) as u64;
    std::time::Duration::from_millis(
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(default_ms),
    )
}

pub(super) fn ci_sensitive_duration(
    local: std::time::Duration,
    ci: std::time::Duration,
) -> std::time::Duration {
    if std::env::var_os("CI").is_some() {
        ci
    } else {
        local
    }
}

#[derive(Default)]
pub(super) struct RecordingHost {
    pub(super) calls: Mutex<Vec<HostCallRequest>>,
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

pub(super) struct SlowEnvelopeHost {
    pub(super) delay: std::time::Duration,
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

pub(super) struct AsyncOnlyHost;

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

pub(super) struct AsyncEchoHost;

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
pub(super) struct PaginateContinuationHost;

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

pub(super) async fn invoke_on_single_worker(
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

pub(super) fn test_invocation_auth(token_identifier: &str) -> InvocationAuth {
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
