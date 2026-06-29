use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::task::{ArcWake, waker_ref};
use serde_json::Value;

use crate::backends::{RuntimeBackend, RuntimeBackendInvocation};
use crate::error::{NimbusRuntimeError, Result};
use crate::execution_plan::RuntimeExecutionPlan;
use crate::executor::WorkerActivitySignal;
use crate::limits::{RuntimePolicy, RuntimePoolKind};
use crate::runtime::CooperativeRuntimeSlotPoll;
use crate::worker_loop::cooperative::backend::{
    CooperativeBackendDriver, CooperativeBackendFinishFuture, CooperativeBackendInvocationStart,
    CooperativeBackendPollFuture, CooperativeBackendSlot,
};

use super::WasmtimeBackendFactory;

const WASMTIME_FUEL_POLL_BUDGET: usize = 256;

pub(crate) struct WasmtimeFuelDriver {
    backend_factory: WasmtimeBackendFactory,
}

impl WasmtimeFuelDriver {
    pub(crate) fn new() -> Self {
        Self {
            backend_factory: WasmtimeBackendFactory::new(),
        }
    }
}

pub(crate) struct WasmtimeFuelSlot {
    state: WasmtimeFuelSlotState,
}

enum WasmtimeFuelSlotState {
    Pending(Option<Box<CooperativeBackendInvocationStart>>),
    Running {
        future: WasmtimeFuelFuture,
        timeout: Pin<Box<tokio::time::Sleep>>,
        wake: Arc<WasmtimeFuelWake>,
        started_at: Instant,
        timeout_budget: Duration,
        cancellation: Option<crate::host::HostCallCancellation>,
    },
    Completed(Option<Result<Value>>),
}

type WasmtimeFuelFuture = Pin<Box<dyn Future<Output = Result<Value>> + 'static>>;

struct WasmtimeFuelWake {
    activity_signal: Arc<WorkerActivitySignal>,
    notified: AtomicBool,
}

impl ArcWake for WasmtimeFuelWake {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.notified.store(true, Ordering::SeqCst);
        arc_self.activity_signal.notify();
    }
}

impl WasmtimeFuelWake {
    fn take_notified(&self) -> bool {
        self.notified.swap(false, Ordering::SeqCst)
    }
}

impl WasmtimeFuelSlot {
    fn new(start: CooperativeBackendInvocationStart) -> Self {
        Self {
            state: WasmtimeFuelSlotState::Pending(Some(Box::new(start))),
        }
    }

    fn start_pending(&mut self, start: CooperativeBackendInvocationStart) {
        let CooperativeBackendInvocationStart {
            watchdog,
            host,
            policy,
            bundle,
            request,
            context,
            execution_plan: _execution_plan,
            cancellation,
            response_ready_tx: _response_ready_tx,
            permit,
            activity_signal,
        } = start;
        let timeout = policy.limits().execution_timeout;
        let cancellation_for_slot = cancellation.clone();
        let wake = Arc::new(WasmtimeFuelWake {
            activity_signal: activity_signal.clone(),
            notified: AtomicBool::new(false),
        });

        if let Some(cancellation) = cancellation_for_slot.clone() {
            let activity_on_cancel = activity_signal.clone();
            cancellation.notify_on_cancel(move || activity_on_cancel.notify());
        }

        let future = Box::pin(async move {
            tokio::task::yield_now().await;
            let mut backend = WasmtimeBackendFactory::new().create_typed()?;
            backend
                .invoke(RuntimeBackendInvocation {
                    watchdog,
                    host,
                    policy,
                    bundle,
                    request,
                    context,
                    cancellation,
                    permit,
                })
                .await
        });

        self.state = WasmtimeFuelSlotState::Running {
            future,
            timeout: Box::pin(tokio::time::sleep(timeout)),
            wake,
            started_at: Instant::now(),
            timeout_budget: timeout,
            cancellation: cancellation_for_slot,
        };
    }

    fn timed_out(started_at: Instant, timeout: Duration) -> bool {
        started_at.elapsed() >= timeout
    }

    fn take_running_future(&mut self) -> Option<WasmtimeFuelFuture> {
        match std::mem::replace(&mut self.state, WasmtimeFuelSlotState::Completed(None)) {
            WasmtimeFuelSlotState::Running { future, .. } => Some(future),
            other => {
                self.state = other;
                None
            }
        }
    }

    fn complete_with(&mut self, result: Result<Value>) {
        self.state = WasmtimeFuelSlotState::Completed(Some(result));
    }
}

impl CooperativeBackendSlot for WasmtimeFuelSlot {
    type ReusableRuntime = ();

    fn poll_once<'a>(&'a mut self) -> CooperativeBackendPollFuture<'a> {
        Box::pin(async move {
            let pending = match &mut self.state {
                WasmtimeFuelSlotState::Pending(start) => start.take(),
                _ => None,
            };
            if let Some(start) = pending {
                self.start_pending(*start);
                return Ok(CooperativeRuntimeSlotPoll::Runnable);
            }

            let timed_out_by_clock = matches!(
                &self.state,
                WasmtimeFuelSlotState::Running {
                    started_at,
                    timeout_budget,
                    ..
                } if Self::timed_out(*started_at, *timeout_budget)
            );

            let mut timed_out_by_waker = false;
            if let WasmtimeFuelSlotState::Running { timeout, wake, .. } = &mut self.state {
                let waker = waker_ref(wake);
                let mut cx = Context::from_waker(&waker);
                timed_out_by_waker = matches!(timeout.as_mut().poll(&mut cx), Poll::Ready(()));
            }

            if timed_out_by_clock || timed_out_by_waker {
                let timeout = self.timeout_for_running_slot();
                let _ = self.take_running_future();
                self.complete_with(Err(NimbusRuntimeError::ExecutionTimeout(timeout)));
                return Ok(CooperativeRuntimeSlotPoll::Completed);
            }

            let cancelled = matches!(
                &self.state,
                WasmtimeFuelSlotState::Running {
                    cancellation: Some(cancellation),
                    ..
                } if cancellation.is_cancelled()
            );
            if cancelled {
                let _ = self.take_running_future();
                self.complete_with(Err(NimbusRuntimeError::Cancelled));
                return Ok(CooperativeRuntimeSlotPoll::Completed);
            }

            if let WasmtimeFuelSlotState::Running { future, wake, .. } = &mut self.state {
                for _ in 0..WASMTIME_FUEL_POLL_BUDGET {
                    let _ = wake.take_notified();
                    let waker = waker_ref(wake);
                    let mut cx = Context::from_waker(&waker);
                    if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
                        self.complete_with(result);
                        return Ok(CooperativeRuntimeSlotPoll::Completed);
                    }
                }
                return Ok(CooperativeRuntimeSlotPoll::Runnable);
            }

            match self.state {
                WasmtimeFuelSlotState::Completed(_) => Ok(CooperativeRuntimeSlotPoll::Completed),
                WasmtimeFuelSlotState::Running { .. } => Ok(CooperativeRuntimeSlotPoll::Parked),
                WasmtimeFuelSlotState::Pending(_) => Ok(CooperativeRuntimeSlotPoll::Runnable),
            }
        })
    }

    fn finish_with_runtime<'a>(self) -> CooperativeBackendFinishFuture<'a, Self::ReusableRuntime>
    where
        Self: Sized,
    {
        Box::pin(async move {
            let result = match self.state {
                WasmtimeFuelSlotState::Completed(result) => result.unwrap_or_else(|| {
                    Err(NimbusRuntimeError::Contract(
                        "WasmtimeFuelSlot completed without a result".to_string(),
                    ))
                }),
                WasmtimeFuelSlotState::Running { .. } => Err(NimbusRuntimeError::Contract(
                    "WasmtimeFuelSlot finished before its parked future completed".to_string(),
                )),
                WasmtimeFuelSlotState::Pending(_) => Err(NimbusRuntimeError::Contract(
                    "WasmtimeFuelSlot finished before it started".to_string(),
                )),
            };
            (result, None)
        })
    }

    fn finish_with_result_and_runtime<'a>(
        self,
        result: Result<Value>,
    ) -> CooperativeBackendFinishFuture<'a, Self::ReusableRuntime>
    where
        Self: Sized,
    {
        Box::pin(async move { (result, None) })
    }

    fn is_ready_to_resume(&self) -> bool {
        match &self.state {
            WasmtimeFuelSlotState::Pending(_) => true,
            WasmtimeFuelSlotState::Running {
                started_at,
                timeout_budget,
                cancellation,
                wake,
                ..
            } => {
                Self::timed_out(*started_at, *timeout_budget)
                    || wake.take_notified()
                    || cancellation
                        .as_ref()
                        .is_some_and(crate::host::HostCallCancellation::is_cancelled)
            }
            WasmtimeFuelSlotState::Completed(_) => true,
        }
    }
}

impl WasmtimeFuelSlot {
    fn timeout_for_running_slot(&self) -> Duration {
        match &self.state {
            WasmtimeFuelSlotState::Running { timeout_budget, .. } => *timeout_budget,
            WasmtimeFuelSlotState::Pending(start) => start
                .as_ref()
                .map(|start| start.policy.limits().execution_timeout)
                .unwrap_or_default(),
            WasmtimeFuelSlotState::Completed(_) => Duration::default(),
        }
    }
}

impl CooperativeBackendDriver for WasmtimeFuelDriver {
    type Slot = WasmtimeFuelSlot;

    fn permits_scheduler_admission(&self, _execution_plan: &RuntimeExecutionPlan) -> bool {
        true
    }

    fn start_slot<'a>(
        &'a mut self,
        start: CooperativeBackendInvocationStart,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Slot>> + 'a>> {
        Box::pin(async move { Ok(WasmtimeFuelSlot::new(start)) })
    }

    fn invoke_direct<'a>(
        &'a mut self,
        start: CooperativeBackendInvocationStart,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>> {
        Box::pin(async move {
            let CooperativeBackendInvocationStart {
                watchdog,
                host,
                policy,
                bundle,
                request,
                context,
                execution_plan: _execution_plan,
                cancellation,
                response_ready_tx: _response_ready_tx,
                permit,
                activity_signal: _activity_signal,
            } = start;
            let mut backend = self.backend_factory.create_typed()?;
            backend
                .invoke(RuntimeBackendInvocation {
                    watchdog,
                    host,
                    policy,
                    bundle,
                    request,
                    context,
                    cancellation,
                    permit,
                })
                .await
        })
    }

    fn retain_reusable_runtime(
        &mut self,
        policy: std::sync::Arc<RuntimePolicy>,
        _host: &crate::runtime::RuntimeHost,
        _bundle: &crate::runtime::RuntimeBundle,
        _context: &crate::RuntimeInvocationContext,
        _reusable_runtime: <Self::Slot as CooperativeBackendSlot>::ReusableRuntime,
    ) {
        match policy.limits().runtime_pool_kind {
            RuntimePoolKind::PrecompiledModuleCache => {}
            RuntimePoolKind::RetainedStorePool => {
                unreachable!("retained Wasmtime Store pooling is owned by W6")
            }
            RuntimePoolKind::StartupSnapshotCache
            | RuntimePoolKind::WarmPool
            | RuntimePoolKind::WarmContextRecycle
            | RuntimePoolKind::BunJscTrustedRetained
            | RuntimePoolKind::BunJscFreshDiscard => {
                unreachable!("non-Wasmtime pool kinds are rejected before Wasmtime invocation")
            }
        }
    }

    fn idle_maintenance(&mut self, _worker_is_idle: bool) {}

    fn clear_retained(&mut self) {}
}
