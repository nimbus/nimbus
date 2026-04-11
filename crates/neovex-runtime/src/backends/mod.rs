use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::RuntimeInvocationContext;
use crate::error::Result;
use crate::executor::SharedInvocationPermit;
use crate::host::HostCallCancellation;
use crate::runtime::{InvocationRequest, NeovexRuntime, RuntimeBundle};
use crate::watchdog::WatchdogTimer;

pub(crate) mod v8;

pub(crate) trait RuntimeBackendFactory: Send + Sync + 'static {
    fn create(&self) -> Box<dyn RuntimeBackend>;
}

pub(crate) struct RuntimeBackendInvocation {
    pub(crate) watchdog: WatchdogTimer,
    pub(crate) runtime: NeovexRuntime,
    pub(crate) bundle: RuntimeBundle,
    pub(crate) request: InvocationRequest,
    pub(crate) context: RuntimeInvocationContext,
    pub(crate) cancellation: Option<HostCallCancellation>,
    pub(crate) permit: SharedInvocationPermit,
}

pub(crate) trait RuntimeBackend: 'static {
    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>>;
}
