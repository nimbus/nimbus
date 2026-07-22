#![expect(
    dead_code,
    reason = "REC1 defines the execution-plan vocabulary before REC3 wires it into scheduler admission"
)]

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;

use crate::RuntimeInvocationContext;
use crate::backends::v8::V8RuntimeConstructionMode;
use crate::limits::{
    RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
    RuntimeBackendTrustTier, RuntimeBundleContentKind, RuntimeCompatibilityTarget,
    RuntimeExecutionModel, RuntimeGrants, RuntimeHostWorkClass, RuntimeJavaScriptEvaluationFormat,
    RuntimeLanguage, RuntimeMemoryEnforcement, RuntimeMode, RuntimeNodeFullRealmReusePolicy,
    RuntimePolicy, RuntimePoolKind, RuntimePreset, RuntimeProfile, RuntimeRoutingAffinity,
    RuntimeTenantBudget,
};
use crate::runtime::{InvocationKind, InvocationRequest, RuntimeBundle, RuntimeComponentWorld};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeEffectClass {
    Unknown,
    PureLocalRead,
    ObservableRead,
    Write,
    Scheduler,
    ServiceExternal,
    NestedRuntime,
    Extension,
    HttpRoute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeSideChannelPosture {
    ProvenSafeForCooperativeReuse,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CooperativeIneligibilityReason {
    EffectfulKind,
    UnknownEffect,
    ObservableRead,
    WriteHostOperation,
    SchedulerOperation,
    ServiceOrExternalOperation,
    NestedRuntimeOperation,
    ExtensionOperation,
    HttpRouteOperation,
    SideChannelPostureMissing,
    UnsupportedRuntimeSurface,
    NodeFullUnproven,
    CpuHeavy,
    OperatorDisabled,
    PoolAuthorityMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CooperativeEligibility {
    Eligible,
    Ineligible(CooperativeIneligibilityReason),
}

impl CooperativeEligibility {
    pub(crate) const fn is_eligible(self) -> bool {
        matches!(self, Self::Eligible)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct RuntimeObservedEffectViolation {
    pub(crate) planned_effect_class: RuntimeEffectClass,
    pub(crate) observed_effect_class: RuntimeEffectClass,
    pub(crate) reason: CooperativeIneligibilityReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeSchedulingClass {
    LatencySensitiveRead,
    CpuHeavy,
    IoHeavy,
    Effectful,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimePoolAuthorityKey {
    Exact(Box<RuntimePoolAuthorityFacts>),
    Missing(RuntimePoolAuthorityMissingReason),
}

impl RuntimePoolAuthorityKey {
    pub(crate) fn exact(facts: RuntimePoolAuthorityFacts) -> Self {
        Self::Exact(Box::new(facts))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RuntimePoolAuthorityFacts {
    runtime_profile: Option<RuntimeProfile>,
    exact_service_grants: Vec<String>,
    strict_reuse: Option<RuntimePoolStrictAuthorityFacts>,
}

impl RuntimePoolAuthorityFacts {
    pub(crate) fn new(runtime_profile: RuntimeProfile, exact_service_grants: Vec<String>) -> Self {
        Self {
            runtime_profile: Some(runtime_profile),
            exact_service_grants,
            strict_reuse: None,
        }
    }

    pub(crate) fn for_realm_reuse(
        runtime_profile: RuntimeProfile,
        policy: &RuntimePolicy,
        bundle: &RuntimeBundle,
        construction_mode: V8RuntimeConstructionMode,
    ) -> crate::Result<Self> {
        Self::for_retained_state(runtime_profile, policy, bundle, construction_mode.as_str())
    }

    pub(crate) fn for_retained_state(
        runtime_profile: RuntimeProfile,
        policy: &RuntimePolicy,
        bundle: &RuntimeBundle,
        construction_shape: &'static str,
    ) -> crate::Result<Self> {
        let limits = policy.limits();
        Ok(Self {
            runtime_profile: Some(runtime_profile),
            exact_service_grants: limits.grants.sorted_service_grants(),
            strict_reuse: Some(RuntimePoolStrictAuthorityFacts::from_parts(
                limits,
                bundle,
                construction_shape,
            )?),
        })
    }

    pub(crate) fn for_profileless_retained_state(
        policy: &RuntimePolicy,
        bundle: &RuntimeBundle,
        construction_shape: &'static str,
    ) -> crate::Result<Self> {
        let limits = policy.limits();
        Ok(Self {
            runtime_profile: None,
            exact_service_grants: limits.grants.sorted_service_grants(),
            strict_reuse: Some(RuntimePoolStrictAuthorityFacts::from_parts(
                limits,
                bundle,
                construction_shape,
            )?),
        })
    }

    pub(crate) const fn runtime_profile(&self) -> Option<RuntimeProfile> {
        self.runtime_profile
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RuntimePoolStrictAuthorityFacts {
    bundle: RuntimePoolBundleAuthorityFacts,
    backend_kind: RuntimeBackendKind,
    backend_trust_tier: RuntimeBackendTrustTier,
    backend_lockdown_profile: RuntimeBackendLockdownProfile,
    backend_lifecycle_policy: RuntimeBackendLifecyclePolicy,
    bundle_content_kind: RuntimeBundleContentKind,
    javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat,
    compatibility_target: RuntimeCompatibilityTarget,
    node_conditions: Vec<String>,
    execution_model: RuntimeExecutionModel,
    mode: RuntimeMode,
    language: RuntimeLanguage,
    preset: RuntimePreset,
    grants: RuntimeGrantsAuthorityFacts,
    service_capability_enabled: bool,
    runtime_pool_kind: RuntimePoolKind,
    node_full_realm_reuse_policy: RuntimeNodeFullRealmReusePolicy,
    memory_enforcement: RuntimeMemoryEnforcement,
    routing_affinity: RuntimeRoutingAffinity,
    max_heap_mb: usize,
    initial_heap_mb: usize,
    execution_timeout: Duration,
    system_timeout: Duration,
    max_nested_runtime_invocations: usize,
    construction_mode: &'static str,
    host_bridge_contract: RuntimeHostBridgeAuthorityContract,
}

impl RuntimePoolStrictAuthorityFacts {
    fn from_parts(
        limits: &crate::RuntimeLimits,
        bundle: &RuntimeBundle,
        construction_shape: &'static str,
    ) -> crate::Result<Self> {
        Ok(Self {
            bundle: RuntimePoolBundleAuthorityFacts::for_bundle(bundle)?,
            backend_kind: limits.backend_kind,
            backend_trust_tier: limits.backend_trust_tier,
            backend_lockdown_profile: limits.backend_lockdown_profile,
            backend_lifecycle_policy: limits.backend_lifecycle_policy,
            bundle_content_kind: limits.bundle_content_kind,
            javascript_evaluation_format: limits.javascript_evaluation_format,
            compatibility_target: limits.compatibility_target,
            node_conditions: limits.node_conditions.clone(),
            execution_model: limits.execution_model,
            mode: limits.mode,
            language: limits.language,
            preset: limits.preset,
            grants: RuntimeGrantsAuthorityFacts::from_grants(&limits.grants),
            service_capability_enabled: limits.service_capability_enabled,
            runtime_pool_kind: limits.runtime_pool_kind,
            node_full_realm_reuse_policy: limits.node_full_realm_reuse_policy,
            memory_enforcement: limits.memory_enforcement,
            routing_affinity: limits.routing_affinity,
            max_heap_mb: limits.max_heap_mb,
            initial_heap_mb: limits.initial_heap_mb,
            execution_timeout: limits.execution_timeout,
            system_timeout: limits.system_timeout,
            max_nested_runtime_invocations: limits.max_nested_runtime_invocations,
            construction_mode: construction_shape,
            host_bridge_contract: RuntimeHostBridgeAuthorityContract::ReboundPerInvocation,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RuntimePoolBundleAuthorityFacts {
    tenant_label: Option<String>,
    content_kind: RuntimeBundleContentKind,
    target_world: Option<RuntimeComponentWorld>,
    entrypoint_kind: &'static str,
    entrypoint: PathBuf,
    module_root: PathBuf,
    expected_sha256: Option<String>,
    // Distinguishes two deploys with byte-identical content but different
    // per-deploy nonces so cooperative reuse never hands back a runtime seeded
    // under the prior deploy (mirrors the warm-pool identity fix, EX10R3.2).
    deploy_nonce: Option<String>,
}

impl RuntimePoolBundleAuthorityFacts {
    fn for_bundle(bundle: &RuntimeBundle) -> crate::Result<Self> {
        let identity = bundle.identity();
        Ok(Self {
            tenant_label: identity.tenant_label().map(str::to_owned),
            content_kind: identity.content_kind(),
            target_world: identity.target_world(),
            entrypoint_kind: match bundle.entrypoint_kind() {
                crate::runtime::RuntimeBundleEntrypointKind::Main => "main",
                crate::runtime::RuntimeBundleEntrypointKind::Side => "side",
            },
            entrypoint: identity.entrypoint().to_path_buf(),
            module_root: bundle.module_root()?,
            expected_sha256: identity.expected_sha256().map(str::to_owned),
            deploy_nonce: identity.deploy_nonce().map(str::to_owned),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct RuntimeGrantsAuthorityFacts {
    read: Vec<String>,
    write: Vec<String>,
    net_connect: Vec<String>,
    net_listen: Vec<String>,
    env_read: Vec<String>,
    env_write: Vec<String>,
    secret: Vec<String>,
    identity: Vec<String>,
    service: Vec<String>,
    run: Vec<String>,
    sys: Vec<String>,
    ffi: Vec<String>,
    worker: Vec<String>,
    tool: Vec<String>,
}

impl RuntimeGrantsAuthorityFacts {
    fn from_grants(grants: &RuntimeGrants) -> Self {
        Self {
            read: sorted_deduped(&grants.read),
            write: sorted_deduped(&grants.write),
            net_connect: sorted_deduped(&grants.net_connect),
            net_listen: sorted_deduped(&grants.net_listen),
            env_read: sorted_deduped(&grants.env_read),
            env_write: sorted_deduped(&grants.env_write),
            secret: sorted_deduped(&grants.secret),
            identity: sorted_deduped(&grants.identity),
            service: sorted_deduped(&grants.service),
            run: sorted_deduped(&grants.run),
            sys: sorted_deduped(&grants.sys),
            ffi: sorted_deduped(&grants.ffi),
            worker: sorted_deduped(&grants.worker),
            tool: sorted_deduped(&grants.tool),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RuntimeHostBridgeAuthorityContract {
    ReboundPerInvocation,
}

fn sorted_deduped(values: &[String]) -> Vec<String> {
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    values
}

impl RuntimePoolAuthorityKey {
    pub(crate) const fn runtime_profile(&self) -> Option<RuntimeProfile> {
        match self {
            Self::Exact(facts) => facts.runtime_profile(),
            Self::Missing(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimePoolAuthorityMissingReason {
    RuntimeProfile,
    TenantOrPrincipal,
    BundleIdentity,
    PermissionProfile,
    HostSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeAdmissionOutcome {
    NotEvaluated,
    Admit,
    Queue(RuntimeAdmissionQueueReason),
    Shed(RuntimeAdmissionShedReason),
    Reject(RuntimeAdmissionRejectReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeAdmissionQueueReason {
    TenantQuota,
    HostPressure,
    QueueCapacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeAdmissionShedReason {
    HostPressure,
    BestEffortOverload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeAdmissionRejectReason {
    TenantQueueLimit,
    ClassificationConflict,
    EffectViolation,
    Cancelled,
}

#[must_use]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RuntimeExecutionPlan {
    function_kind: InvocationKind,
    runtime_profile: Option<RuntimeProfile>,
    effect_class: RuntimeEffectClass,
    cooperative_eligibility: CooperativeEligibility,
    pool_authority_key: RuntimePoolAuthorityKey,
    scheduling_class: RuntimeSchedulingClass,
    tenant_budget: RuntimeTenantBudget,
    host_work_class: RuntimeHostWorkClass,
    admission_outcome: RuntimeAdmissionOutcome,
}

impl RuntimeExecutionPlan {
    pub(crate) fn classify(input: RuntimeExecutionPlanInput) -> Self {
        let cooperative_eligibility = cooperative_eligibility_for(&input);
        Self {
            function_kind: input.function_kind,
            runtime_profile: input.runtime_profile,
            effect_class: input.effect_class,
            cooperative_eligibility,
            pool_authority_key: input.pool_authority_key,
            scheduling_class: input.scheduling_class,
            tenant_budget: input.tenant_budget,
            host_work_class: input.host_work_class,
            admission_outcome: RuntimeAdmissionOutcome::NotEvaluated,
        }
    }

    pub(crate) const fn cooperative_eligibility(&self) -> CooperativeEligibility {
        self.cooperative_eligibility
    }

    pub(crate) const fn pool_authority_key(&self) -> &RuntimePoolAuthorityKey {
        &self.pool_authority_key
    }

    pub(crate) fn for_invocation(
        policy: &RuntimePolicy,
        request: &InvocationRequest,
        context: &RuntimeInvocationContext,
    ) -> Self {
        let runtime_profile = policy.runtime_profile();
        let input = RuntimeExecutionPlanInput {
            function_kind: request.kind.clone(),
            runtime_profile,
            effect_class: effect_class_for_invocation_kind(&request.kind),
            side_channel_posture: side_channel_posture_for_invocation(
                policy,
                request,
                runtime_profile,
            ),
            pool_authority_key: pool_authority_key_for_invocation(policy, runtime_profile, context),
            node_full_realm_reuse_policy: policy.limits().node_full_realm_reuse_policy,
            scheduling_class: scheduling_class_for_invocation_kind(&request.kind),
            tenant_budget: policy.tenant_budget(),
            host_work_class: host_work_class_for_context(context),
            operator_enabled: true,
        };
        Self::classify(input)
    }

    pub(crate) fn for_realm_lease_invocation(
        policy: &RuntimePolicy,
        bundle: &RuntimeBundle,
        request: &InvocationRequest,
        context: &RuntimeInvocationContext,
        construction_mode: V8RuntimeConstructionMode,
    ) -> crate::Result<Self> {
        let runtime_profile = policy.runtime_profile();
        let input = RuntimeExecutionPlanInput {
            function_kind: request.kind.clone(),
            runtime_profile,
            effect_class: effect_class_for_invocation_kind(&request.kind),
            side_channel_posture: side_channel_posture_for_invocation(
                policy,
                request,
                runtime_profile,
            ),
            pool_authority_key: pool_authority_key_for_realm_reuse(
                policy,
                runtime_profile,
                context,
                bundle,
                construction_mode,
            )?,
            node_full_realm_reuse_policy: policy.limits().node_full_realm_reuse_policy,
            scheduling_class: scheduling_class_for_invocation_kind(&request.kind),
            tenant_budget: policy.tenant_budget(),
            host_work_class: host_work_class_for_context(context),
            operator_enabled: true,
        };
        Ok(Self::classify(input))
    }

    pub(crate) const fn permits_cooperative_scheduler_admission(&self) -> bool {
        self.cooperative_eligibility.is_eligible()
            && matches!(
                self.scheduling_class,
                RuntimeSchedulingClass::LatencySensitiveRead
            )
    }

    pub(crate) const fn observed_effect_violation(
        &self,
        observed_effect_class: RuntimeEffectClass,
    ) -> Option<RuntimeObservedEffectViolation> {
        if !self.cooperative_eligibility.is_eligible() {
            return None;
        }
        let Some(reason) =
            observed_effect_violation_reason(self.effect_class, observed_effect_class)
        else {
            return None;
        };
        Some(RuntimeObservedEffectViolation {
            planned_effect_class: self.effect_class,
            observed_effect_class,
            reason,
        })
    }

    #[cfg(test)]
    pub(crate) const fn admission_outcome(&self) -> RuntimeAdmissionOutcome {
        self.admission_outcome
    }

    #[cfg(test)]
    pub(crate) const fn runtime_profile(&self) -> Option<RuntimeProfile> {
        self.runtime_profile
    }

    pub(crate) const fn host_work_class(&self) -> RuntimeHostWorkClass {
        self.host_work_class
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeExecutionPlanInput {
    pub(crate) function_kind: InvocationKind,
    pub(crate) runtime_profile: Option<RuntimeProfile>,
    pub(crate) effect_class: RuntimeEffectClass,
    pub(crate) side_channel_posture: RuntimeSideChannelPosture,
    pub(crate) pool_authority_key: RuntimePoolAuthorityKey,
    pub(crate) node_full_realm_reuse_policy: RuntimeNodeFullRealmReusePolicy,
    pub(crate) scheduling_class: RuntimeSchedulingClass,
    pub(crate) tenant_budget: RuntimeTenantBudget,
    pub(crate) host_work_class: RuntimeHostWorkClass,
    pub(crate) operator_enabled: bool,
}

fn cooperative_eligibility_for(input: &RuntimeExecutionPlanInput) -> CooperativeEligibility {
    if !input.operator_enabled {
        return CooperativeEligibility::Ineligible(
            CooperativeIneligibilityReason::OperatorDisabled,
        );
    }
    if !input.function_kind.is_convex_read_semantic_candidate() {
        return CooperativeEligibility::Ineligible(CooperativeIneligibilityReason::EffectfulKind);
    }
    let Some(runtime_profile) = input.runtime_profile else {
        return CooperativeEligibility::Ineligible(
            CooperativeIneligibilityReason::UnsupportedRuntimeSurface,
        );
    };
    if matches!(runtime_profile, RuntimeProfile::NodeFull)
        && !matches!(
            input.node_full_realm_reuse_policy,
            RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority
        )
    {
        return CooperativeEligibility::Ineligible(
            CooperativeIneligibilityReason::NodeFullUnproven,
        );
    }
    if matches!(
        input.pool_authority_key,
        RuntimePoolAuthorityKey::Missing(_)
    ) {
        return CooperativeEligibility::Ineligible(
            CooperativeIneligibilityReason::PoolAuthorityMissing,
        );
    }
    if !matches!(
        input.side_channel_posture,
        RuntimeSideChannelPosture::ProvenSafeForCooperativeReuse
    ) {
        return CooperativeEligibility::Ineligible(
            CooperativeIneligibilityReason::SideChannelPostureMissing,
        );
    }
    if matches!(input.scheduling_class, RuntimeSchedulingClass::CpuHeavy) {
        return CooperativeEligibility::Ineligible(CooperativeIneligibilityReason::CpuHeavy);
    }
    match cooperative_ineligibility_reason_for_effect_class(input.effect_class) {
        None => CooperativeEligibility::Eligible,
        Some(reason) => CooperativeEligibility::Ineligible(reason),
    }
}

const fn cooperative_ineligibility_reason_for_effect_class(
    effect_class: RuntimeEffectClass,
) -> Option<CooperativeIneligibilityReason> {
    match effect_class {
        RuntimeEffectClass::PureLocalRead | RuntimeEffectClass::ObservableRead => None,
        RuntimeEffectClass::Unknown => Some(CooperativeIneligibilityReason::UnknownEffect),
        RuntimeEffectClass::Write => Some(CooperativeIneligibilityReason::WriteHostOperation),
        RuntimeEffectClass::Scheduler => Some(CooperativeIneligibilityReason::SchedulerOperation),
        RuntimeEffectClass::ServiceExternal => {
            Some(CooperativeIneligibilityReason::ServiceOrExternalOperation)
        }
        RuntimeEffectClass::NestedRuntime => {
            Some(CooperativeIneligibilityReason::NestedRuntimeOperation)
        }
        RuntimeEffectClass::Extension => Some(CooperativeIneligibilityReason::ExtensionOperation),
        RuntimeEffectClass::HttpRoute => Some(CooperativeIneligibilityReason::HttpRouteOperation),
    }
}

const fn observed_effect_violation_reason(
    planned_effect_class: RuntimeEffectClass,
    observed_effect_class: RuntimeEffectClass,
) -> Option<CooperativeIneligibilityReason> {
    match (planned_effect_class, observed_effect_class) {
        (_, RuntimeEffectClass::PureLocalRead)
        | (RuntimeEffectClass::ObservableRead, RuntimeEffectClass::ObservableRead) => None,
        (_, RuntimeEffectClass::ObservableRead) => {
            Some(CooperativeIneligibilityReason::ObservableRead)
        }
        (_, effect_class) => cooperative_ineligibility_reason_for_effect_class(effect_class),
    }
}

fn effect_class_for_invocation_kind(kind: &InvocationKind) -> RuntimeEffectClass {
    match kind {
        InvocationKind::Query | InvocationKind::PaginatedQuery => {
            RuntimeEffectClass::ObservableRead
        }
        InvocationKind::Mutation => RuntimeEffectClass::Write,
        InvocationKind::Action | InvocationKind::CloudflareWorkerFetch => {
            RuntimeEffectClass::ServiceExternal
        }
    }
}

fn side_channel_posture_for_invocation(
    policy: &RuntimePolicy,
    request: &InvocationRequest,
    runtime_profile: Option<RuntimeProfile>,
) -> RuntimeSideChannelPosture {
    let web_lean_reuse_safe = matches!(runtime_profile, Some(RuntimeProfile::WebLean))
        && !policy.limits().grants.has_service_grants()
        && request.services.is_empty();
    let node_full_reuse_safe = matches!(runtime_profile, Some(RuntimeProfile::NodeFull))
        && matches!(
            policy.limits().runtime_pool_kind,
            RuntimePoolKind::WarmContextRecycle
        )
        && matches!(
            policy.limits().node_full_realm_reuse_policy,
            RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority
        )
        && policy.limits().grants.permits_same_process_realm_reuse()
        && request.services.is_empty();
    if web_lean_reuse_safe || node_full_reuse_safe {
        RuntimeSideChannelPosture::ProvenSafeForCooperativeReuse
    } else {
        RuntimeSideChannelPosture::Unknown
    }
}

fn pool_authority_key_for_invocation(
    policy: &RuntimePolicy,
    runtime_profile: Option<RuntimeProfile>,
    context: &RuntimeInvocationContext,
) -> RuntimePoolAuthorityKey {
    let Some(runtime_profile) = runtime_profile else {
        return RuntimePoolAuthorityKey::Missing(RuntimePoolAuthorityMissingReason::RuntimeProfile);
    };
    if context.tenant_label.is_none() {
        return RuntimePoolAuthorityKey::Missing(
            RuntimePoolAuthorityMissingReason::TenantOrPrincipal,
        );
    }
    RuntimePoolAuthorityKey::exact(RuntimePoolAuthorityFacts::new(
        runtime_profile,
        policy.limits().grants.sorted_service_grants(),
    ))
}

fn pool_authority_key_for_realm_reuse(
    policy: &RuntimePolicy,
    runtime_profile: Option<RuntimeProfile>,
    context: &RuntimeInvocationContext,
    bundle: &RuntimeBundle,
    construction_mode: V8RuntimeConstructionMode,
) -> crate::Result<RuntimePoolAuthorityKey> {
    let Some(runtime_profile) = runtime_profile else {
        return Ok(RuntimePoolAuthorityKey::Missing(
            RuntimePoolAuthorityMissingReason::RuntimeProfile,
        ));
    };
    if context.tenant_label.is_none() {
        return Ok(RuntimePoolAuthorityKey::Missing(
            RuntimePoolAuthorityMissingReason::TenantOrPrincipal,
        ));
    }
    Ok(RuntimePoolAuthorityKey::exact(
        RuntimePoolAuthorityFacts::for_realm_reuse(
            runtime_profile,
            policy,
            bundle,
            construction_mode,
        )?,
    ))
}

fn scheduling_class_for_invocation_kind(kind: &InvocationKind) -> RuntimeSchedulingClass {
    match kind {
        InvocationKind::Query | InvocationKind::PaginatedQuery => {
            RuntimeSchedulingClass::LatencySensitiveRead
        }
        InvocationKind::Mutation
        | InvocationKind::Action
        | InvocationKind::CloudflareWorkerFetch => RuntimeSchedulingClass::Effectful,
    }
}

fn host_work_class_for_context(context: &RuntimeInvocationContext) -> RuntimeHostWorkClass {
    if context.bypasses_concurrency_limit() {
        RuntimeHostWorkClass::Guaranteed
    } else {
        RuntimeHostWorkClass::Burstable
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::Value;

    use crate::RuntimeInvocationContext;
    use crate::backends::v8::V8RuntimeConstructionMode;
    use crate::limits::{
        RuntimeLimits, RuntimeMemoryEnforcement, RuntimePolicy, RuntimeTenantBudget,
    };
    use crate::runtime::{InvocationRequest, RuntimeBundle};

    use super::*;

    type RuntimeLimitsCase = (&'static str, fn(&mut RuntimeLimits));

    fn budget() -> RuntimeTenantBudget {
        RuntimeTenantBudget {
            max_active_runtime_slots: 2,
            max_in_flight_top_level_invocations: 4,
            max_queued_top_level_invocations: 8,
            max_worker_thread_slots: 0,
            max_heap_mb_per_runtime: 128,
            memory_enforcement: RuntimeMemoryEnforcement::V8IsolateHeapLimit,
            max_active_heap_mb: 256,
            execution_timeout: Duration::from_secs(1),
            system_timeout: Duration::from_secs(2),
            max_nested_runtime_invocations_per_top_level: 1,
        }
    }

    fn web_read_input() -> RuntimeExecutionPlanInput {
        RuntimeExecutionPlanInput {
            function_kind: InvocationKind::Query,
            runtime_profile: Some(RuntimeProfile::WebLean),
            effect_class: RuntimeEffectClass::PureLocalRead,
            side_channel_posture: RuntimeSideChannelPosture::ProvenSafeForCooperativeReuse,
            pool_authority_key: RuntimePoolAuthorityKey::exact(RuntimePoolAuthorityFacts::new(
                RuntimeProfile::WebLean,
                Vec::new(),
            )),
            node_full_realm_reuse_policy: RuntimeNodeFullRealmReusePolicy::Unproven,
            scheduling_class: RuntimeSchedulingClass::LatencySensitiveRead,
            tenant_budget: budget(),
            host_work_class: RuntimeHostWorkClass::Burstable,
            operator_enabled: true,
        }
    }

    fn request(kind: InvocationKind) -> InvocationRequest {
        InvocationRequest {
            kind,
            function_name: "messages:list".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        }
    }

    fn tenant_context(request: &InvocationRequest) -> RuntimeInvocationContext {
        RuntimeInvocationContext::top_level_for_tenant(request, "tenant-a")
    }

    #[test]
    fn runtime_execution_plan_admits_web_pure_read_candidate() {
        let plan = RuntimeExecutionPlan::classify(web_read_input());

        assert_eq!(plan.runtime_profile(), Some(RuntimeProfile::WebLean));
        assert_eq!(plan.host_work_class(), RuntimeHostWorkClass::Burstable);
        assert_eq!(
            plan.admission_outcome(),
            RuntimeAdmissionOutcome::NotEvaluated
        );
        assert!(plan.cooperative_eligibility().is_eligible());
    }

    #[test]
    fn runtime_execution_plan_rejects_effectful_semantic_kind() {
        let mut input = web_read_input();
        input.function_kind = InvocationKind::Mutation;

        let plan = RuntimeExecutionPlan::classify(input);

        assert_eq!(
            plan.cooperative_eligibility(),
            CooperativeEligibility::Ineligible(CooperativeIneligibilityReason::EffectfulKind)
        );
    }

    #[test]
    fn runtime_execution_plan_fails_closed_for_unknown_or_effectful_operations() {
        for (effect_class, reason) in [
            (
                RuntimeEffectClass::Unknown,
                CooperativeIneligibilityReason::UnknownEffect,
            ),
            (
                RuntimeEffectClass::Write,
                CooperativeIneligibilityReason::WriteHostOperation,
            ),
            (
                RuntimeEffectClass::Scheduler,
                CooperativeIneligibilityReason::SchedulerOperation,
            ),
            (
                RuntimeEffectClass::ServiceExternal,
                CooperativeIneligibilityReason::ServiceOrExternalOperation,
            ),
            (
                RuntimeEffectClass::NestedRuntime,
                CooperativeIneligibilityReason::NestedRuntimeOperation,
            ),
            (
                RuntimeEffectClass::Extension,
                CooperativeIneligibilityReason::ExtensionOperation,
            ),
            (
                RuntimeEffectClass::HttpRoute,
                CooperativeIneligibilityReason::HttpRouteOperation,
            ),
        ] {
            let mut input = web_read_input();
            input.effect_class = effect_class;

            let plan = RuntimeExecutionPlan::classify(input);

            assert_eq!(
                plan.cooperative_eligibility(),
                CooperativeEligibility::Ineligible(reason),
                "{effect_class:?} should map to {reason:?}"
            );
        }
    }

    #[test]
    fn runtime_execution_plan_admits_web_observable_read_candidate() {
        let mut input = web_read_input();
        input.effect_class = RuntimeEffectClass::ObservableRead;

        let plan = RuntimeExecutionPlan::classify(input);

        assert!(plan.cooperative_eligibility().is_eligible());
        assert!(plan.permits_cooperative_scheduler_admission());
    }

    #[test]
    fn runtime_execution_plan_reports_typed_observed_effect_violations() {
        let plan = RuntimeExecutionPlan::classify(web_read_input());

        assert_eq!(
            plan.observed_effect_violation(RuntimeEffectClass::ObservableRead),
            Some(RuntimeObservedEffectViolation {
                planned_effect_class: RuntimeEffectClass::PureLocalRead,
                observed_effect_class: RuntimeEffectClass::ObservableRead,
                reason: CooperativeIneligibilityReason::ObservableRead,
            })
        );
        assert_eq!(
            plan.observed_effect_violation(RuntimeEffectClass::Write),
            Some(RuntimeObservedEffectViolation {
                planned_effect_class: RuntimeEffectClass::PureLocalRead,
                observed_effect_class: RuntimeEffectClass::Write,
                reason: CooperativeIneligibilityReason::WriteHostOperation,
            })
        );

        let mut ineligible_input = web_read_input();
        ineligible_input.effect_class = RuntimeEffectClass::Write;
        let ineligible_plan = RuntimeExecutionPlan::classify(ineligible_input);
        assert_eq!(
            ineligible_plan.observed_effect_violation(RuntimeEffectClass::Write),
            None
        );

        let mut observable_input = web_read_input();
        observable_input.effect_class = RuntimeEffectClass::ObservableRead;
        let observable_plan = RuntimeExecutionPlan::classify(observable_input);
        assert_eq!(
            observable_plan.observed_effect_violation(RuntimeEffectClass::ObservableRead),
            None
        );
        assert_eq!(
            observable_plan.observed_effect_violation(RuntimeEffectClass::Write),
            Some(RuntimeObservedEffectViolation {
                planned_effect_class: RuntimeEffectClass::ObservableRead,
                observed_effect_class: RuntimeEffectClass::Write,
                reason: CooperativeIneligibilityReason::WriteHostOperation,
            })
        );
    }

    #[test]
    fn runtime_execution_plan_keeps_node_full_ineligible_until_realm_proof() {
        let mut input = web_read_input();
        input.runtime_profile = Some(RuntimeProfile::NodeFull);
        input.pool_authority_key = RuntimePoolAuthorityKey::exact(RuntimePoolAuthorityFacts::new(
            RuntimeProfile::NodeFull,
            Vec::new(),
        ));

        let plan = RuntimeExecutionPlan::classify(input);

        assert_eq!(
            plan.cooperative_eligibility(),
            CooperativeEligibility::Ineligible(CooperativeIneligibilityReason::NodeFullUnproven)
        );
    }

    #[test]
    fn runtime_execution_plan_requires_side_channel_posture_and_cpu_budget() {
        let mut missing_posture = web_read_input();
        missing_posture.side_channel_posture = RuntimeSideChannelPosture::Unknown;
        let posture_plan = RuntimeExecutionPlan::classify(missing_posture);
        assert_eq!(
            posture_plan.cooperative_eligibility(),
            CooperativeEligibility::Ineligible(
                CooperativeIneligibilityReason::SideChannelPostureMissing
            )
        );

        let mut cpu_heavy = web_read_input();
        cpu_heavy.scheduling_class = RuntimeSchedulingClass::CpuHeavy;
        let cpu_plan = RuntimeExecutionPlan::classify(cpu_heavy);
        assert_eq!(
            cpu_plan.cooperative_eligibility(),
            CooperativeEligibility::Ineligible(CooperativeIneligibilityReason::CpuHeavy)
        );
    }

    #[test]
    fn runtime_execution_plan_for_invocation_admits_web_read_jobs() {
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let request = request(InvocationKind::Query);
        let context = tenant_context(&request);

        let plan = RuntimeExecutionPlan::for_invocation(&policy, &request, &context);

        assert_eq!(plan.runtime_profile(), Some(RuntimeProfile::WebLean));
        assert_eq!(plan.host_work_class(), RuntimeHostWorkClass::Burstable);
        assert_eq!(
            plan.cooperative_eligibility(),
            CooperativeEligibility::Eligible
        );
        assert!(plan.permits_cooperative_scheduler_admission());
    }

    #[test]
    fn runtime_execution_plan_for_invocation_rejects_effectful_semantic_kinds() {
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());

        for kind in [InvocationKind::Mutation, InvocationKind::Action] {
            let request = request(kind);
            let context = tenant_context(&request);
            let plan = RuntimeExecutionPlan::for_invocation(&policy, &request, &context);

            assert_eq!(
                plan.cooperative_eligibility(),
                CooperativeEligibility::Ineligible(CooperativeIneligibilityReason::EffectfulKind)
            );
            assert!(!plan.permits_cooperative_scheduler_admission());
        }
    }

    #[test]
    fn runtime_execution_plan_for_invocation_keeps_node_full_read_ineligible_until_proven() {
        let policy = RuntimePolicy::new(RuntimeLimits::application_node24());
        let request = request(InvocationKind::Query);
        let context = tenant_context(&request);

        let plan = RuntimeExecutionPlan::for_invocation(&policy, &request, &context);

        assert_eq!(plan.runtime_profile(), Some(RuntimeProfile::NodeFull));
        assert_eq!(
            plan.cooperative_eligibility(),
            CooperativeEligibility::Ineligible(CooperativeIneligibilityReason::NodeFullUnproven)
        );
        assert!(!plan.permits_cooperative_scheduler_admission());
    }

    #[test]
    fn runtime_execution_plan_for_invocation_admits_node_full_with_same_owner_realm_proof() {
        let mut limits = RuntimeLimits::application_node24();
        limits.execution_model = RuntimeExecutionModel::CooperativeLocker;
        limits.runtime_pool_kind = RuntimePoolKind::WarmContextRecycle;
        limits.node_full_realm_reuse_policy =
            RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority;
        let policy = RuntimePolicy::new(limits);
        let request = request(InvocationKind::Query);
        let context = tenant_context(&request);

        let plan = RuntimeExecutionPlan::for_invocation(&policy, &request, &context);

        assert_eq!(plan.runtime_profile(), Some(RuntimeProfile::NodeFull));
        assert_eq!(
            plan.cooperative_eligibility(),
            CooperativeEligibility::Eligible
        );
        assert!(plan.permits_cooperative_scheduler_admission());
    }

    #[test]
    fn runtime_execution_plan_keeps_node_full_ineligible_for_uv_handle_grants() {
        let cases: &[RuntimeLimitsCase] = &[
            ("net_connect", |limits| {
                limits.grants.net_connect = vec!["127.0.0.1".to_string()];
            }),
            ("net_listen", |limits| {
                limits.grants.net_listen = vec!["127.0.0.1".to_string()];
            }),
            ("run", |limits| {
                limits.grants.run = vec!["$runtime_self_exec".to_string()];
            }),
            ("ffi", |limits| {
                limits.mode = RuntimeMode::Privileged;
                limits.grants.ffi = vec!["/usr/lib/libexample.dylib".to_string()];
            }),
            ("worker", |limits| {
                limits.grants.worker = vec!["thread".to_string()];
            }),
            ("tool", |limits| {
                limits.grants.tool = vec!["shell".to_string()];
            }),
            ("inspector", |limits| {
                limits.grants.sys.push("inspector".to_string());
            }),
        ];

        for (name, configure) in cases {
            let mut limits = RuntimeLimits::application_node24();
            limits.execution_model = RuntimeExecutionModel::CooperativeLocker;
            limits.runtime_pool_kind = RuntimePoolKind::WarmContextRecycle;
            limits.node_full_realm_reuse_policy =
                RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority;
            configure(&mut limits);
            let policy = RuntimePolicy::new(limits);
            let request = request(InvocationKind::Query);
            let context = tenant_context(&request);

            let plan = RuntimeExecutionPlan::for_invocation(&policy, &request, &context);

            assert_eq!(
                plan.cooperative_eligibility(),
                CooperativeEligibility::Ineligible(
                    CooperativeIneligibilityReason::SideChannelPostureMissing
                ),
                "NodeFull same-process realm reuse must reject {name} grants that can create uv/native host handles"
            );
            assert!(!plan.permits_cooperative_scheduler_admission());
        }
    }

    #[test]
    fn runtime_execution_plan_for_realm_lease_admits_node_full_with_strict_authority() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);
        let mut limits = RuntimeLimits::application_node24();
        limits.execution_model = RuntimeExecutionModel::CooperativeLocker;
        limits.runtime_pool_kind = RuntimePoolKind::WarmContextRecycle;
        limits.node_full_realm_reuse_policy =
            RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority;
        let policy = RuntimePolicy::new(limits);
        let request = request(InvocationKind::Query);
        let context = tenant_context(&request);

        let plan = RuntimeExecutionPlan::for_realm_lease_invocation(
            &policy,
            &bundle,
            &request,
            &context,
            V8RuntimeConstructionMode::StartupSnapshot,
        )
        .expect("realm lease plan should classify");

        assert_eq!(plan.runtime_profile(), Some(RuntimeProfile::NodeFull));
        assert_eq!(
            plan.cooperative_eligibility(),
            CooperativeEligibility::Eligible
        );
        assert!(plan.permits_cooperative_scheduler_admission());
        match plan.pool_authority_key() {
            RuntimePoolAuthorityKey::Exact(facts) => {
                assert!(
                    facts.strict_reuse.is_some(),
                    "realm lease admission must carry strict bundle/authority facts"
                );
            }
            RuntimePoolAuthorityKey::Missing(reason) => {
                panic!("realm lease admission should have exact authority, got {reason:?}");
            }
        }
    }

    #[test]
    fn runtime_execution_plan_for_invocation_requires_safe_side_channel_posture() {
        let mut limits = RuntimeLimits::application_web_standard();
        limits.grants.service.push("search".to_string());
        let policy = RuntimePolicy::new(limits);
        let request = request(InvocationKind::Query);
        let context = tenant_context(&request);

        let plan = RuntimeExecutionPlan::for_invocation(&policy, &request, &context);

        assert_eq!(
            plan.cooperative_eligibility(),
            CooperativeEligibility::Ineligible(
                CooperativeIneligibilityReason::SideChannelPostureMissing
            )
        );
        assert!(!plan.permits_cooperative_scheduler_admission());
    }

    #[test]
    fn runtime_execution_plan_for_invocation_carries_host_work_class() {
        let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
        let request = request(InvocationKind::Query);
        let context = tenant_context(&request).with_bypassed_concurrency_limit();

        let plan = RuntimeExecutionPlan::for_invocation(&policy, &request, &context);

        assert_eq!(plan.host_work_class(), RuntimeHostWorkClass::Guaranteed);
    }

    #[test]
    fn realm_lease_authority_key_partitions_target_bundle_and_construction_mode() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_a_path = tempdir.path().join("bundle-a.mjs");
        let bundle_b_path = tempdir.path().join("bundle-b.mjs");
        std::fs::write(&bundle_a_path, "export {};").expect("bundle A should write");
        std::fs::write(&bundle_b_path, "export {};").expect("bundle B should write");
        let bundle_a = RuntimeBundle::new(&bundle_a_path);
        let bundle_b = RuntimeBundle::new(&bundle_b_path);
        let request = request(InvocationKind::Query);
        let context = tenant_context(&request);

        let mut node22_db_cache = RuntimeLimits::application_node22();
        node22_db_cache.service_capability_enabled = true;
        node22_db_cache.grants.service = vec!["db".to_string(), "cache".to_string()];
        let node22_db_cache_policy = RuntimePolicy::new(node22_db_cache);
        let mut node22_cache_db = RuntimeLimits::application_node22();
        node22_cache_db.service_capability_enabled = true;
        node22_cache_db.grants.service = vec!["cache".to_string(), "db".to_string()];
        let node22_cache_db_policy = RuntimePolicy::new(node22_cache_db);
        let node24_db_cache_policy = {
            let mut limits = RuntimeLimits::application_node24();
            limits.service_capability_enabled = true;
            limits.grants.service = vec!["db".to_string(), "cache".to_string()];
            RuntimePolicy::new(limits)
        };

        let node22_bundle_a = RuntimeExecutionPlan::for_realm_lease_invocation(
            &node22_db_cache_policy,
            &bundle_a,
            &request,
            &context,
            V8RuntimeConstructionMode::StartupSnapshot,
        )
        .expect("node22 bundle A plan should classify");
        let node22_bundle_a_reordered_grants = RuntimeExecutionPlan::for_realm_lease_invocation(
            &node22_cache_db_policy,
            &bundle_a,
            &request,
            &context,
            V8RuntimeConstructionMode::StartupSnapshot,
        )
        .expect("node22 bundle A plan with reordered grants should classify");
        let node22_bundle_b = RuntimeExecutionPlan::for_realm_lease_invocation(
            &node22_db_cache_policy,
            &bundle_b,
            &request,
            &context,
            V8RuntimeConstructionMode::StartupSnapshot,
        )
        .expect("node22 bundle B plan should classify");
        let node24_bundle_a = RuntimeExecutionPlan::for_realm_lease_invocation(
            &node24_db_cache_policy,
            &bundle_a,
            &request,
            &context,
            V8RuntimeConstructionMode::StartupSnapshot,
        )
        .expect("node24 bundle A plan should classify");
        let node22_unsnapshotted = RuntimeExecutionPlan::for_realm_lease_invocation(
            &node22_db_cache_policy,
            &bundle_a,
            &request,
            &context,
            V8RuntimeConstructionMode::Unsnapshotted,
        )
        .expect("node22 unsnapshotted plan should classify");

        assert_eq!(
            node22_bundle_a.pool_authority_key(),
            node22_bundle_a_reordered_grants.pool_authority_key(),
            "service grant order should not fragment authority"
        );
        assert_ne!(
            node22_bundle_a.pool_authority_key(),
            node22_bundle_b.pool_authority_key(),
            "bundle identity should partition realm reuse"
        );
        assert_ne!(
            node22_bundle_a.pool_authority_key(),
            node24_bundle_a.pool_authority_key(),
            "Node target should partition realm reuse"
        );
        assert_ne!(
            node22_bundle_a.pool_authority_key(),
            node22_unsnapshotted.pool_authority_key(),
            "construction mode should partition realm reuse"
        );
    }

    #[test]
    fn realm_lease_authority_key_partitions_permission_grants_and_node_conditions() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("bundle.mjs");
        std::fs::write(&bundle_path, "export {};").expect("bundle should write");
        let bundle = RuntimeBundle::new(&bundle_path);
        let request = request(InvocationKind::Query);
        let context = tenant_context(&request);

        let base_policy = RuntimePolicy::new(RuntimeLimits::application_node22());
        let read_policy = {
            let mut limits = RuntimeLimits::application_node22();
            limits.grants.read = vec!["./data-a".to_string()];
            RuntimePolicy::new(limits)
        };
        let env_policy = {
            let mut limits = RuntimeLimits::application_node22();
            limits.grants.env_read = vec!["NIMBUS_TOKEN_A".to_string()];
            RuntimePolicy::new(limits)
        };
        let conditions_policy = {
            let mut limits = RuntimeLimits::application_node22();
            limits.node_conditions.push("nimbus-custom".to_string());
            RuntimePolicy::new(limits)
        };

        let base = RuntimeExecutionPlan::for_realm_lease_invocation(
            &base_policy,
            &bundle,
            &request,
            &context,
            V8RuntimeConstructionMode::StartupSnapshot,
        )
        .expect("base plan should classify");
        let read = RuntimeExecutionPlan::for_realm_lease_invocation(
            &read_policy,
            &bundle,
            &request,
            &context,
            V8RuntimeConstructionMode::StartupSnapshot,
        )
        .expect("read-grant plan should classify");
        let env = RuntimeExecutionPlan::for_realm_lease_invocation(
            &env_policy,
            &bundle,
            &request,
            &context,
            V8RuntimeConstructionMode::StartupSnapshot,
        )
        .expect("env-grant plan should classify");
        let conditions = RuntimeExecutionPlan::for_realm_lease_invocation(
            &conditions_policy,
            &bundle,
            &request,
            &context,
            V8RuntimeConstructionMode::StartupSnapshot,
        )
        .expect("condition plan should classify");

        assert_ne!(
            base.pool_authority_key(),
            read.pool_authority_key(),
            "read grants should partition realm reuse authority"
        );
        assert_ne!(
            base.pool_authority_key(),
            env.pool_authority_key(),
            "env grants should partition realm reuse authority"
        );
        assert_ne!(
            base.pool_authority_key(),
            conditions.pool_authority_key(),
            "Node conditions should partition realm reuse authority"
        );
    }
}
