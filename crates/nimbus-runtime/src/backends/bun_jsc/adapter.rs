use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::backends::RuntimeBackendInvocation;
use crate::error::Result;
use crate::limits::RuntimeExecutionAdapterState;

#[cfg(any(test, not(feature = "bun-jsc-linked-adapter")))]
use super::pool::BunJscPool;
use super::pool::BunJscPoolPolicy;

pub(crate) trait BunJscExecutionAdapterFactory:
    std::fmt::Debug + Send + Sync + 'static
{
    fn create(&self) -> Box<dyn BunJscExecutionAdapter>;
}

pub(crate) trait BunJscExecutionAdapter: std::fmt::Debug + 'static {
    fn state(&self) -> RuntimeExecutionAdapterState;

    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
        pool_policy: BunJscPoolPolicy,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>>;
}

#[derive(Debug, Default)]
#[cfg(any(test, not(feature = "bun-jsc-linked-adapter")))]
pub(crate) struct BunJscNoLinkExecutionAdapterFactory;

#[cfg(any(test, not(feature = "bun-jsc-linked-adapter")))]
impl BunJscExecutionAdapterFactory for BunJscNoLinkExecutionAdapterFactory {
    fn create(&self) -> Box<dyn BunJscExecutionAdapter> {
        Box::<BunJscNoLinkExecutionAdapter>::default()
    }
}

#[derive(Debug, Default)]
#[cfg(any(test, not(feature = "bun-jsc-linked-adapter")))]
struct BunJscNoLinkExecutionAdapter;

#[cfg(any(test, not(feature = "bun-jsc-linked-adapter")))]
impl BunJscExecutionAdapter for BunJscNoLinkExecutionAdapter {
    fn state(&self) -> RuntimeExecutionAdapterState {
        RuntimeExecutionAdapterState::NotLinked
    }

    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
        pool_policy: BunJscPoolPolicy,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>> {
        drop(invocation);
        let _ = pool_policy;
        Box::pin(async { Err(BunJscPool::disabled_error()) })
    }
}
