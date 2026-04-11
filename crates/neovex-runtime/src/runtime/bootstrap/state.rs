use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::backends::v8::embedder::{CancelHandle, JsRuntime};
use crate::error::Result;
use crate::executor::SharedInvocationPermit;
use crate::host::{HostBridge, HostCallCancellation};
use crate::runtime::NeovexRuntime;
use crate::watchdog::{WatchdogRegistration, WatchdogTimer};

#[derive(Clone)]
pub(super) struct RuntimeHostState {
    pub(super) bridge: Arc<dyn HostBridge>,
}

#[derive(Clone)]
pub(crate) struct RuntimeCancellationState {
    pub(crate) cancel_handle: Rc<CancelHandle>,
    pub(crate) signal: HostCallCancellation,
}

fn fresh_runtime_cancellation_state() -> RuntimeCancellationState {
    RuntimeCancellationState {
        cancel_handle: CancelHandle::new_rc(),
        signal: HostCallCancellation::default(),
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeInvocationTimeoutController {
    inner: Arc<Mutex<RuntimeInvocationTimeoutControllerState>>,
}

struct RuntimeInvocationTimeoutControllerState {
    timer: WatchdogTimer,
    remaining: Duration,
    armed_at: Option<Instant>,
    registration: Option<WatchdogRegistration>,
    callback: Arc<dyn Fn() + Send + Sync>,
    disarmed: bool,
}

impl RuntimeInvocationTimeoutController {
    pub(crate) fn new(
        timer: WatchdogTimer,
        timeout: Duration,
        callback: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let registration = if timeout.is_zero() {
            None
        } else {
            Some(Self::register(&timer, timeout, callback.clone())?)
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(RuntimeInvocationTimeoutControllerState {
                timer,
                remaining: timeout,
                armed_at: (!timeout.is_zero()).then_some(Instant::now()),
                registration,
                callback,
                disarmed: false,
            })),
        })
    }

    fn register(
        timer: &WatchdogTimer,
        timeout: Duration,
        callback: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<WatchdogRegistration> {
        timer.register_timeout(Instant::now() + timeout, move || {
            callback();
        })
    }

    pub(crate) async fn pause(&self) {
        let registration = {
            let mut state = self
                .inner
                .lock()
                .expect("runtime timeout controller lock should not be poisoned");
            if state.disarmed {
                return;
            }
            let Some(armed_at) = state.armed_at.take() else {
                return;
            };
            state.remaining = state.remaining.saturating_sub(armed_at.elapsed());
            state.registration.take()
        };
        if let Some(registration) = registration {
            registration.disarm().await;
        }
    }

    pub(crate) fn resume(&self) -> Result<()> {
        let mut state = self
            .inner
            .lock()
            .expect("runtime timeout controller lock should not be poisoned");
        if state.disarmed || state.remaining.is_zero() || state.registration.is_some() {
            return Ok(());
        }
        let registration = Self::register(&state.timer, state.remaining, state.callback.clone())?;
        state.armed_at = Some(Instant::now());
        state.registration = Some(registration);
        Ok(())
    }

    pub(crate) async fn disarm(&self) {
        let registration = {
            let mut state = self
                .inner
                .lock()
                .expect("runtime timeout controller lock should not be poisoned");
            state.disarmed = true;
            state.armed_at = None;
            state.registration.take()
        };
        if let Some(registration) = registration {
            registration.disarm().await;
        }
    }
}

pub(crate) fn initialize_runtime_state(runtime: &mut JsRuntime, runtime_owner: &NeovexRuntime) {
    let op_state = runtime.op_state();
    {
        let mut state = op_state.borrow_mut();
        state.put(RuntimeHostState {
            bridge: runtime_owner.host.clone(),
        });
    }
    reset_runtime_invocation_state(
        runtime,
        SharedInvocationPermit::new(runtime_owner.policy.clone(), None, None, true, None),
    );
}

pub(crate) fn reset_runtime_invocation_state(
    runtime: &mut JsRuntime,
    permit: SharedInvocationPermit,
) {
    let op_state = runtime.op_state();
    let mut state = op_state.borrow_mut();
    state.put(fresh_runtime_cancellation_state());
    state.put(permit);
}
