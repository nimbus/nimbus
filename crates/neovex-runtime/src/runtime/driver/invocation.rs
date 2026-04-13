use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::backends::v8::embedder::JsRuntime;
use crate::backends::v8::{ReusableV8Runtime, V8RuntimeConstructionMode, V8WorkerRuntimePool};
use crate::error::Result;
use crate::executor::SharedInvocationPermit;
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::watchdog::WatchdogTimer;

use super::super::bootstrap::RuntimeCancellationState;
use super::super::helpers::classify_runtime_error;
use super::super::{NeovexRuntime, RuntimeInvocationExecution, RuntimeInvocationTimeoutController};

pub(crate) struct RuntimeInvocationDriver {
    pub(crate) runtime: JsRuntime,
    pub(crate) warm_reuse_count: usize,
    pub(crate) construction_mode: V8RuntimeConstructionMode,
    policy: Arc<RuntimePolicy>,
    permit: SharedInvocationPermit,
    timeout_controller: Option<RuntimeInvocationTimeoutController>,
    external_cancellation_watchdog: Option<crate::watchdog::WatchdogRegistration>,
    timeout_triggered: Arc<AtomicBool>,
    heap_limit_triggered: Arc<AtomicBool>,
    pub(crate) external_cancellation_triggered: Arc<AtomicBool>,
    record_replacement_on_error: bool,
}

impl RuntimeInvocationDriver {
    pub(crate) async fn finalize_with_runtime(
        self,
        result: Result<serde_json::Value>,
    ) -> (Result<serde_json::Value>, Option<ReusableV8Runtime>) {
        if let Some(timeout_controller) = self.timeout_controller {
            timeout_controller.disarm().await;
        }
        self.permit.clear_timeout_controller();
        if let Some(external_cancellation_watchdog) = self.external_cancellation_watchdog {
            external_cancellation_watchdog.disarm().await;
        }

        let replacement_required = self.timeout_triggered.load(Ordering::SeqCst)
            || self.heap_limit_triggered.load(Ordering::SeqCst)
            || self.external_cancellation_triggered.load(Ordering::SeqCst);

        let result = result.map_err(|error| {
            classify_runtime_error(
                error,
                &self.timeout_triggered,
                &self.heap_limit_triggered,
                &self.external_cancellation_triggered,
                self.policy.limits(),
            )
        });
        if result.is_err() && replacement_required && self.record_replacement_on_error {
            self.policy.metrics().record_runtime_pool_replacement();
        }
        let runtime = if result.is_ok() && !replacement_required {
            Some(ReusableV8Runtime {
                runtime: self.runtime,
                warm_reuse_count: self.warm_reuse_count,
                construction_mode: self.construction_mode,
            })
        } else {
            None
        };
        (result, runtime)
    }

    pub(crate) async fn finalize(
        self,
        result: Result<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        self.finalize_with_runtime(result).await.0
    }
}

impl NeovexRuntime {
    pub(crate) async fn invoke_bundle_unmanaged(
        &self,
        v8_runtime_pool: Option<&mut V8WorkerRuntimePool>,
        invocation: RuntimeInvocationExecution,
    ) -> Result<serde_json::Value> {
        let RuntimeInvocationExecution {
            watchdog,
            bundle,
            request,
            context,
            external_cancellation,
            permit,
        } = invocation;
        bundle.verify_integrity()?;
        let mut v8_runtime_pool = v8_runtime_pool;
        let runtime = match v8_runtime_pool.as_deref_mut() {
            Some(pool) => pool.take_runtime_for_invocation(self, &bundle, Some(&context))?,
            None => {
                let snapshot = self.bootstrap_snapshot()?;
                ReusableV8Runtime::fresh(
                    self.create_runtime_from_snapshot(&bundle, snapshot)?,
                    V8RuntimeConstructionMode::StartupSnapshot,
                )
            }
        };
        let mut driver = self.prepare_runtime_invocation_driver(
            runtime,
            watchdog.clone(),
            external_cancellation.clone(),
            permit.clone(),
            v8_runtime_pool.is_some(),
        )?;

        let result = {
            let isolate_handle = driver.runtime.v8_isolate().thread_safe_handle();
            let cancellation_signal = {
                let op_state = driver.runtime.op_state();
                op_state
                    .borrow()
                    .borrow::<RuntimeCancellationState>()
                    .signal
                    .clone()
            };
            let external_cancellation_triggered = driver.external_cancellation_triggered.clone();
            let is_warm_hit = matches!(
                self.policy.limits().runtime_pool_kind,
                crate::limits::RuntimePoolKind::WarmPool,
            ) && driver.warm_reuse_count > 0;
            let invoke = async {
                if !is_warm_hit {
                    self.load_bundle_with_trace(
                        &mut driver.runtime,
                        &bundle,
                        driver.construction_mode,
                        Some(&context),
                        Some(&request),
                    )
                    .await?;
                }
                self.invoke_loaded_bundle_with_trace(
                    &mut driver.runtime,
                    &request,
                    Some(&bundle),
                    driver.construction_mode,
                    Some(&context),
                )
                .await
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

        let (result, reusable_runtime) = driver.finalize_with_runtime(result).await;
        if let (Some(pool), Some(mut runtime)) = (v8_runtime_pool, reusable_runtime) {
            if matches!(
                self.policy.limits().runtime_pool_kind,
                crate::limits::RuntimePoolKind::WarmPool,
            ) {
                // Clear event loop state while preserving evaluated modules.
                // If the runtime is not quiescent, discard instead of pooling.
                if runtime.runtime.reset_request_state().is_err() {
                    self.policy.metrics().record_warm_pool_discard_unquiesced();
                    return result;
                }
                runtime.warm_reuse_count = runtime.warm_reuse_count.saturating_add(1);
            }
            pool.return_runtime_for_invocation(self, &bundle, Some(&context), runtime);
        }
        result
    }

    pub(crate) fn prepare_runtime_invocation_driver(
        &self,
        runtime: ReusableV8Runtime,
        watchdog: WatchdogTimer,
        external_cancellation: Option<HostCallCancellation>,
        permit: SharedInvocationPermit,
        record_replacement_on_error: bool,
    ) -> Result<RuntimeInvocationDriver> {
        let ReusableV8Runtime {
            mut runtime,
            warm_reuse_count,
            construction_mode,
        } = runtime;
        let timeout = self.policy.limits().execution_timeout;
        let timeout_triggered = Arc::new(AtomicBool::new(false));
        let heap_limit_triggered = Arc::new(AtomicBool::new(false));
        let external_cancellation_triggered = Arc::new(AtomicBool::new(false));
        super::super::bootstrap::bind_runtime_host_bridge(&mut runtime, self.host.clone());
        super::super::bootstrap::reset_runtime_invocation_state(&mut runtime, permit.clone());
        super::super::bootstrap::reset_bootstrap_invocation_state(&mut runtime)?;
        let cancellation_signal = {
            let op_state = runtime.op_state();
            op_state
                .borrow()
                .borrow::<RuntimeCancellationState>()
                .signal
                .clone()
        };
        let external_cancellation_watchdog = external_cancellation
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
                watchdog, timeout, callback,
            )?)
        };
        if let Some(timeout_controller) = timeout_controller.clone() {
            permit.set_timeout_controller(timeout_controller);
        }

        Ok(RuntimeInvocationDriver {
            runtime,
            warm_reuse_count,
            construction_mode,
            policy: self.policy.clone(),
            permit,
            timeout_controller,
            external_cancellation_watchdog,
            timeout_triggered,
            heap_limit_triggered,
            external_cancellation_triggered,
            record_replacement_on_error,
        })
    }
}
