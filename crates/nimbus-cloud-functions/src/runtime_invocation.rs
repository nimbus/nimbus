use std::sync::Arc;

use nimbus_core::{Result, TenantId};
use nimbus_provenance::RuntimeBundleProvenanceConfig;
use nimbus_runtime::{
    HostBridge, InvocationRequest, RuntimeBundle, RuntimeExecutor, RuntimePolicy,
};
use serde_json::Value;

pub trait CloudFunctionsRuntimeInvoker: Send + Sync {
    fn invoke_runtime_bundle(&self, invocation: CloudFunctionsRuntimeInvocation) -> Result<Value>;
}

pub struct CloudFunctionsRuntimeInvocation {
    pub runtime_executor: Arc<RuntimeExecutor>,
    pub runtime_policy: Arc<RuntimePolicy>,
    pub host_bridge: Arc<dyn HostBridge>,
    pub bundle: RuntimeBundle,
    pub request: InvocationRequest,
    pub tenant_id: TenantId,
    pub server_request_id: Option<String>,
    pub provenance_gate: Option<RuntimeBundleProvenanceConfig>,
}
