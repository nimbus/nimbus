mod affinity;
mod backends;
mod context;
mod error;
mod execution_plan;
mod executor;
mod host;
mod limits;
mod metrics;
mod module_loader;
mod node_compat;
mod runtime;
mod runtime_capabilities;
#[cfg(test)]
mod test_support;
mod watchdog;
mod worker_loop;

pub fn bun_jsc_execution_adapter_state() -> RuntimeExecutionAdapterState {
    backends::bun_jsc::execution_adapter_state()
}

pub fn bun_jsc_adapter_artifact_diagnostics() -> RuntimeExecutionAdapterArtifactDiagnostics {
    backends::bun_jsc::adapter_artifact_diagnostics()
}

pub use context::RuntimeInvocationContext;
pub use error::{NimbusRuntimeError, Result};
pub use executor::{RuntimeExecutor, RuntimeInvocationResponse};
pub use host::{
    HOST_CALL_ABI_VERSION, HostBridge, HostBridgeFuture, HostCallCancellation,
    HostCallCancellationCause, HostCallEnvelope, HostCallOperation, HostCallPayload,
    HostCallRequest, RuntimeAsyncActionPayload, RuntimeAsyncDbDeletePayload,
    RuntimeAsyncDbGetPayload, RuntimeAsyncDbInsertPayload, RuntimeAsyncDbPatchPayload,
    RuntimeAsyncExtensionPayload, RuntimeAsyncFunctionCallPayload, RuntimeAsyncHttpRoutePayload,
    RuntimeAsyncMutationPayload, RuntimeAsyncPaginatedQueryPayload,
    RuntimeAsyncQueryPaginatePayload, RuntimeAsyncQueryPayload, RuntimeAsyncQueryTakePayload,
    RuntimeAsyncQueryTerminalPayload, RuntimeAsyncSchedulerCancelPayload,
    RuntimeAsyncSchedulerRunAfterPayload, RuntimeAsyncSchedulerRunAtPayload,
    RuntimeAsyncServiceLookupPayload, RuntimeSyncNestedCallPayload, RuntimeSyncQueryFilterPayload,
    RuntimeSyncQueryOrderPayload, RuntimeSyncQueryStartPayload, RuntimeSyncQueryWithIndexPayload,
};
pub use limits::{
    EffectiveRuntimeScalingPlan, NominalRuntimeHostPressureSource, RequestedRuntimeScalingTarget,
    RuntimeAdaptiveActuationResult, RuntimeAdaptiveActuator, RuntimeAdaptiveCanaryPolicy,
    RuntimeAdaptiveClock, RuntimeAdaptiveControllerMode, RuntimeAdaptiveControllerSettings,
    RuntimeAdaptiveMetricsSink, RuntimeAdaptiveObservationSource, RuntimeAdaptivePressureAdapter,
    RuntimeAdaptiveWarmPoolActuation, RuntimeAdaptiveWarmPoolActuationKind,
    RuntimeAdaptiveWarmPoolAuthorityInput, RuntimeAdaptiveWarmPoolController,
    RuntimeAdaptiveWarmPoolDecision, RuntimeAdaptiveWarmPoolDecisionReason,
    RuntimeAdaptiveWarmPoolEvaluation, RuntimeAdaptiveWarmPoolRun, RuntimeAdaptiveWarmPoolSnapshot,
    RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
    RuntimeBackendTrustTier, RuntimeBundleContentKind, RuntimeCompatibilityTarget,
    RuntimeControllerReplayAuthorityInput, RuntimeControllerReplayAuthorityKey,
    RuntimeControllerReplayConfig, RuntimeControllerReplayDecision,
    RuntimeControllerReplayObservation, RuntimeControllerReplayState, RuntimeDensityBudget,
    RuntimeDensityMeasurement, RuntimeDensityMeasurementMethod, RuntimeDensityPlan,
    RuntimeExecutionAdapterArtifactDiagnostics, RuntimeExecutionAdapterArtifactSource,
    RuntimeExecutionAdapterArtifactStatus, RuntimeExecutionAdapterExpectedArtifact,
    RuntimeExecutionAdapterManifestArtifact, RuntimeExecutionAdapterState, RuntimeExecutionModel,
    RuntimeGrants, RuntimeHostAdmissionAction, RuntimeHostAdmissionDecision,
    RuntimeHostPressureLevel, RuntimeHostPressureSample, RuntimeHostPressureSource,
    RuntimeHostPressureSourceStatus, RuntimeHostResourceBudget, RuntimeHostResourceDecision,
    RuntimeHostWorkClass, RuntimeIsolateGroupFfiStatus, RuntimeJavaScriptEvaluationFormat,
    RuntimeLanguage, RuntimeLimits, RuntimeMemoryEnforcement, RuntimeMemoryPressureDecision,
    RuntimeMemoryPressureLevel, RuntimeMemoryPressureSample, RuntimeMemoryPressureSourceStatus,
    RuntimeMode, RuntimeModuleStateSemantics, RuntimeNodeFullRealmReusePolicy, RuntimeNodeLtsLane,
    RuntimeNodeSupportPhase, RuntimePolicy, RuntimePoolKind, RuntimePreset,
    RuntimePrewarmScheduleDecision, RuntimeProfile, RuntimeResetCapabilities,
    RuntimeRoutingAffinity, RuntimeScalingAdjustmentReason, RuntimeScalingLimit,
    RuntimeScalingPlanSet, RuntimeScalingPreset, RuntimeScalingTarget, RuntimeTenantBudget,
    replay_runtime_controller,
};
pub use metrics::{
    RuntimeAdaptiveControllerMetricsSnapshot, RuntimeDurationDistributionSnapshot,
    RuntimeHostOperationMetricsSnapshot, RuntimeHostPressureMetricsSnapshot, RuntimeMetrics,
    RuntimeMetricsSnapshot, RuntimeProfileCountersSnapshot, RuntimeProfileTelemetrySnapshot,
    RuntimeRequestCorrelationSnapshot, RuntimeTenantMetricsSnapshot,
};
pub use runtime::{
    InvocationAuth, InvocationKind, InvocationRequest, InvocationServiceBinding,
    InvocationServiceEndpoint, InvocationServiceProtocol, InvocationServices, NimbusRuntime,
    RuntimeBundle, RuntimeUserIdentity, VerifiedUserIdentity, VerifiedUserIdentityKind,
};
