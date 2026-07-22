use std::sync::Arc;

use nimbus_core::{Result, TenantId};
use nimbus_provenance::RuntimeBundleProvenanceConfig;
use nimbus_runtime::{InvocationRequest, RuntimeBundle, RuntimeLimits, RuntimePolicy};
use serde_json::Value;

use crate::CloudFunctionsHostBridge;

pub trait CloudFunctionsRuntimeInvoker: Send + Sync {
    fn runtime_policy(&self, limits: &RuntimeLimits) -> Arc<RuntimePolicy>;

    fn invoke_runtime_bundle(&self, invocation: CloudFunctionsRuntimeInvocation) -> Result<Value>;
}

pub struct CloudFunctionsRuntimeInvocation {
    pub runtime_policy: Arc<RuntimePolicy>,
    pub host_bridge: Arc<CloudFunctionsHostBridge>,
    pub bundle: RuntimeBundle,
    pub request: InvocationRequest,
    pub tenant_id: TenantId,
    pub deployment_generation: u64,
    pub server_request_id: Option<String>,
    pub provenance_gate: Option<RuntimeBundleProvenanceConfig>,
}
