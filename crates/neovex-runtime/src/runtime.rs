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
    use std::sync::Arc;

    use super::*;
    use crate::test_support::{
        IsolatedRuntimeTestCase, acquire_runtime_suite_lock, acquire_snapshot_reset_test_lock,
        cooperative_startup_snapshot_runtime_test_limits,
        cooperative_startup_snapshot_runtime_test_policy,
        cooperative_warm_pool_runtime_test_limits, cooperative_warm_pool_runtime_test_policy,
        product_default_runtime_test_limits, run_to_completion_snapshot_runtime_test_limits,
        run_to_completion_snapshot_runtime_test_policy,
        run_v8_sensitive_runtime_test_in_subprocess,
    };
    use crate::{HostCallCancellation, HostCallOperation, HostCallRequest};

    use self::support::*;

    mod basic_invocation;
    mod bundle_integrity;
    mod cooperative;
    mod host_bridge;
    mod locker;
    mod pool_reuse;
    mod snapshot_lifecycle;
    mod support;
    mod timeout_cancellation;
    mod verification_harness;
    mod warm_pool;
}
