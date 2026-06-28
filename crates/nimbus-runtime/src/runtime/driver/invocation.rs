use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::backends::v8::embedder::JsRuntime;
use crate::backends::v8::{ReusableV8Runtime, V8RuntimeConstructionMode, V8WorkerRuntimePool};
use crate::error::Result;
use crate::execution_plan::RuntimeExecutionPlan;
use crate::executor::{SharedInvocationPermit, WorkerActivitySignal};
use crate::host::HostCallCancellation;
use crate::limits::{RuntimePolicy, RuntimePoolKind};
use crate::watchdog::WatchdogTimer;

use super::super::bootstrap::{RuntimeCancellationState, take_runtime_wait_until_pending};
use super::super::helpers::{classify_runtime_error, classify_wait_until_drain_error};
use super::super::realm_lease::RuntimeRealmLeaseCondemnationReason;
use super::super::realm_lifecycle::destroy_fresh_realm;
use super::super::{
    FreshRealmInvocationResponse, FreshRealmInvocationTrace, NimbusRuntime,
    RuntimeInvocationExecution, RuntimeInvocationTimeoutController,
};

pub(crate) struct RuntimeInvocationDriver {
    pub(crate) runtime: JsRuntime,
    pub(crate) warm_reuse_count: usize,
    pub(crate) construction_mode: V8RuntimeConstructionMode,
    pub(crate) lifecycle: crate::backends::v8::RuntimeReuseLifecycle,
    pub(crate) realm_lease_controller: crate::runtime::realm_lease::RuntimeRealmLeaseController,
    policy: Arc<RuntimePolicy>,
    permit: SharedInvocationPermit,
    watchdog: WatchdogTimer,
    timeout_controller: Option<RuntimeInvocationTimeoutController>,
    system_timeout_watchdog: Option<crate::watchdog::WatchdogRegistration>,
    system_timeout_callback: Option<Arc<dyn Fn() + Send + Sync>>,
    external_cancellation_watchdog: Option<crate::watchdog::WatchdogRegistration>,
    timeout_triggered: Arc<AtomicBool>,
    system_timeout_triggered: Arc<AtomicBool>,
    heap_limit_triggered: Arc<AtomicBool>,
    pub(crate) external_cancellation_triggered: Arc<AtomicBool>,
    record_replacement_on_error: bool,
}

pub(crate) struct RuntimeInvocationDriverPrepare<'a> {
    pub(crate) runtime: ReusableV8Runtime,
    pub(crate) watchdog: WatchdogTimer,
    pub(crate) external_cancellation: Option<HostCallCancellation>,
    pub(crate) permit: SharedInvocationPermit,
    pub(crate) context: &'a crate::context::RuntimeInvocationContext,
    pub(crate) execution_plan: Option<&'a RuntimeExecutionPlan>,
    pub(crate) record_replacement_on_error: bool,
    pub(crate) activity_signal: Option<Arc<WorkerActivitySignal>>,
}

impl RuntimeInvocationDriver {
    pub(crate) async fn begin_wait_until_phase(&mut self) -> Result<()> {
        let limits = self.policy.limits();
        if let Some(timeout_controller) = &self.timeout_controller {
            timeout_controller.reset(limits.execution_timeout).await?;
        }
        if let Some(system_timeout_watchdog) = self.system_timeout_watchdog.take() {
            system_timeout_watchdog.disarm().await;
        }
        self.system_timeout_watchdog = match (&self.system_timeout_callback, limits.system_timeout)
        {
            (_, timeout) if timeout.is_zero() => None,
            (Some(callback), timeout) => Some(self.watchdog.register_timeout(
                std::time::Instant::now() + timeout,
                {
                    let callback = callback.clone();
                    move || callback()
                },
            )?),
            (None, _) => None,
        };
        Ok(())
    }

    pub(crate) fn wait_until_phase_timeout_error(
        &self,
    ) -> Option<crate::error::NimbusRuntimeError> {
        let limits = self.policy.limits();
        if self.system_timeout_triggered.load(Ordering::SeqCst) {
            return Some(crate::error::NimbusRuntimeError::SystemTimeout(
                limits.system_timeout,
            ));
        }
        if self.timeout_triggered.load(Ordering::SeqCst) {
            return Some(crate::error::NimbusRuntimeError::ExecutionTimeout(
                limits.execution_timeout,
            ));
        }
        None
    }

    pub(crate) fn classify_wait_until_drain_result(&self, result: Result<()>) -> Result<()> {
        match result {
            Ok(()) => self.wait_until_phase_timeout_error().map_or(Ok(()), Err),
            Err(error) => Err(classify_wait_until_drain_error(
                error,
                &self.timeout_triggered,
                &self.system_timeout_triggered,
                self.policy.limits(),
            )),
        }
    }

    pub(crate) fn record_fresh_realm_promise_resolve(&self, duration: Duration) {
        self.policy
            .metrics()
            .record_fresh_realm_promise_resolve(duration);
    }

    pub(crate) fn record_fresh_realm_deserialization(&self, duration: Duration) {
        self.policy
            .metrics()
            .record_fresh_realm_deserialization(duration);
    }

    pub(crate) fn record_fresh_realm_destroy(&self, duration: Duration) {
        self.policy.metrics().record_fresh_realm_destroy(duration);
    }

    pub(crate) fn realm_lease_condemnation_reason(&self) -> RuntimeRealmLeaseCondemnationReason {
        realm_lease_condemnation_reason_from_flags(
            &self.timeout_triggered,
            &self.system_timeout_triggered,
            &self.heap_limit_triggered,
            &self.external_cancellation_triggered,
        )
    }

    pub(crate) fn realm_lease_condemnation_reason_classifier(
        &self,
    ) -> impl Fn() -> RuntimeRealmLeaseCondemnationReason + 'static {
        let timeout_triggered = self.timeout_triggered.clone();
        let system_timeout_triggered = self.system_timeout_triggered.clone();
        let heap_limit_triggered = self.heap_limit_triggered.clone();
        let external_cancellation_triggered = self.external_cancellation_triggered.clone();
        move || {
            realm_lease_condemnation_reason_from_flags(
                &timeout_triggered,
                &system_timeout_triggered,
                &heap_limit_triggered,
                &external_cancellation_triggered,
            )
        }
    }

    pub(crate) async fn finalize_with_runtime(
        mut self,
        result: Result<serde_json::Value>,
    ) -> (Result<serde_json::Value>, Option<ReusableV8Runtime>) {
        if let Some(timeout_controller) = self.timeout_controller {
            timeout_controller.disarm().await;
        }
        if let Some(system_timeout_watchdog) = self.system_timeout_watchdog {
            system_timeout_watchdog.disarm().await;
        }
        self.permit.clear_timeout_controller();
        if let Some(external_cancellation_watchdog) = self.external_cancellation_watchdog {
            external_cancellation_watchdog.disarm().await;
        }

        let replacement_required = self.timeout_triggered.load(Ordering::SeqCst)
            || self.system_timeout_triggered.load(Ordering::SeqCst)
            || self.heap_limit_triggered.load(Ordering::SeqCst)
            || self.external_cancellation_triggered.load(Ordering::SeqCst);

        let result = result.map_err(|error| {
            classify_runtime_error(
                error,
                &self.timeout_triggered,
                &self.system_timeout_triggered,
                &self.heap_limit_triggered,
                &self.external_cancellation_triggered,
                self.policy.limits(),
            )
        });
        let near_heap_limit_triggered = self.heap_limit_triggered.load(Ordering::SeqCst);
        if result.is_err() && replacement_required && self.record_replacement_on_error {
            self.policy.metrics().record_runtime_pool_replacement();
            self.policy
                .metrics()
                .record_profile_runtime_pool_replacement(self.policy.runtime_profile());
            if near_heap_limit_triggered
                && matches!(
                    self.policy.limits().runtime_pool_kind,
                    RuntimePoolKind::WarmPool
                )
            {
                self.policy.metrics().record_warm_pool_retirement();
            }
        }
        let runtime = if result.is_ok() && !replacement_required {
            Some(ReusableV8Runtime {
                runtime: self.runtime,
                warm_reuse_count: self.warm_reuse_count,
                construction_mode: self.construction_mode,
                lifecycle: self.lifecycle,
                realm_lease_controller: self.realm_lease_controller,
            })
        } else {
            self.lifecycle.mark_condemned();
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

impl NimbusRuntime {
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
            execution_plan,
            external_cancellation,
            response_ready_tx,
            permit,
        } = invocation;
        let mut response_ready_tx = response_ready_tx;
        let integrity_started_at = Instant::now();
        let integrity_result = bundle.verify_integrity();
        self.policy
            .metrics()
            .record_bundle_integrity_verify(integrity_started_at.elapsed());
        integrity_result?;
        self.policy
            .validate_bundle_content_kind(bundle.content_kind())?;
        let mut v8_runtime_pool = v8_runtime_pool;
        let runtime = match v8_runtime_pool.as_deref_mut() {
            Some(pool) => pool.take_runtime_for_invocation(self, &bundle, Some(&context))?,
            None => {
                // Pool-less direct path: route through the SAME mode->construction mapping the
                // warm pool uses (`NimbusRuntime::create_runtime_for_mode`) so non-Node profiles
                // are built UNSNAPSHOTTED here too. Hardcoding StartupSnapshot here was the second
                // cross-profile cage-crash hole (efd891a8a): a snapshotted WebStandard deserializes
                // against the NodeFull anchor's RO heap and aborts. `use_locker = false` matches
                // the prior create_runtime_from_snapshot behavior on this path.
                let mode = V8RuntimeConstructionMode::for_compatibility_target(
                    self.policy.limits().compatibility_target,
                );
                let runtime = self.create_runtime_for_mode(&bundle, false, mode)?;
                ReusableV8Runtime::fresh(runtime, mode)
            }
        };
        let mut driver =
            self.prepare_runtime_invocation_driver(RuntimeInvocationDriverPrepare {
                runtime,
                watchdog: watchdog.clone(),
                external_cancellation: external_cancellation.clone(),
                permit: permit.clone(),
                context: &context,
                execution_plan: Some(&execution_plan),
                record_replacement_on_error: v8_runtime_pool.is_some(),
                activity_signal: None,
            })?;

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
            let context_recycling = matches!(
                self.policy.limits().runtime_pool_kind,
                crate::limits::RuntimePoolKind::WarmContextRecycle,
            );
            let is_warm_hit = matches!(
                self.policy.limits().runtime_pool_kind,
                crate::limits::RuntimePoolKind::WarmPool,
            ) && driver.warm_reuse_count > 0;
            let invoke = async {
                if context_recycling {
                    let lease_failure_reason = driver.realm_lease_condemnation_reason_classifier();
                    let trace = FreshRealmInvocationTrace {
                        bundle: &bundle,
                        request: &request,
                        construction_mode: driver.construction_mode,
                        context: Some(&context),
                    };
                    let (value, realm, mut realm_lease) = self
                        .start_fresh_realm_bundle_invocation_with_lease_and_reason_trace(
                            &driver.realm_lease_controller,
                            &mut driver.runtime,
                            trace,
                            lease_failure_reason,
                        )
                        .await?;
                    let response = self
                        .resolve_fresh_realm_invocation_response_with_lease_and_trace(
                            &mut driver.runtime,
                            FreshRealmInvocationResponse {
                                realm: &realm,
                                value,
                                trace,
                            },
                            &mut realm_lease,
                        )
                        .await;
                    let result = match response {
                        Ok(response) => {
                            if let Some(response_ready_tx) = response_ready_tx.take() {
                                let _ = response_ready_tx.send(response.clone());
                            }
                            if take_runtime_wait_until_pending(&mut driver.runtime) {
                                driver.begin_wait_until_phase().await?;
                                let drain = self
                                    .drain_wait_until_with_trace(
                                        &mut driver.runtime,
                                        Some(&realm),
                                        Some(&bundle),
                                        &request,
                                        driver.construction_mode,
                                        Some(&context),
                                    )
                                    .await;
                                driver
                                    .classify_wait_until_drain_result(drain)
                                    .map(|()| response)
                            } else {
                                Ok(response)
                            }
                        }
                        Err(error) => Err(error),
                    };
                    let destroy_started_at = std::time::Instant::now();
                    destroy_fresh_realm(&mut driver.runtime, realm);
                    driver.record_fresh_realm_destroy(destroy_started_at.elapsed());
                    return match result {
                        Ok(response) => {
                            self.return_clean_fresh_realm_lease(
                                &mut driver.runtime,
                                &mut realm_lease,
                            )?;
                            Ok(response)
                        }
                        Err(error) => {
                            self.condemn_fresh_realm_lease_with_reason(
                                &mut realm_lease,
                                driver.realm_lease_condemnation_reason(),
                            );
                            Err(error)
                        }
                    };
                }
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
                let response = self
                    .invoke_loaded_bundle_with_trace(
                        &mut driver.runtime,
                        &request,
                        Some(&bundle),
                        driver.construction_mode,
                        Some(&context),
                    )
                    .await?;
                if let Some(response_ready_tx) = response_ready_tx.take() {
                    let _ = response_ready_tx.send(response.clone());
                }
                if take_runtime_wait_until_pending(&mut driver.runtime) {
                    driver.begin_wait_until_phase().await?;
                    let drain = self
                        .drain_wait_until_with_trace(
                            &mut driver.runtime,
                            None,
                            Some(&bundle),
                            &request,
                            driver.construction_mode,
                            Some(&context),
                        )
                        .await;
                    driver
                        .classify_wait_until_drain_result(drain)
                        .map(|()| response)
                } else {
                    Ok(response)
                }
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
            match self.policy.limits().runtime_pool_kind {
                crate::limits::RuntimePoolKind::WarmPool
                | crate::limits::RuntimePoolKind::WarmContextRecycle => {
                    runtime.warm_reuse_count = runtime.warm_reuse_count.saturating_add(1);
                }
                crate::limits::RuntimePoolKind::StartupSnapshotCache
                | crate::limits::RuntimePoolKind::BunJscTrustedRetained
                | crate::limits::RuntimePoolKind::BunJscFreshDiscard
                | crate::limits::RuntimePoolKind::PrecompiledModuleCache
                | crate::limits::RuntimePoolKind::RetainedStorePool => {}
            }
            pool.return_runtime_for_invocation(self, &bundle, Some(&context), runtime);
        }
        result
    }

    pub(crate) fn prepare_runtime_invocation_driver(
        &self,
        prepare: RuntimeInvocationDriverPrepare<'_>,
    ) -> Result<RuntimeInvocationDriver> {
        let RuntimeInvocationDriverPrepare {
            runtime,
            watchdog,
            external_cancellation,
            permit,
            context,
            execution_plan,
            record_replacement_on_error,
            activity_signal,
        } = prepare;
        let ReusableV8Runtime {
            mut runtime,
            warm_reuse_count,
            construction_mode,
            lifecycle,
            realm_lease_controller,
        } = runtime;
        let timeout = self.policy.limits().execution_timeout;
        let system_timeout = self.policy.limits().system_timeout;
        let timeout_triggered = Arc::new(AtomicBool::new(false));
        let system_timeout_triggered = Arc::new(AtomicBool::new(false));
        let heap_limit_triggered = Arc::new(AtomicBool::new(false));
        let external_cancellation_triggered = Arc::new(AtomicBool::new(false));
        super::super::bootstrap::bind_runtime_host_bridge(&mut runtime, self.host.clone());
        super::super::bootstrap::reset_runtime_invocation_state(
            &mut runtime,
            permit.clone(),
            Some(context),
            execution_plan,
        );
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
                let activity_signal = activity_signal.clone();
                watchdog.register_cancellation(external, move || {
                    external_cancellation_triggered.store(true, Ordering::SeqCst);
                    cancellation_signal.cancel();
                    let _ = isolate_handle.terminate_execution();
                    if let Some(activity_signal) = activity_signal {
                        activity_signal.notify();
                    }
                })
            })
            .transpose()?;

        {
            let heap_limit_triggered = heap_limit_triggered.clone();
            let cancellation_signal = cancellation_signal.clone();
            let isolate_handle = runtime.v8_isolate().thread_safe_handle();
            let activity_signal = activity_signal.clone();
            runtime.add_near_heap_limit_callback(move |current_limit, _initial_limit| {
                heap_limit_triggered.store(true, Ordering::SeqCst);
                cancellation_signal.cancel();
                let _ = isolate_handle.terminate_execution();
                if let Some(activity_signal) = &activity_signal {
                    activity_signal.notify();
                }
                current_limit.saturating_mul(2)
            });
        }

        let timeout_controller = if timeout.is_zero() {
            None
        } else {
            let isolate_handle = runtime.v8_isolate().thread_safe_handle();
            let timeout_triggered = timeout_triggered.clone();
            let cancellation_signal = cancellation_signal.clone();
            let activity_signal = activity_signal.clone();
            let callback: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                timeout_triggered.store(true, Ordering::SeqCst);
                cancellation_signal.cancel();
                let _ = isolate_handle.terminate_execution();
                if let Some(activity_signal) = &activity_signal {
                    activity_signal.notify();
                }
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

        let system_timeout_callback: Option<Arc<dyn Fn() + Send + Sync>> =
            if system_timeout.is_zero() {
                None
            } else {
                let isolate_handle = runtime.v8_isolate().thread_safe_handle();
                let system_timeout_triggered = system_timeout_triggered.clone();
                let cancellation_signal = cancellation_signal.clone();
                let activity_signal = activity_signal.clone();
                Some(Arc::new(move || {
                    system_timeout_triggered.store(true, Ordering::SeqCst);
                    cancellation_signal.cancel();
                    let _ = isolate_handle.terminate_execution();
                    if let Some(activity_signal) = &activity_signal {
                        activity_signal.notify();
                    }
                }))
            };

        let system_timeout_watchdog = if system_timeout.is_zero() {
            None
        } else {
            let callback = system_timeout_callback
                .as_ref()
                .expect("nonzero system timeout should install a callback")
                .clone();
            Some(watchdog.register_timeout(
                std::time::Instant::now() + system_timeout,
                move || {
                    callback();
                },
            )?)
        };

        Ok(RuntimeInvocationDriver {
            runtime,
            warm_reuse_count,
            construction_mode,
            lifecycle,
            realm_lease_controller,
            policy: self.policy.clone(),
            permit,
            watchdog,
            timeout_controller,
            system_timeout_watchdog,
            system_timeout_callback,
            external_cancellation_watchdog,
            timeout_triggered,
            system_timeout_triggered,
            heap_limit_triggered,
            external_cancellation_triggered,
            record_replacement_on_error,
        })
    }
}

fn realm_lease_condemnation_reason_from_flags(
    timeout_triggered: &AtomicBool,
    system_timeout_triggered: &AtomicBool,
    heap_limit_triggered: &AtomicBool,
    external_cancellation_triggered: &AtomicBool,
) -> RuntimeRealmLeaseCondemnationReason {
    if timeout_triggered.load(Ordering::SeqCst) || system_timeout_triggered.load(Ordering::SeqCst) {
        return RuntimeRealmLeaseCondemnationReason::TimedOut;
    }
    if heap_limit_triggered.load(Ordering::SeqCst)
        || external_cancellation_triggered.load(Ordering::SeqCst)
    {
        return RuntimeRealmLeaseCondemnationReason::ExternalPressure;
    }
    RuntimeRealmLeaseCondemnationReason::Dirty
}
