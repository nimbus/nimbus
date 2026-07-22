use std::time::Instant;

use serde_json::Value;
use tokio::sync::oneshot;

use crate::context::RuntimeInvocationContext;
use crate::error::Result;
use crate::execution_plan::RuntimeExecutionPlan;
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::runtime::{InvocationRequest, RuntimeBundle, RuntimeHost};

use crate::executor::admission::RuntimeInvocationDispatchHandle;
use std::sync::Arc;

pub(crate) enum RuntimeWorkerMessage {
    Job(Box<RuntimeWorkerJob>),
    Control(RuntimeWorkerControl),
}

pub(crate) struct RuntimeWorkerControl {
    pub(crate) command: RuntimeWorkerControlCommand,
    pub(crate) acknowledged: oneshot::Sender<RuntimeWorkerRetirementAck>,
}

pub(crate) enum RuntimeWorkerControlCommand {
    RetireOwner(crate::RuntimeOwnerId),
    RetireDeploymentAuthority(crate::RuntimeDeploymentAuthorityId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeWorkerRetirementAck {
    pub(crate) worker_id: usize,
    pub(crate) retained_entries_purged: usize,
}

pub(crate) struct RuntimeWorkerJob {
    pub(crate) host: RuntimeHost,
    pub(crate) policy: Arc<RuntimePolicy>,
    pub(crate) bundle: RuntimeBundle,
    pub(crate) request: InvocationRequest,
    pub(crate) context: RuntimeInvocationContext,
    pub(crate) execution_plan: RuntimeExecutionPlan,
    pub(crate) cancellation: Option<HostCallCancellation>,
    pub(crate) enqueued_at: Instant,
    pub(crate) response_ready_tx: Option<oneshot::Sender<Value>>,
    pub(crate) result_tx: RuntimeWorkerResultSender,
    pub(crate) dispatch_handle: Option<RuntimeInvocationDispatchHandle>,
    pub(crate) _retirement_guard: Option<crate::executor::retirement::RuntimeRetirementGuard>,
}

pub(crate) enum RuntimeWorkerResultSender {
    Async(oneshot::Sender<Result<Value>>),
    Blocking(std::sync::mpsc::SyncSender<Result<Value>>),
}

impl RuntimeWorkerResultSender {
    pub(crate) fn send(self, result: Result<Value>) {
        match self {
            Self::Async(result_tx) => {
                let _ = result_tx.send(result);
            }
            Self::Blocking(result_tx) => {
                let _ = result_tx.send(result);
            }
        }
    }
}
