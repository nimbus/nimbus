use std::sync::Arc;

use nimbus_auth::normalize_principal_context;
use nimbus_bridge::admission::RuntimeExecutionAdmission;
use nimbus_bridge::{RuntimeHostInvocation, RuntimeHostScope};
use nimbus_core::{Result, TenantId};
use nimbus_engine::Engine;
use nimbus_runtime::{InvocationAuth, InvocationKind, InvocationRequest};
use nimbus_services::RuntimeServiceRegistry;
use nimbus_tenant::{
    RuntimeIsolationTier, TenantIsolationContext, TenantIsolationMode,
    admit_runtime_invocation_decision,
};
use serde_json::Value;

use super::response::{CloudFunctionsHttpResponseParts, build_http_response_parts};
use crate::{
    CloudFunctionsHostBridge, CloudFunctionsRegistry, CloudFunctionsRuntimeInvocation,
    CloudFunctionsRuntimeInvoker,
};

#[derive(Clone)]
pub struct CloudFunctionsRuntimeContext {
    engine: Arc<Engine>,
    runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    tenant_isolation_mode: TenantIsolationMode,
    runtime_invoker: Arc<dyn CloudFunctionsRuntimeInvoker>,
}

impl CloudFunctionsRuntimeContext {
    pub fn new(
        engine: Arc<Engine>,
        runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
        tenant_isolation_mode: TenantIsolationMode,
        runtime_invoker: Arc<dyn CloudFunctionsRuntimeInvoker>,
    ) -> Self {
        Self {
            engine,
            runtime_service_registry,
            tenant_isolation_mode,
            runtime_invoker,
        }
    }
}

pub struct CloudFunctionsHttpInvocation {
    pub registry: Arc<CloudFunctionsRegistry>,
    pub deployment_generation: u64,
    pub tenant_id: TenantId,
    pub function_name: String,
    pub args: Value,
    pub auth: Option<InvocationAuth>,
    pub server_request_id: String,
}

pub fn execute_http_target(
    runtime_context: CloudFunctionsRuntimeContext,
    invocation: CloudFunctionsHttpInvocation,
) -> Result<CloudFunctionsHttpResponseParts> {
    let CloudFunctionsHttpInvocation {
        registry,
        deployment_generation,
        tenant_id,
        function_name,
        args,
        auth,
        server_request_id,
    } = invocation;
    let isolation = TenantIsolationContext::application(
        tenant_id.clone(),
        normalize_principal_context(auth.as_ref()),
        "cloud_functions.http_runtime",
    )
    .with_deployment_generation(deployment_generation);
    isolation.ensure_deployment_generation_matches(
        deployment_generation,
        "cloud functions http runtime deployment",
    )?;
    isolation.validate_principal_claim_if_present("cloud functions http tenant")?;
    let bundle = registry.runtime_bundle();
    isolation.ensure_runtime_bundle_matches(&bundle, "cloud functions http runtime bundle")?;
    let services = runtime_context
        .runtime_service_registry
        .snapshot_for_tenant(isolation.tenant_id());
    let runtime_policy = registry.runtime_policy();
    let decision = admit_runtime_invocation_decision(
        &isolation,
        &function_name,
        Some(server_request_id.as_str()),
        &runtime_policy,
        RuntimeIsolationTier::InProcessUntrusted,
        runtime_context.tenant_isolation_mode,
        services.keys().cloned(),
    )?;
    decision.ensure_runtime_bundle_matches(&bundle, "cloud functions http runtime bundle")?;
    RuntimeExecutionAdmission::for_decision(&decision)
        .ensure_in_process_available("cloud functions http runtime invocation")?;
    let request = InvocationRequest {
        kind: InvocationKind::Mutation,
        function_name,
        args,
        page_size: None,
        cursor: None,
        auth: auth.clone(),
        services: services.clone(),
    };
    let bridge = Arc::new(CloudFunctionsHostBridge::build(
        RuntimeHostScope::new(
            runtime_context.engine.clone(),
            registry.runtime_policy(),
            decision.clone(),
        ),
        RuntimeHostInvocation::new(
            normalize_principal_context(auth.as_ref()),
            Some(server_request_id.clone()),
            InvocationKind::Mutation,
        ),
    )?);

    let runtime_response =
        runtime_context
            .runtime_invoker
            .invoke_runtime_bundle(CloudFunctionsRuntimeInvocation {
                runtime_executor: registry.runtime_executor(),
                runtime_policy: registry.runtime_policy(),
                host_bridge: bridge.clone(),
                bundle,
                request,
                tenant_id: decision.tenant_id().clone(),
                server_request_id: Some(server_request_id),
                provenance_gate: registry.runtime_bundle_provenance().cloned(),
            })?;
    let response = build_http_response_parts(runtime_response)?;
    bridge.commit_mutation_execution_unit()?;
    Ok(response)
}
