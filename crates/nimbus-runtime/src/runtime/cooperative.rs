use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Instant;

use serde_json::Value;
use tokio::sync::oneshot;

use crate::RuntimeInvocationContext;
use crate::backends::v8::embedder::{JsError, JsRealm, PollEventLoopOptions, v8};
use crate::backends::v8::{ReusableV8Runtime, V8WorkerRuntimePool};
use crate::error::Result;
use crate::execution_plan::RuntimeExecutionPlan;
use crate::executor::{SharedInvocationPermit, WorkerActivitySignal};
use crate::host::HostCallCancellation;
use crate::watchdog::WatchdogTimer;

use super::bootstrap::{clear_runtime_wait_until_pending, take_runtime_wait_until_pending};
use super::helpers::{deserialize_json_value, ensure_wait_until_drain_succeeded, runtime_js_error};
use super::realm_lifecycle::destroy_fresh_realm;
use super::{
    FreshRealmInvocationTrace, InvocationRequest, NimbusRuntime, RuntimeBundle,
    RuntimeInvocationDriver, RuntimeInvocationDriverPrepare,
};

pub(crate) struct RuntimeInvocationExecution {
    pub(crate) watchdog: WatchdogTimer,
    pub(crate) bundle: RuntimeBundle,
    pub(crate) request: InvocationRequest,
    pub(crate) context: RuntimeInvocationContext,
    pub(crate) execution_plan: RuntimeExecutionPlan,
    pub(crate) external_cancellation: Option<HostCallCancellation>,
    pub(crate) response_ready_tx: Option<oneshot::Sender<Value>>,
    pub(crate) permit: SharedInvocationPermit,
}

pub(crate) struct CooperativeRuntimeSlotStart {
    pub(crate) invocation: RuntimeInvocationExecution,
    pub(crate) activity_signal: Arc<WorkerActivitySignal>,
}

type CooperativePromiseFuture =
    Pin<Box<dyn Future<Output = std::result::Result<v8::Global<v8::Value>, Box<JsError>>>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CooperativeRuntimeSlotPoll {
    Runnable,
    Parked,
    ResponseReady,
    Completed,
}

struct CooperativeRuntimeWakeFlag {
    woken: AtomicBool,
    activity_signal: Arc<WorkerActivitySignal>,
}

impl CooperativeRuntimeWakeFlag {
    fn new(activity_signal: Arc<WorkerActivitySignal>) -> Self {
        Self {
            woken: AtomicBool::new(false),
            activity_signal,
        }
    }

    fn take_woken(&self) -> bool {
        self.woken.swap(false, Ordering::SeqCst)
    }

    fn is_woken(&self) -> bool {
        self.woken.load(Ordering::SeqCst)
    }
}

impl Wake for CooperativeRuntimeWakeFlag {
    fn wake(self: Arc<Self>) {
        self.woken.store(true, Ordering::SeqCst);
        self.activity_signal.notify();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.store(true, Ordering::SeqCst);
        self.activity_signal.notify();
    }
}

pub(crate) struct CooperativeLockerRuntimeSlot {
    driver: Option<RuntimeInvocationDriver>,
    resolve: Option<CooperativePromiseFuture>,
    resolve_started_at: Option<Instant>,
    wait_until: Option<(CooperativePromiseFuture, Result<Value>)>,
    response_ready: Option<(RuntimeInvocationDriver, Result<Value>)>,
    response_ready_tx: Option<oneshot::Sender<Value>>,
    fresh_realm: Option<JsRealm>,
    wake_flag: Arc<CooperativeRuntimeWakeFlag>,
    completed: Option<(RuntimeInvocationDriver, Result<Value>)>,
}

impl CooperativeLockerRuntimeSlot {
    fn destroy_fresh_realm(&mut self, driver: &mut RuntimeInvocationDriver) {
        if let Some(realm) = self.fresh_realm.take() {
            let destroy_started_at = Instant::now();
            destroy_fresh_realm(&mut driver.runtime, realm);
            driver.record_fresh_realm_destroy(destroy_started_at.elapsed());
        }
    }

    fn poll_once_now(&mut self) -> Result<CooperativeRuntimeSlotPoll> {
        let mut driver = self
            .driver
            .take()
            .ok_or_else(|| runtime_js_error("cooperative runtime slot polled after completion"))?;
        let mut locked = driver.runtime.acquire_v8_lock();
        let waker = Waker::from(self.wake_flag.clone());
        let mut cx = Context::from_waker(&waker);

        if let Some((wait_until, _response)) = self.wait_until.as_mut() {
            if let Poll::Ready(result) = wait_until.as_mut().poll(&mut cx) {
                let response = self
                    .wait_until
                    .take()
                    .expect("waitUntil phase should be present")
                    .1;
                let drain_result = result
                    .map_err(runtime_js_error)
                    .and_then(|value| ensure_wait_until_drain_succeeded(&mut locked, value));
                drop(locked);
                clear_runtime_wait_until_pending(&mut driver.runtime);
                let result = match drain_result {
                    Ok(_) => driver
                        .wait_until_phase_timeout_error()
                        .map_or(response, Err),
                    Err(error) => Err(driver.classify_wait_until_drain_error(error)),
                };
                self.destroy_fresh_realm(&mut driver);
                self.completed = Some((driver, result));
                return Ok(CooperativeRuntimeSlotPoll::Completed);
            }
        } else if let Some(resolve) = self.resolve.as_mut() {
            if let Poll::Ready(result) = resolve.as_mut().poll(&mut cx) {
                let promise_resolve_elapsed = self
                    .resolve_started_at
                    .take()
                    .expect("response phase should have a start time")
                    .elapsed();
                self.resolve.take();
                let mut deserialization_elapsed = None;
                let result: Result<Value> = result.map_err(runtime_js_error).and_then(|value| {
                    let deserialize_started_at = Instant::now();
                    let result = deserialize_json_value(&mut locked, value);
                    deserialization_elapsed = Some(deserialize_started_at.elapsed());
                    result
                });
                drop(locked);
                driver.record_fresh_realm_promise_resolve(promise_resolve_elapsed);
                if let Some(elapsed) = deserialization_elapsed {
                    driver.record_fresh_realm_deserialization(elapsed);
                }
                if result.is_ok() {
                    if let (Some(response), Some(response_ready_tx)) =
                        (result.as_ref().ok(), self.response_ready_tx.take())
                    {
                        let _ = response_ready_tx.send(response.clone());
                    }
                    self.response_ready = Some((driver, result));
                    return Ok(CooperativeRuntimeSlotPoll::ResponseReady);
                }
                self.destroy_fresh_realm(&mut driver);
                self.completed = Some((driver, result));
                return Ok(CooperativeRuntimeSlotPoll::Completed);
            }
        } else {
            drop(locked);
            self.driver = Some(driver);
            return Err(runtime_js_error(
                "cooperative runtime slot has no active phase",
            ));
        }

        let event_loop_poll = if let Some(realm) = self.fresh_realm.as_ref() {
            locked.poll_event_loop_in_realm(realm, &mut cx, PollEventLoopOptions::default())
        } else {
            locked.poll_event_loop(&mut cx, PollEventLoopOptions::default())
        };
        match event_loop_poll {
            Poll::Ready(Ok(())) => {
                if let Some((wait_until, _response)) = self.wait_until.as_mut() {
                    let result = match wait_until.as_mut().poll(&mut cx) {
                        Poll::Ready(result) => {
                            let response = self
                                .wait_until
                                .take()
                                .expect("waitUntil phase should be present")
                                .1;
                            let drain_result = result.map_err(runtime_js_error).and_then(|value| {
                                ensure_wait_until_drain_succeeded(&mut locked, value)
                            });
                            drop(locked);
                            clear_runtime_wait_until_pending(&mut driver.runtime);
                            let result = match drain_result {
                                Ok(_) => driver
                                    .wait_until_phase_timeout_error()
                                    .map_or(response, Err),
                                Err(error) => Err(driver.classify_wait_until_drain_error(error)),
                            };
                            self.destroy_fresh_realm(&mut driver);
                            self.completed = Some((driver, result));
                            return Ok(CooperativeRuntimeSlotPoll::Completed);
                        }
                        Poll::Pending => Err(runtime_js_error(
                            "waitUntil drain is still pending but the event loop has already resolved",
                        )),
                    };
                    drop(locked);
                    let result =
                        result.map_err(|error| driver.classify_wait_until_drain_error(error));
                    self.destroy_fresh_realm(&mut driver);
                    self.completed = Some((driver, result));
                    return Ok(CooperativeRuntimeSlotPoll::Completed);
                }

                let mut promise_resolve_elapsed = None;
                let mut deserialization_elapsed = None;
                let result: Result<Value> = match self.resolve.as_mut() {
                    Some(resolve) => match resolve.as_mut().poll(&mut cx) {
                        Poll::Ready(result) => {
                            promise_resolve_elapsed = Some(
                                self.resolve_started_at
                                    .take()
                                    .expect("response phase should have a start time")
                                    .elapsed(),
                            );
                            self.resolve.take();
                            result.map_err(runtime_js_error).and_then(|value| {
                                let deserialize_started_at = Instant::now();
                                let result = deserialize_json_value(&mut locked, value);
                                deserialization_elapsed = Some(deserialize_started_at.elapsed());
                                result
                            })
                        }
                        Poll::Pending => Err(runtime_js_error(
                            "Promise resolution is still pending but the event loop has already resolved",
                        )),
                    },
                    None => Err(runtime_js_error(
                        "cooperative runtime slot has no active phase",
                    )),
                };
                drop(locked);
                if let Some(elapsed) = promise_resolve_elapsed {
                    driver.record_fresh_realm_promise_resolve(elapsed);
                }
                if let Some(elapsed) = deserialization_elapsed {
                    driver.record_fresh_realm_deserialization(elapsed);
                }
                if result.is_ok() {
                    if let (Some(response), Some(response_ready_tx)) =
                        (result.as_ref().ok(), self.response_ready_tx.take())
                    {
                        let _ = response_ready_tx.send(response.clone());
                    }
                    self.response_ready = Some((driver, result));
                    Ok(CooperativeRuntimeSlotPoll::ResponseReady)
                } else {
                    self.destroy_fresh_realm(&mut driver);
                    self.completed = Some((driver, result));
                    Ok(CooperativeRuntimeSlotPoll::Completed)
                }
            }
            Poll::Ready(Err(error)) => {
                drop(locked);
                self.destroy_fresh_realm(&mut driver);
                self.completed = Some((driver, Err(runtime_js_error(error))));
                Ok(CooperativeRuntimeSlotPoll::Completed)
            }
            Poll::Pending => {
                drop(locked);
                self.driver = Some(driver);
                if self.wake_flag.take_woken() {
                    Ok(CooperativeRuntimeSlotPoll::Runnable)
                } else {
                    Ok(CooperativeRuntimeSlotPoll::Parked)
                }
            }
        }
    }

    async fn start_wait_until_phase(&mut self) -> Result<()> {
        let Some((mut driver, response)) = self.response_ready.take() else {
            return Err(runtime_js_error(
                "cooperative runtime slot has no response-ready phase",
            ));
        };
        if !take_runtime_wait_until_pending(&mut driver.runtime) {
            self.completed = Some((driver, response));
            return Ok(());
        }
        driver.begin_wait_until_phase().await?;
        let value = match self.fresh_realm.as_ref() {
            Some(realm) => realm.execute_script(
                driver.runtime.v8_isolate(),
                "<nimbus-runtime:wait-until>",
                "globalThis.__nimbusDrainWaitUntil()",
            ),
            None => driver.runtime.execute_script(
                "<nimbus-runtime:wait-until>",
                "globalThis.__nimbusDrainWaitUntil()",
            ),
        }
        .map_err(runtime_js_error);
        let value = match value {
            Ok(value) => value,
            Err(error) => {
                clear_runtime_wait_until_pending(&mut driver.runtime);
                self.destroy_fresh_realm(&mut driver);
                self.completed = Some((driver, Err(error)));
                return Ok(());
            }
        };
        let wait_until: CooperativePromiseFuture = match self.fresh_realm.as_ref() {
            Some(realm) => Box::pin(driver.runtime.resolve_in_realm(realm, value)),
            None => Box::pin(driver.runtime.resolve(value)),
        };
        driver.runtime.release_v8_lock();
        self.wait_until = Some((wait_until, response));
        self.driver = Some(driver);
        Ok(())
    }

    pub(crate) async fn poll_once(&mut self) -> Result<CooperativeRuntimeSlotPoll> {
        let mut should_yield = false;
        loop {
            if should_yield {
                tokio::task::yield_now().await;
            }
            let poll = self.poll_once_now()?;
            match poll {
                CooperativeRuntimeSlotPoll::ResponseReady => {
                    self.start_wait_until_phase().await?;
                    if self.completed.is_some() {
                        return Ok(CooperativeRuntimeSlotPoll::Completed);
                    }
                    should_yield = false;
                }
                CooperativeRuntimeSlotPoll::Parked if !should_yield => {
                    should_yield = true;
                }
                other => return Ok(other),
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn take_result(mut self) -> Result<Value> {
        let (_, result) = self.completed.take().ok_or_else(|| {
            runtime_js_error("cooperative runtime slot result requested before completion")
        })?;
        result
    }

    pub(crate) async fn finish_with_runtime(self) -> (Result<Value>, Option<ReusableV8Runtime>) {
        let mut slot = self;
        let Some((driver, result)) = slot.completed.take() else {
            return (
                Err(runtime_js_error(
                    "cooperative runtime slot result requested before completion",
                )),
                None,
            );
        };
        driver.finalize_with_runtime(result).await
    }

    pub(crate) async fn finish_with_result_and_runtime(
        self,
        result: Result<Value>,
    ) -> (Result<Value>, Option<ReusableV8Runtime>) {
        let CooperativeLockerRuntimeSlot {
            driver,
            resolve,
            wait_until,
            response_ready,
            response_ready_tx,
            resolve_started_at: _,
            fresh_realm,
            wake_flag: _,
            completed,
        } = self;
        drop(resolve);
        drop(wait_until);
        drop(response_ready_tx);
        let mut driver = match completed {
            Some((driver, _)) => driver,
            None if response_ready.is_some() => {
                response_ready
                    .expect("response-ready state should contain a driver")
                    .0
            }
            None => match driver {
                Some(driver) => driver,
                None => {
                    return (
                        Err(runtime_js_error(
                            "cooperative runtime slot result requested after completion",
                        )),
                        None,
                    );
                }
            },
        };
        if let Some(realm) = fresh_realm {
            let destroy_started_at = Instant::now();
            destroy_fresh_realm(&mut driver.runtime, realm);
            driver.record_fresh_realm_destroy(destroy_started_at.elapsed());
        }
        driver.finalize_with_runtime(result).await
    }

    pub(crate) fn is_ready_to_resume(&self) -> bool {
        self.wake_flag.is_woken()
    }
}

impl NimbusRuntime {
    pub(crate) async fn start_cooperative_locker_runtime_slot(
        &self,
        v8_runtime_pool: &mut V8WorkerRuntimePool,
        start: CooperativeRuntimeSlotStart,
    ) -> Result<CooperativeLockerRuntimeSlot> {
        let CooperativeRuntimeSlotStart {
            invocation:
                RuntimeInvocationExecution {
                    watchdog,
                    bundle,
                    request,
                    context,
                    execution_plan,
                    external_cancellation,
                    response_ready_tx,
                    permit,
                },
            activity_signal,
        } = start;
        let integrity_started_at = Instant::now();
        let integrity_result = bundle.verify_integrity();
        self.policy
            .metrics()
            .record_bundle_integrity_verify(integrity_started_at.elapsed());
        integrity_result?;
        self.policy
            .validate_bundle_content_kind(bundle.content_kind())?;
        let runtime = v8_runtime_pool.take_runtime_with_options_for_invocation(
            self,
            &bundle,
            Some(&context),
            true,
        )?;
        let mut driver =
            self.prepare_runtime_invocation_driver(RuntimeInvocationDriverPrepare {
                runtime,
                watchdog,
                external_cancellation,
                permit,
                context: &context,
                execution_plan: Some(&execution_plan),
                record_replacement_on_error: true,
            })?;
        let context_recycling = matches!(
            self.policy.limits().runtime_pool_kind,
            crate::limits::RuntimePoolKind::WarmContextRecycle,
        );
        let is_warm_hit = matches!(
            self.policy.limits().runtime_pool_kind,
            crate::limits::RuntimePoolKind::WarmPool,
        ) && driver.warm_reuse_count > 0;
        if !context_recycling
            && !is_warm_hit
            && let Err(error) = self
                .load_bundle_with_trace(
                    &mut driver.runtime,
                    &bundle,
                    driver.construction_mode,
                    Some(&context),
                    Some(&request),
                )
                .await
        {
            let error = driver.finalize(Err(error)).await.expect_err(
                "cooperative slot startup error finalization should preserve the failure",
            );
            return Err(error);
        }

        let (value, fresh_realm) = if context_recycling {
            let (value, realm) = self
                .start_fresh_realm_bundle_invocation_with_trace(
                    &mut driver.runtime,
                    FreshRealmInvocationTrace {
                        bundle: &bundle,
                        request: &request,
                        construction_mode: driver.construction_mode,
                        context: Some(&context),
                    },
                )
                .await?;
            (Ok(value), Some(realm))
        } else {
            let request_json = serde_json::to_string(&request)?;
            let expression = format!("globalThis.__nimbusInvoke({request_json})");
            (
                driver
                    .runtime
                    .execute_script("<nimbus-runtime:invoke>", expression)
                    .map_err(runtime_js_error),
                None,
            )
        };
        let value = match value {
            Ok(value) => value,
            Err(error) => {
                let error = driver.finalize(Err(error)).await.expect_err(
                    "cooperative slot startup error finalization should preserve the failure",
                );
                return Err(error);
            }
        };
        let resolve: CooperativePromiseFuture = if let Some(realm) = fresh_realm.as_ref() {
            Box::pin(driver.runtime.resolve_in_realm(realm, value))
        } else {
            Box::pin(driver.runtime.resolve(value))
        };
        let wake_flag = Arc::new(CooperativeRuntimeWakeFlag::new(activity_signal));
        driver.runtime.release_v8_lock();
        Ok(CooperativeLockerRuntimeSlot {
            driver: Some(driver),
            resolve: Some(resolve),
            resolve_started_at: Some(Instant::now()),
            wait_until: None,
            response_ready: None,
            response_ready_tx,
            fresh_realm,
            wake_flag,
            completed: None,
        })
    }
}
