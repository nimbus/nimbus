use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;

use crate::RuntimeInvocationContext;
use crate::error::Result;
use crate::executor::SharedInvocationPermit;
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::runtime::{InvocationRequest, RuntimeBundle, RuntimeHost};
use crate::watchdog::WatchdogTimer;

pub(crate) mod v8;

pub(crate) trait RuntimeBackendFactory: Send + Sync + 'static {
    fn create(&self) -> Box<dyn RuntimeBackend>;
}

pub(crate) struct RuntimeBackendInvocation {
    pub(crate) watchdog: WatchdogTimer,
    pub(crate) host: RuntimeHost,
    pub(crate) policy: Arc<RuntimePolicy>,
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
