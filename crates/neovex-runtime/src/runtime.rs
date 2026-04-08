use std::sync::Arc;
use std::sync::OnceLock;

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

    mod basic_invocation;
    mod bundle_integrity;
    mod cooperative;
    mod host_bridge;
    mod locker;
    mod pool_reuse;
    mod snapshot_lifecycle;
    mod timeout_cancellation;
    mod warm_pool;
}
