mod affinity;
mod backends;
mod context;
mod error;
mod executor;
mod host;
mod limits;
mod metrics;
mod module_loader;
mod runtime;
#[cfg(test)]
mod test_support;
mod watchdog;
mod worker_loop;

pub use context::RuntimeInvocationContext;
pub use error::{NeovexRuntimeError, Result};
pub use executor::RuntimeExecutor;
pub use host::{
    HostBridge, HostBridgeFuture, HostCallCancellation, HostCallCancellationCause,
    HostCallOperation, HostCallRequest,
};
pub use limits::{
    RuntimeBackendKind, RuntimeExecutionModel, RuntimeLimits, RuntimeModuleStateSemantics,
    RuntimePolicy, RuntimePoolKind, RuntimeResetCapabilities, RuntimeRoutingAffinity,
};
pub use metrics::{
    RuntimeDurationDistributionSnapshot, RuntimeHostOperationMetricsSnapshot, RuntimeMetrics,
    RuntimeMetricsSnapshot, RuntimeRequestCorrelationSnapshot, RuntimeTenantMetricsSnapshot,
};
pub use runtime::{
    InvocationAuth, InvocationKind, InvocationRequest, InvocationServiceBinding,
    InvocationServiceEndpoint, InvocationServiceProtocol, InvocationServices, NeovexRuntime,
    RuntimeBundle, RuntimeUserIdentity, VerifiedUserIdentity, VerifiedUserIdentityKind,
};
