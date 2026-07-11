mod affinity;
mod backends;
mod context;
mod egress;
mod error;
mod execution_plan;
mod executor;
pub mod fs;
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

pub fn wasmtime_component_linker_diagnostics() -> Result<()> {
    backends::wasmtime::component_linker_diagnostics()
}

/// Build / check the embeddable NodeFull(Node22) anchor-snapshot blob. Called by the
/// `build_node22_anchor_snapshot` builder binary (a normal consumer of this crate): it writes the
/// committed per-config `.bin`/`.pc.bin` the lib `include_bytes!`es, and `--check` byte-compares a
/// fresh rebuild against the committed file. Neither is used on the serving path.
pub use backends::v8::{
    build_embeddable_node22_snapshot_blob, check_committed_embedded_anchor_snapshot,
};
/// DIAGNOSTIC re-export: force-install the committed embedded anchor snapshot in THIS binary to
/// isolate "bad bytes" from "cross-binary" magic mismatches. Used by `build_node22_anchor_snapshot
/// --smoke`. See `runtime::driver::anchor::smoke_install_committed_embedded_anchor`.
pub use runtime::driver::anchor::smoke_install_committed_embedded_anchor;

pub use context::RuntimeInvocationContext;
pub use egress::{
    DenyAllEgressGateway, EgressAuthorization, EgressGateway, EgressProtocol, EgressRequest,
    EgressRequestError, EgressSubstrate, RuntimeEgressPosture, WasmHttpClientEgressGatewayBinding,
};
pub use error::{NimbusRuntimeError, Result};
pub use executor::{RuntimeExecutor, RuntimeInvocationResponse};
pub use fs::{NimbusFsBackend, RuntimeFileSystem};
pub use host::{
    HOST_CALL_ABI_VERSION, HostBridge, HostBridgeFuture, HostCallCancellation,
    HostCallCancellationCause, HostCallEnvelope, HostCallOperation, HostCallPayload,
    HostCallRequest, RuntimeAsyncActionPayload, RuntimeAsyncCfKvDeletePayload,
    RuntimeAsyncCfKvGetPayload, RuntimeAsyncCfKvListPayload, RuntimeAsyncCfKvPutPayload,
    RuntimeAsyncDbDeletePayload, RuntimeAsyncDbGetPayload, RuntimeAsyncDbInsertPayload,
    RuntimeAsyncDbPatchPayload, RuntimeAsyncExtensionPayload, RuntimeAsyncFunctionCallPayload,
    RuntimeAsyncHttpRoutePayload, RuntimeAsyncMutationPayload, RuntimeAsyncPaginatedQueryPayload,
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
    RuntimeGrants, RuntimeGuestSemantics, RuntimeHostAdmissionAction, RuntimeHostAdmissionDecision,
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
    InvocationKind, InvocationRequest, InvocationServiceBinding, InvocationServiceEndpoint,
    InvocationServiceProtocol, InvocationServices, NimbusRuntime, RuntimeBundle,
    RuntimeBundleContent, RuntimeBundleWasmComponentContent, RuntimeComponentWorld,
};
