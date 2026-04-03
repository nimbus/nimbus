use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::RuntimeInvocationContext;
use crate::error::Result;
use crate::executor::SharedInvocationPermit;
use crate::host::HostCallCancellation;
use crate::runtime::{InvocationRequest, NeovexRuntime, RuntimeBundle, RuntimeWorkerIsolatePool};
use crate::watchdog::WatchdogTimer;

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

pub(crate) trait RuntimeBackend: Send + 'static {
    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>>;
}

#[derive(Debug, Default)]
pub(crate) struct DenoRuntimeBackendFactory;

impl RuntimeBackendFactory for DenoRuntimeBackendFactory {
    fn create(&self) -> Box<dyn RuntimeBackend> {
        Box::new(DenoRuntimeBackend {
            isolate_pool: RuntimeWorkerIsolatePool::new(),
        })
    }
}

struct DenoRuntimeBackend {
    isolate_pool: RuntimeWorkerIsolatePool,
}

impl RuntimeBackend for DenoRuntimeBackend {
    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>> {
        let RuntimeBackendInvocation {
            watchdog,
            runtime,
            bundle,
            request,
            context,
            cancellation,
            permit,
        } = invocation;
        Box::pin(async move {
            runtime
                .invoke_bundle_unmanaged(
                    Some(&mut self.isolate_pool),
                    crate::runtime::RuntimeInvocationExecution {
                        watchdog,
                        bundle,
                        request,
                        context,
                        external_cancellation: cancellation,
                        permit,
                    },
                )
                .await
        })
    }
}
