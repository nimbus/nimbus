use std::sync::Arc;

use axum::response::Response;
use nimbus_core::TenantId;
use nimbus_runtime::{InvocationAuth, InvocationKind, InvocationRequest};
use serde_json::Value;

use super::response::build_http_response;
use crate::adapters::cloud_functions::CloudFunctionsRegistry;
use crate::adapters::cloud_functions::host_bridge::CloudFunctionsHostBridge;
use crate::application_auth::normalize_principal_context;
use crate::execution::errors::runtime_error_to_core;
use crate::execution::invocations::{
    RuntimeBundleInvocationOptions, invoke_runtime_bundle_blocking_with_host,
    next_runtime_server_request_id,
};
use crate::execution::runtime_admission::RuntimeExecutionAdmission;
use crate::runtime_host::{RuntimeHostInvocation, RuntimeHostScope};
use crate::state::{AppError, AppState};
use crate::tenant_isolation::{
    RuntimeIsolationTier, TenantIsolationContext, admit_runtime_invocation_decision,
};

pub(super) fn execute_http_target(
    state: Arc<AppState>,
    registry: Arc<CloudFunctionsRegistry>,
    deployment_generation: u64,
    tenant_id: TenantId,
    function_name: String,
    args: Value,
    auth: Option<InvocationAuth>,
) -> std::result::Result<Response, AppError> {
    let server_request_id = next_runtime_server_request_id("cloud-functions-http");
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
    isolation.ensure_application_principal_tenant_access("cloud functions http tenant")?;
    let bundle = registry.runtime_bundle();
    isolation.ensure_runtime_bundle_matches(&bundle, "cloud functions http runtime bundle")?;
    let services = state
        .runtime_service_registry()
        .snapshot_for_tenant(isolation.tenant_id());
    let runtime_policy = registry.runtime_policy();
    let decision = admit_runtime_invocation_decision(
        &isolation,
        &function_name,
        Some(server_request_id.as_str()),
        &runtime_policy,
        RuntimeIsolationTier::InProcessUntrusted,
        state.tenant_isolation_mode,
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
            state.service.clone(),
            registry.runtime_policy(),
            decision.clone(),
        ),
        RuntimeHostInvocation::new(
            normalize_principal_context(auth.as_ref()),
            Some(server_request_id.clone()),
            InvocationKind::Mutation,
        ),
    )?);

    let runtime_response = invoke_runtime_bundle_blocking_with_host(
        &registry.runtime_executor(),
        registry.runtime_policy(),
        bridge.clone(),
        bundle,
        request,
        RuntimeBundleInvocationOptions::enforcing_policy_limit(
            decision.tenant_id(),
            Some(server_request_id.as_str()),
            None,
        )
        .with_runtime_bundle_provenance_gate(registry.runtime_bundle_provenance()),
    )
    .map_err(runtime_error_to_core)?;
    let response = build_http_response(runtime_response)?;
    bridge.commit_mutation_execution_unit()?;
    Ok(response)
}
