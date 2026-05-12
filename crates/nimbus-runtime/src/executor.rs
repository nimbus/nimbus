#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use crate::context::RuntimeInvocationContext;
#[cfg(test)]
use crate::error::{NimbusRuntimeError, Result};
#[cfg(test)]
use crate::host::HostCallCancellation;
#[cfg(test)]
use crate::limits::RuntimePolicy;
#[cfg(test)]
use crate::runtime::{InvocationRequest, NimbusRuntime, RuntimeBundle};

mod admission;
mod facade;
mod invoke;
mod lifecycle;
mod queue;

pub(crate) use self::admission::SharedInvocationPermit;
pub use self::facade::RuntimeExecutor;
#[cfg(test)]
pub(crate) use self::facade::RuntimeExecutorTestState;
pub(crate) use self::lifecycle::run_invocation_lifecycle;
pub(crate) use self::queue::RuntimeWorkerJob;
pub(crate) use self::queue::{RuntimeWorkerQueue, RuntimeWorkerShutdown, WorkerActivitySignal};

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};

    use super::*;
    use crate::test_support::{
        bounded_fairness_runtime_test_limits, cooperative_startup_snapshot_runtime_test_limits,
        cooperative_warm_pool_runtime_test_limits, product_default_runtime_test_policy,
        run_to_completion_snapshot_runtime_test_limits,
    };

    use self::support::*;

    mod cooperative;
    mod lifecycle;
    pub(crate) mod queue_fairness;
    mod router_affinity;
    mod support;
}
