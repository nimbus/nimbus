use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use nimbus_auth::{ApplicationAuthError, ApplicationAuthVerifier};
pub(crate) use nimbus_core::{
    CommitEntry, Cursor, DocumentId, Error, Filter, InvocationAuth, Mutation, OrderBy,
    OrderDirection, PaginatedQuery, Query, Schema, TableName, TenantId,
};
use nimbus_provenance::RuntimeBundleProvenanceConfig;
pub(crate) use nimbus_runtime::{
    InvocationRequest, NimbusRuntimeError, RuntimeBackendKind, RuntimeBundle,
    RuntimeCompatibilityTarget, RuntimeExecutionAdapterArtifactDiagnostics,
    RuntimeExecutionAdapterState, RuntimeLimits, RuntimeResetCapabilities,
};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::Value;

mod auth;
mod document_identity;
mod error;
pub mod host_bridge;
mod manifest;
mod registry;
mod requests;
mod silo_auth;
pub mod subscriptions;
mod templates;
pub mod tenancy;

pub use document_identity::{
    document_to_convex_json, documents_to_convex_json, encode_convex_document_id,
    page_to_convex_json, replace_id_in_value, resolve_convex_document_id,
};
pub use error::{ConvexCommitErrorVocabulary, convex_commit_error_vocabulary};
pub use host_bridge::{
    ConvexHostCallFamily, ConvexHostCallOperation, ConvexHostCallRequest, ConvexHttpResponseParts,
    ConvexRuntimeActionPayload, ConvexRuntimeDbDeletePayload, ConvexRuntimeDbGetPayload,
    ConvexRuntimeDbInsertPayload, ConvexRuntimeDbPatchPayload, ConvexRuntimeFunctionCallPayload,
    ConvexRuntimeHttpRouteInvokePayload, ConvexRuntimeMutationPayload,
    ConvexRuntimePaginatedQueryPayload, ConvexRuntimeQueryBuilderState, ConvexRuntimeQueryBuilders,
    ConvexRuntimeQueryFilterPayload, ConvexRuntimeQueryOrderPayload,
    ConvexRuntimeQueryPaginatePayload, ConvexRuntimeQueryPayload, ConvexRuntimeQueryStartPayload,
    ConvexRuntimeQueryTakePayload, ConvexRuntimeQueryTerminalPayload,
    ConvexRuntimeQueryWithIndexPayload, ConvexRuntimeResponseEnvelope,
    ConvexRuntimeSchedulerCancelPayload, ConvexRuntimeSchedulerRunAfterPayload,
    ConvexRuntimeSchedulerRunAtPayload, ConvexRuntimeServiceLookupPayload,
    convex_host_operation_name, runtime_host_payload_value, synthesize_runtime_paginate_cursor,
};
pub use manifest::{
    ConvexFunctionDefinition, ConvexFunctionKind, ConvexFunctionVisibility, ConvexHttpActionPlan,
    ConvexHttpMethod, ConvexHttpResponseKind, ConvexHttpResponseTemplate,
    ConvexHttpRouteDefinition, ConvexRuntimeEnvironment, ConvexRuntimePackageResolution,
    ConvexRuntimeSelection,
};
pub use registry::{
    ConvexFunctionDeploySummary, ConvexHttpRouteDeploySummary, ConvexRegistryDeploySummary,
    validate_runtime_http_route,
};
pub use requests::{
    ConvexAction, ConvexActionRequest, ConvexExecutableAction, ConvexExecutableMutation,
    ConvexExecutableQuery, ConvexFunctionCallCommand, ConvexMutationRequest,
    ConvexNamedPaginatedQueryRequest, ConvexNamedRequest, ConvexPaginatedQueryRequest,
    ConvexQueryRequest, ConvexReadCommand, ConvexScheduleAfterRequest, ConvexScheduleAtRequest,
    ConvexScheduledCommand,
};
pub use silo_auth::ConvexSiloAuthRegistry;
pub use subscriptions::{
    ConvexClientMessage, ConvexRuntimeSubscriptionSetup, ConvexSubscriptionEvent,
    ConvexSubscriptionTransform, ConvexSubscriptionTransforms, activate_transform,
    apply_builtin_transform, clear_pending_transform, is_scalar_filter_value,
    remove_subscription_transform, resolve_subscription_transform, set_pending_transform,
    should_replace_lower_bound, should_replace_upper_bound, subscription_plan_for_named_query,
    update_runtime_transform_read_set,
};
pub use templates::{
    empty_args, method_name, normalize_http_request_path, parse_job_id, resolve_http_template,
    resolve_template,
};
pub use tenancy::{
    ConvexTeamAuthzError, ConvexTenancyConfig, SiloTeamRegistry, TeamId, TenancySpecError,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvexHttpRequestContext {
    pub method: String,
    pub url: String,
    pub pathname: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body_bytes: Vec<u8>,
    pub body_text: String,
}

#[derive(Debug, Clone)]
struct RuntimeExecutionRequirements {
    limits: RuntimeLimits,
    execution_adapter_state: RuntimeExecutionAdapterState,
    execution_adapter_artifact: RuntimeExecutionAdapterArtifactDiagnostics,
}

#[derive(Debug, Clone)]
pub struct RuntimeExecutionRequirementsDiagnostics {
    pub lane_name: &'static str,
    pub default_lane: bool,
    pub execution_adapter_state: RuntimeExecutionAdapterState,
    pub execution_adapter_artifact: RuntimeExecutionAdapterArtifactDiagnostics,
    pub limits: RuntimeLimits,
    pub reset_capabilities: RuntimeResetCapabilities,
}

impl RuntimeExecutionRequirements {
    fn from_limits(
        limits: RuntimeLimits,
        execution_adapter_state: RuntimeExecutionAdapterState,
        execution_adapter_artifact: RuntimeExecutionAdapterArtifactDiagnostics,
    ) -> Self {
        Self {
            limits,
            execution_adapter_state,
            execution_adapter_artifact,
        }
    }

    fn limits(&self) -> &RuntimeLimits {
        &self.limits
    }

    fn diagnostics(
        &self,
        lane_name: &'static str,
        default_lane: bool,
    ) -> RuntimeExecutionRequirementsDiagnostics {
        RuntimeExecutionRequirementsDiagnostics {
            lane_name,
            default_lane,
            execution_adapter_state: self.execution_adapter_state,
            execution_adapter_artifact: self.execution_adapter_artifact.clone(),
            limits: self.limits.clone(),
            reset_capabilities: self.limits.reset_capabilities(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConvexRegistry {
    functions: HashMap<String, ConvexFunctionDefinition>,
    http_routes: Vec<ConvexHttpRouteDefinition>,
    schema: Option<Schema>,
    runtime_bundle: Option<RuntimeBundle>,
    bun_jsc_runtime_bundle: Option<RuntimeBundle>,
    artifact_guard: Option<Arc<tempfile::TempDir>>,
    auth_verifier: Arc<auth::ConvexAuthVerifier>,
    runtime_lane: RuntimeExecutionRequirements,
    node20_runtime_lane: RuntimeExecutionRequirements,
    node22_runtime_lane: RuntimeExecutionRequirements,
    node24_runtime_lane: RuntimeExecutionRequirements,
    node26_runtime_lane: RuntimeExecutionRequirements,
    bun_jsc_runtime_lane: RuntimeExecutionRequirements,
    runtime_bundle_provenance: Option<RuntimeBundleProvenanceConfig>,
}

impl Default for ConvexRegistry {
    fn default() -> Self {
        let runtime_lane = convex_default_runtime_lane(RuntimeLimits::default());
        let node20_runtime_lane =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node20);
        let node22_runtime_lane =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node22);
        let node24_runtime_lane =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node24);
        let node26_runtime_lane =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node26);
        let bun_jsc_runtime_lane = convex_bun_jsc_runtime_lane(RuntimeLimits::default());
        Self {
            functions: HashMap::new(),
            http_routes: Vec::new(),
            schema: None,
            runtime_bundle: None,
            bun_jsc_runtime_bundle: None,
            artifact_guard: None,
            auth_verifier: Arc::new(auth::ConvexAuthVerifier::empty()),
            runtime_lane,
            node20_runtime_lane,
            node22_runtime_lane,
            node24_runtime_lane,
            node26_runtime_lane,
            bun_jsc_runtime_lane,
            runtime_bundle_provenance: None,
        }
    }
}

impl ConvexRegistry {
    pub fn function_definition(&self, name: &str) -> Option<&ConvexFunctionDefinition> {
        self.functions.get(name)
    }
}

fn convex_default_runtime_lane(base_limits: RuntimeLimits) -> RuntimeExecutionRequirements {
    let mut limits = RuntimeLimits::default();
    if matches!(base_limits.backend_kind, RuntimeBackendKind::V8) {
        limits = base_limits;
    } else {
        limits.apply_resource_overrides_from(&base_limits);
    }
    // The Convex default runtime carries the upstream guest-semantics
    // contract (seeded Math.random, frozen invocation clock, deploy-pinned
    // timeOrigin, fetch-in-actions-only, process.env + node:async_hooks)
    // regardless of what base limits the server passed in. The node lanes
    // stay on Host semantics: the upstream Node runtime is exempt from these
    // rules and only runs actions.
    limits.guest_semantics = nimbus_runtime::RuntimeGuestSemantics::ConvexDefault;
    RuntimeExecutionRequirements::from_limits(
        limits,
        RuntimeExecutionAdapterState::Linked,
        RuntimeExecutionAdapterArtifactDiagnostics::built_in("v8_builtin"),
    )
}

fn convex_node_runtime_lane(
    base_limits: RuntimeLimits,
    target: RuntimeCompatibilityTarget,
) -> RuntimeExecutionRequirements {
    let use_full_override = matches!(base_limits.backend_kind, RuntimeBackendKind::V8)
        && base_limits.compatibility_target == target;
    let mut limits = if use_full_override {
        base_limits.clone()
    } else {
        RuntimeLimits::application_node(target)
    };
    if !use_full_override {
        limits.apply_resource_overrides_from(&base_limits);
    }
    RuntimeExecutionRequirements::from_limits(
        limits,
        RuntimeExecutionAdapterState::Linked,
        RuntimeExecutionAdapterArtifactDiagnostics::built_in("v8_builtin"),
    )
}

fn convex_bun_jsc_runtime_lane(base_limits: RuntimeLimits) -> RuntimeExecutionRequirements {
    let mut limits = RuntimeLimits::application_bun_jsc();
    limits.apply_resource_overrides_from(&base_limits);
    RuntimeExecutionRequirements::from_limits(
        limits,
        nimbus_runtime::bun_jsc_execution_adapter_state(),
        nimbus_runtime::bun_jsc_adapter_artifact_diagnostics(),
    )
}

impl ApplicationAuthVerifier for ConvexRegistry {
    fn verify_bearer_token<'a>(
        &'a self,
        token: &'a str,
    ) -> BoxFuture<'a, Result<InvocationAuth, ApplicationAuthError>> {
        Box::pin(async move { ConvexRegistry::verify_bearer_token(self, token).await })
    }
}
