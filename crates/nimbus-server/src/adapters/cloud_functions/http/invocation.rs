use std::sync::Arc;

use axum::response::Response;
use nimbus_cloud_functions::http::{
    CloudFunctionsHttpInvocation, CloudFunctionsRuntimeContext,
    execute_http_target as execute_adapter_http_target,
};
use nimbus_core::TenantId;
use nimbus_engine::Engine;
use nimbus_runtime::InvocationAuth;
use serde_json::Value;

use super::response::build_http_response;
use crate::adapters::cloud_functions::CloudFunctionsRegistry;
use crate::adapters::cloud_functions::ServerCloudFunctionsRuntimeInvoker;
use crate::execution::invocations::next_runtime_server_request_id;
use crate::state::AppError;
use nimbus_services::RuntimeServiceRegistry;
use nimbus_tenant::TenantIsolationMode;

pub(super) struct ServerCloudFunctionsHttpInvocation {
    pub engine: Arc<Engine>,
    pub runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    pub tenant_isolation_mode: TenantIsolationMode,
    pub registry: Arc<CloudFunctionsRegistry>,
    pub deployment_generation: u64,
    pub tenant_id: TenantId,
    pub function_name: String,
    pub args: Value,
    pub auth: Option<InvocationAuth>,
}

pub(super) fn execute_http_target(
    invocation: ServerCloudFunctionsHttpInvocation,
) -> std::result::Result<Response, AppError> {
    let ServerCloudFunctionsHttpInvocation {
        engine,
        runtime_service_registry,
        tenant_isolation_mode,
        registry,
        deployment_generation,
        tenant_id,
        function_name,
        args,
        auth,
    } = invocation;
    let server_request_id = next_runtime_server_request_id("cloud-functions-http");
    let runtime_context = CloudFunctionsRuntimeContext::new(
        engine,
        runtime_service_registry,
        tenant_isolation_mode,
        Arc::new(ServerCloudFunctionsRuntimeInvoker),
    );
    let response = execute_adapter_http_target(
        runtime_context,
        CloudFunctionsHttpInvocation {
            registry,
            deployment_generation,
            tenant_id,
            function_name,
            args,
            auth,
            server_request_id,
        },
    )?;
    build_http_response(response)
}
