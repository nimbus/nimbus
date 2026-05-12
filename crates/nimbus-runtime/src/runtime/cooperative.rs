use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};

use serde_json::Value;

use crate::RuntimeInvocationContext;
use crate::backends::v8::embedder::{JsError, PollEventLoopOptions, v8};
use crate::backends::v8::{ReusableV8Runtime, V8WorkerRuntimePool};
use crate::error::Result;
use crate::executor::{SharedInvocationPermit, WorkerActivitySignal};
use crate::host::HostCallCancellation;
use crate::watchdog::WatchdogTimer;

use super::helpers::{deserialize_json_value, runtime_js_error};
use super::{InvocationRequest, NimbusRuntime, RuntimeBundle, RuntimeInvocationDriver};

pub(crate) struct RuntimeInvocationExecution {
    pub(crate) watchdog: WatchdogTimer,
    pub(crate) bundle: RuntimeBundle,
    pub(crate) request: InvocationRequest,
    pub(crate) context: RuntimeInvocationContext,
    pub(crate) external_cancellation: Option<HostCallCancellation>,
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
    resolve: CooperativePromiseFuture,
    wake_flag: Arc<CooperativeRuntimeWakeFlag>,
    completed: Option<(RuntimeInvocationDriver, Result<Value>)>,
}

impl CooperativeLockerRuntimeSlot {
    fn poll_once_now(&mut self) -> Result<CooperativeRuntimeSlotPoll> {
        let driver = self
            .driver
            .as_mut()
            .ok_or_else(|| runtime_js_error("cooperative runtime slot polled after completion"))?;
        let mut locked = driver.runtime.acquire_v8_lock();
        let waker = Waker::from(self.wake_flag.clone());
        let mut cx = Context::from_waker(&waker);
        if let Poll::Ready(result) = self.resolve.as_mut().poll(&mut cx) {
            let result: Result<Value> = result
                .map_err(runtime_js_error)
                .and_then(|value| deserialize_json_value(&mut locked, value));
            drop(locked);
            let driver = self
                .driver
                .take()
                .expect("driver should exist until completion");
            self.completed = Some((driver, result));
            return Ok(CooperativeRuntimeSlotPoll::Completed);
        }

        match locked.poll_event_loop(&mut cx, PollEventLoopOptions::default()) {
            Poll::Ready(Ok(())) => {
                let result: Result<Value> = match self.resolve.as_mut().poll(&mut cx) {
                    Poll::Ready(result) => result
                        .map_err(runtime_js_error)
                        .and_then(|value| deserialize_json_value(&mut locked, value)),
                    Poll::Pending => Err(runtime_js_error(
                        "Promise resolution is still pending but the event loop has already resolved",
                    )),
                };
                drop(locked);
                let driver = self
                    .driver
                    .take()
                    .expect("driver should exist until completion");
                self.completed = Some((driver, result));
                Ok(CooperativeRuntimeSlotPoll::Completed)
            }
            Poll::Ready(Err(error)) => {
                drop(locked);
                let driver = self
                    .driver
                    .take()
                    .expect("driver should exist until completion");
                self.completed = Some((driver, Err(runtime_js_error(error))));
                Ok(CooperativeRuntimeSlotPoll::Completed)
            }
            Poll::Pending => {
                drop(locked);
                if self.wake_flag.take_woken() {
                    Ok(CooperativeRuntimeSlotPoll::Runnable)
                } else {
                    Ok(CooperativeRuntimeSlotPoll::Parked)
                }
            }
        }
    }

    pub(crate) async fn poll_once(&mut self) -> Result<CooperativeRuntimeSlotPoll> {
        let poll = self.poll_once_now()?;
        if poll != CooperativeRuntimeSlotPoll::Parked {
            return Ok(poll);
        }

        tokio::task::yield_now().await;
        self.poll_once_now()
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
        let mut slot = self;
        let Some((driver, _)) = slot.completed.take() else {
            return (
                Err(runtime_js_error(
                    "cooperative runtime slot result requested before completion",
                )),
                None,
            );
        };
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
                    external_cancellation,
                    permit,
                },
            activity_signal,
        } = start;
        bundle.verify_integrity()?;
        let runtime = v8_runtime_pool.take_runtime_with_options_for_invocation(
            self,
            &bundle,
            Some(&context),
            true,
        )?;
        let mut driver = self.prepare_runtime_invocation_driver(
            runtime,
            watchdog,
            external_cancellation,
            permit,
            true,
        )?;
        let is_warm_hit = matches!(
            self.policy.limits().runtime_pool_kind,
            crate::limits::RuntimePoolKind::WarmPool,
        ) && driver.warm_reuse_count > 0;
        if !is_warm_hit
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

        let request_json = serde_json::to_string(&request)?;
        let expression = format!("globalThis.__nimbusInvoke({request_json})");
        let value = driver
            .runtime
            .execute_script("<nimbus-runtime:invoke>", expression)
            .map_err(runtime_js_error);
        let value = match value {
            Ok(value) => value,
            Err(error) => {
                let error = driver.finalize(Err(error)).await.expect_err(
                    "cooperative slot startup error finalization should preserve the failure",
                );
                return Err(error);
            }
        };
        let resolve = Box::pin(driver.runtime.resolve(value));
        let wake_flag = Arc::new(CooperativeRuntimeWakeFlag::new(activity_signal));
        driver.runtime.release_v8_lock();
        Ok(CooperativeLockerRuntimeSlot {
            driver: Some(driver),
            resolve,
            wake_flag,
            completed: None,
        })
    }
}
