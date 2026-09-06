use std::sync::Arc;
use std::sync::OnceLock;

#[cfg(test)]
use serde_json::Value;

#[cfg(test)]
use crate::RuntimeInvocationContext;
use crate::egress::RuntimeEgressGatewayBinding;
#[cfg(test)]
use crate::error::{NimbusRuntimeError, Result};
use crate::executor::RuntimeExecutor;
#[cfg(test)]
use crate::executor::SharedInvocationPermit;
use crate::host::HostBridge;
use crate::limits::RuntimePolicy;
#[cfg(test)]
use crate::watchdog::WatchdogTimer;

pub(crate) mod bootstrap;
pub(crate) mod bundle;
mod captured_dispatch;
mod classify;
mod cooperative;
pub(crate) mod driver;
mod facade;
mod invocation;

#[cfg(test)]
use self::bootstrap::RuntimeCancellationState;
pub(crate) use self::bootstrap::RuntimeInvocationTimeoutController;
pub(crate) use self::bundle::RuntimeBundleEntrypointKind;
pub use self::bundle::{
    RuntimeBundle, RuntimeBundleContent, RuntimeBundleWasmComponentContent, RuntimeComponentWorld,
};
#[cfg(test)]
use self::classify::deserialize_json_value;
pub use self::invocation::{
    InvocationKind, InvocationRequest, InvocationServiceBinding, InvocationServiceEndpoint,
    InvocationServiceProtocol, InvocationServices,
};

#[derive(Clone)]
pub struct NimbusRuntime {
    host: Arc<dyn HostBridge>,
    policy: Arc<RuntimePolicy>,
    egress_gateway: RuntimeEgressGatewayBinding,
    owned_executor: Arc<OnceLock<RuntimeExecutor>>,
}

#[derive(Clone)]
pub(crate) struct RuntimeHost {
    bridge: Arc<dyn HostBridge>,
    egress_gateway: RuntimeEgressGatewayBinding,
}

impl RuntimeHost {
    pub(crate) fn new_with_egress_gateway(
        bridge: Arc<dyn HostBridge>,
        egress_gateway: RuntimeEgressGatewayBinding,
    ) -> Self {
        Self {
            bridge,
            egress_gateway,
        }
    }

    pub(crate) fn from_runtime(runtime: &NimbusRuntime) -> Self {
        Self::new_with_egress_gateway(runtime.host.clone(), runtime.egress_gateway.clone())
    }

    pub(crate) fn runtime_with_policy(&self, policy: Arc<RuntimePolicy>) -> NimbusRuntime {
        NimbusRuntime::with_policy(
            self.bridge.clone(),
            policy,
            self.egress_gateway.clone().into_posture(),
        )
    }

    pub(crate) fn bridge(&self) -> Arc<dyn HostBridge> {
        self.bridge.clone()
    }
}

pub(crate) use self::cooperative::{
    CooperativeLockerRuntimeSlot, CooperativeRuntimeSlotPoll, CooperativeRuntimeSlotStart,
    RuntimeInvocationExecution,
};

pub(crate) use self::driver::{RuntimeInvocationDriver, RuntimeInvocationDriverPrepare};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    #[cfg(feature = "v8-pointer-compression")]
    use crate::test_support::run_v8_crash_control_in_subprocess;
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
    mod capture_ordering;
    mod captured_dispatch;
    mod cooperative;
    mod host_bridge;
    mod locker;
    #[path = "node/mod.rs"]
    mod node_compat;
    #[path = "node/canary_registry.rs"]
    mod node_compat_canary_registry;
    #[path = "node/manifest_catalog.rs"]
    mod node_compat_manifest_catalog;
    #[path = "node/manifest_metadata.rs"]
    mod node_compat_manifest_metadata;
    #[path = "node/manifest_report.rs"]
    mod node_compat_manifest_report;
    #[path = "node/manifest_resolution.rs"]
    mod node_compat_manifest_resolution;
    #[path = "node/manifest_topology.rs"]
    mod node_compat_manifest_topology;
    #[path = "node/oracle.rs"]
    mod node_compat_oracle;
    mod pool_reuse;
    mod snapshot_lifecycle;
    mod support;
    mod timeout_cancellation;
    mod verification_harness;
    mod warm_pool;
    mod wasmtime_fuel;
    mod wasmtime_store_pool;
}
