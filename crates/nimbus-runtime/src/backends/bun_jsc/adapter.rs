use std::future::Future;
use std::pin::Pin;

use serde_json::Value;

use crate::backends::RuntimeBackendInvocation;
use crate::error::Result;
use crate::limits::RuntimeExecutionAdapterState;

use super::pool::{BunJscPool, BunJscPoolPolicy};

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
pub(crate) struct BunJscNoLinkExecutionAdapterFactory;

impl BunJscExecutionAdapterFactory for BunJscNoLinkExecutionAdapterFactory {
    fn create(&self) -> Box<dyn BunJscExecutionAdapter> {
        Box::<BunJscNoLinkExecutionAdapter>::default()
    }
}

#[derive(Debug, Default)]
struct BunJscNoLinkExecutionAdapter;

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
