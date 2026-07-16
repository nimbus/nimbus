use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use futures::future::BoxFuture;
use nimbus_auth::{ApplicationAuthError, ApplicationAuthVerifier};
pub(crate) use nimbus_core::{
    CommitEntry, Cursor, DocumentId, Error, Filter, InvocationAuth, Mutation, OrderBy,
    OrderDirection, PaginatedQuery, Query, Schema, TableName, TenantId,
};
use nimbus_provenance::RuntimeBundleProvenanceConfig;
pub(crate) use nimbus_runtime::{
    InvocationRequest, NimbusRuntimeError, RuntimeAdaptiveControllerSettings, RuntimeBackendKind,
    RuntimeBundle, RuntimeCompatibilityTarget, RuntimeExecutionAdapterArtifactDiagnostics,
    RuntimeExecutionAdapterState, RuntimeExecutor, RuntimeHostPressureSource,
    RuntimeHostResourceBudget, RuntimeLimits, RuntimeMetricsSnapshot, RuntimePolicy,
    RuntimeResetCapabilities, RuntimeScalingPlanSet,
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
    ConvexTeamAuthzError, ConvexTenancyConfig, PrincipalTeamRegistry, SiloTeamRegistry, TeamId,
    TenancySpecError,
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
struct ConvexRuntimeLane {
    policy: Arc<RuntimePolicy>,
    executor: Arc<OnceLock<Arc<RuntimeExecutor>>>,
    execution_adapter_state: RuntimeExecutionAdapterState,
    execution_adapter_artifact: RuntimeExecutionAdapterArtifactDiagnostics,
}

#[derive(Debug, Clone)]
pub struct ConvexRuntimeLaneDiagnostics {
    pub lane_name: &'static str,
    pub default_lane: bool,
    pub executor_started: bool,
    pub execution_adapter_state: RuntimeExecutionAdapterState,
    pub execution_adapter_artifact: RuntimeExecutionAdapterArtifactDiagnostics,
    pub limits: RuntimeLimits,
    pub reset_capabilities: RuntimeResetCapabilities,
    pub metrics: RuntimeMetricsSnapshot,
}

impl ConvexRuntimeLane {
    fn from_limits(
        limits: RuntimeLimits,
        execution_adapter_state: RuntimeExecutionAdapterState,
        execution_adapter_artifact: RuntimeExecutionAdapterArtifactDiagnostics,
    ) -> Self {
        Self::from_policy(
            Arc::new(RuntimePolicy::new(limits)),
            execution_adapter_state,
            execution_adapter_artifact,
        )
    }

    fn from_policy(
        policy: Arc<RuntimePolicy>,
        execution_adapter_state: RuntimeExecutionAdapterState,
        execution_adapter_artifact: RuntimeExecutionAdapterArtifactDiagnostics,
    ) -> Self {
        Self {
            policy,
            executor: Arc::new(OnceLock::new()),
            execution_adapter_state,
            execution_adapter_artifact,
        }
    }

    fn with_runtime_host_governor(
        &self,
        budget: RuntimeHostResourceBudget,
        pressure_source: Arc<dyn RuntimeHostPressureSource>,
        adaptive_settings: RuntimeAdaptiveControllerSettings,
    ) -> Self {
        Self::from_policy(
            Arc::new(self.policy.clone_with_host_resource_governor(
                budget,
                pressure_source,
                adaptive_settings,
            )),
            self.execution_adapter_state,
            self.execution_adapter_artifact.clone(),
        )
    }

    fn with_effective_scaling_plans(&self, plans: RuntimeScalingPlanSet) -> Self {
        Self::from_policy(
            Arc::new(self.policy.clone_with_effective_scaling_plans(plans)),
            self.execution_adapter_state,
            self.execution_adapter_artifact.clone(),
        )
    }

    fn policy(&self) -> Arc<RuntimePolicy> {
        self.policy.clone()
    }

    fn executor(&self) -> Option<Arc<RuntimeExecutor>> {
        match self.execution_adapter_state {
            RuntimeExecutionAdapterState::Linked => Some(
                self.executor
                    .get_or_init(|| Arc::new(RuntimeExecutor::new(self.policy.clone())))
                    .clone(),
            ),
            RuntimeExecutionAdapterState::NotLinked => None,
        }
    }

    fn limits(&self) -> &RuntimeLimits {
        self.policy.limits()
    }

    fn diagnostics(
        &self,
        lane_name: &'static str,
        default_lane: bool,
    ) -> ConvexRuntimeLaneDiagnostics {
        ConvexRuntimeLaneDiagnostics {
            lane_name,
            default_lane,
            executor_started: self.executor.get().is_some(),
            execution_adapter_state: self.execution_adapter_state,
            execution_adapter_artifact: self.execution_adapter_artifact.clone(),
            limits: self.policy.limits().clone(),
            reset_capabilities: self.policy.limits().reset_capabilities(),
            metrics: self.policy.metrics_snapshot(),
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
    runtime_lane: ConvexRuntimeLane,
    node20_runtime_lane: ConvexRuntimeLane,
    node22_runtime_lane: ConvexRuntimeLane,
    node24_runtime_lane: ConvexRuntimeLane,
    node26_runtime_lane: ConvexRuntimeLane,
    bun_jsc_runtime_lane: ConvexRuntimeLane,
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

fn convex_default_runtime_lane(base_limits: RuntimeLimits) -> ConvexRuntimeLane {
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
    ConvexRuntimeLane::from_limits(
        limits,
        RuntimeExecutionAdapterState::Linked,
        RuntimeExecutionAdapterArtifactDiagnostics::built_in("v8_builtin"),
    )
}

fn convex_node_runtime_lane(
    base_limits: RuntimeLimits,
    target: RuntimeCompatibilityTarget,
) -> ConvexRuntimeLane {
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
    ConvexRuntimeLane::from_limits(
        limits,
        RuntimeExecutionAdapterState::Linked,
        RuntimeExecutionAdapterArtifactDiagnostics::built_in("v8_builtin"),
    )
}

fn convex_bun_jsc_runtime_lane(base_limits: RuntimeLimits) -> ConvexRuntimeLane {
    let mut limits = RuntimeLimits::application_bun_jsc();
    limits.apply_resource_overrides_from(&base_limits);
    ConvexRuntimeLane::from_limits(
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
