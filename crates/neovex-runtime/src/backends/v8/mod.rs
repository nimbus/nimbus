use std::future::Future;
use std::pin::Pin;

use crate::backends::{RuntimeBackend, RuntimeBackendFactory, RuntimeBackendInvocation};
use crate::error::Result;
use crate::runtime::RuntimeInvocationExecution;

pub(crate) mod embedder;
mod startup;
mod warm_pool;

use self::embedder::JsRuntime;

#[cfg(test)]
pub(crate) use self::startup::v8_bootstrap_snapshot_build_count_for_test;
pub(crate) use self::startup::{
    V8RuntimeConstructionMode, V8StartupSnapshot, create_v8_startup_snapshot,
};
pub(crate) use self::warm_pool::{ReusableV8Runtime, V8WorkerRuntimePool};

#[derive(Debug, Default)]
pub(crate) struct V8RuntimeBackendFactory;

impl RuntimeBackendFactory for V8RuntimeBackendFactory {
    fn create(&self) -> Box<dyn RuntimeBackend> {
        Box::new(V8RuntimeBackend {
            v8_runtime_pool: V8WorkerRuntimePool::new(),
        })
    }
}

struct V8RuntimeBackend {
    v8_runtime_pool: V8WorkerRuntimePool,
}

impl RuntimeBackend for V8RuntimeBackend {
    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value>> + 'a>> {
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
                    Some(&mut self.v8_runtime_pool),
                    RuntimeInvocationExecution {
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

#[derive(Default)]
pub(crate) struct DeferredV8RuntimeDropQueue {
    pending: Vec<JsRuntime>,
}

impl DeferredV8RuntimeDropQueue {
    pub(crate) fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    pub(crate) fn defer(&mut self, runtime: JsRuntime) {
        self.pending.push(runtime);
    }

    pub(crate) fn drain_if_idle(&mut self, worker_is_idle: bool) {
        if !worker_is_idle || self.pending.is_empty() {
            return;
        }

        self.pending.clear();
    }

    pub(crate) fn clear(&mut self) {
        self.pending.clear();
    }
}
