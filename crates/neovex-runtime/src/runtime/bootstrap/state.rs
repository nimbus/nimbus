use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use deno_permissions::PermissionsContainer;
use deno_web::{JsMessageData, MessagePort};

use crate::RuntimeBundle;
use crate::backends::v8::embedder::{CancelHandle, JsRuntime};
use crate::error::Result;
use crate::executor::SharedInvocationPermit;
use crate::host::{HostBridge, HostCallCancellation};
use crate::limits::{RuntimeCompatibilityTarget, RuntimeProfile};
use crate::runtime::NeovexRuntime;
use crate::runtime_capabilities::{
    RuntimeEnvPolicy, RuntimePathPolicy, build_permissions_container,
};
use crate::watchdog::{WatchdogRegistration, WatchdogTimer};

#[derive(Default)]
struct RuntimeHostBridgeSlotState {
    bridge: Option<Arc<dyn HostBridge>>,
}

#[derive(Clone, Default)]
pub(crate) struct RuntimeHostBridgeSlot {
    state: Arc<Mutex<RuntimeHostBridgeSlotState>>,
}

impl RuntimeHostBridgeSlot {
    pub(crate) fn new(initial_bridge: Arc<dyn HostBridge>) -> Self {
        let slot = Self::default();
        slot.bind(initial_bridge);
        slot
    }

    pub(crate) fn bind(&self, bridge: Arc<dyn HostBridge>) {
        self.state
            .lock()
            .expect("runtime host bridge slot lock should not be poisoned")
            .bridge = Some(bridge);
    }

    pub(crate) fn current(&self) -> Arc<dyn HostBridge> {
        self.state
            .lock()
            .expect("runtime host bridge slot lock should not be poisoned")
            .bridge
            .as_ref()
            .cloned()
            .expect("runtime host bridge slot should be bound before invocation")
    }
}

#[derive(Clone)]
pub(super) struct InstalledRuntimeHostBridge {
    pub(super) slot: RuntimeHostBridgeSlot,
}

#[derive(Clone)]
pub(crate) struct InstalledRuntimeOwner {
    pub(crate) runtime: NeovexRuntime,
}

#[derive(Clone, Copy)]
pub(super) struct InstalledRuntimeContract {
    pub(super) compatibility_target: RuntimeCompatibilityTarget,
    pub(super) profile: RuntimeProfile,
}

#[derive(Clone)]
pub(super) struct InstalledRuntimeCapabilityPolicy {
    pub(super) paths: RuntimePathPolicy,
    pub(super) env: RuntimeEnvPolicy,
    pub(super) permissions: PermissionsContainer,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeWorkerBootstrapDescriptor {
    pub(crate) running_on_main_thread: bool,
    pub(crate) worker_id: u32,
    pub(crate) close_on_idle: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) module_specifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) worker_metadata: Option<JsMessageData>,
}

pub(crate) struct InstalledRuntimeWorkerBootstrapState {
    pub(crate) descriptor: RuntimeWorkerBootstrapDescriptor,
    pub(crate) parent_port: Option<Rc<MessagePort>>,
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

pub(crate) fn initialize_runtime_state(
    runtime: &mut JsRuntime,
    runtime_owner: &NeovexRuntime,
    bundle: &RuntimeBundle,
) -> Result<()> {
    install_runtime_owner(runtime, runtime_owner.clone());
    install_runtime_host_bridge_slot(runtime, runtime_owner.host.clone());
    install_runtime_contract(runtime, runtime_owner, bundle)?;
    if runtime_owner
        .policy()
        .limits()
        .compatibility_target
        .is_node()
    {
        let inspector = runtime.inspector();
        let module_specifier = bundle.module_specifier()?;
        let op_state = runtime.op_state();
        let mut op_state = op_state.borrow_mut();
        op_state.put(inspector);
        op_state.put(module_specifier);
    }
    reset_runtime_invocation_state(
        runtime,
        SharedInvocationPermit::new(runtime_owner.policy.clone(), None, None, true, None),
    );
    Ok(())
}

fn install_runtime_contract(
    runtime: &mut JsRuntime,
    runtime_owner: &NeovexRuntime,
    bundle: &RuntimeBundle,
) -> Result<()> {
    let limits = runtime_owner.policy().limits().clone();
    let paths = RuntimePathPolicy::for_bundle(bundle, &limits)?;
    let env = RuntimeEnvPolicy::for_profile(limits.profile);
    let capability_policy = InstalledRuntimeCapabilityPolicy {
        permissions: build_permissions_container(&paths, &env, &limits)?,
        paths,
        env,
    };
    let op_state = runtime.op_state();
    let mut state = op_state.borrow_mut();
    state.put(InstalledRuntimeContract {
        compatibility_target: limits.compatibility_target,
        profile: limits.profile,
    });
    state.put(capability_policy.permissions.clone());
    state.put(capability_policy);
    Ok(())
}

pub(crate) fn install_runtime_owner(runtime: &mut JsRuntime, runtime_owner: NeovexRuntime) {
    let op_state = runtime.op_state();
    let mut state = op_state.borrow_mut();
    state.put(InstalledRuntimeOwner {
        runtime: runtime_owner,
    });
}

pub(crate) fn install_runtime_host_bridge_slot(
    runtime: &mut JsRuntime,
    bridge: Arc<dyn HostBridge>,
) {
    let op_state = runtime.op_state();
    let mut state = op_state.borrow_mut();
    state.put(InstalledRuntimeHostBridge {
        slot: RuntimeHostBridgeSlot::new(bridge),
    });
}

pub(crate) fn main_thread_worker_bootstrap_state() -> InstalledRuntimeWorkerBootstrapState {
    InstalledRuntimeWorkerBootstrapState {
        descriptor: RuntimeWorkerBootstrapDescriptor {
            running_on_main_thread: true,
            worker_id: 0,
            close_on_idle: false,
            module_specifier: None,
            worker_metadata: None,
        },
        parent_port: None,
    }
}

pub(crate) fn bind_runtime_host_bridge(runtime: &mut JsRuntime, bridge: Arc<dyn HostBridge>) {
    let op_state = runtime.op_state();
    let state = op_state.borrow();
    state
        .borrow::<InstalledRuntimeHostBridge>()
        .slot
        .bind(bridge);
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
