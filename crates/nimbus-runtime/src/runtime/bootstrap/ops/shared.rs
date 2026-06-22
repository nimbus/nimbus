use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use serde::Serialize;
use serde_json::Value;

use crate::backends::v8::embedder::{CancelFuture, JsErrorBox, OpState, op2};
use crate::execution_plan::RuntimeEffectClass;
use crate::executor::SharedInvocationPermit;
use crate::host::{HostCallOperation, HostCallRequest};
use crate::limits::{
    RuntimeCompatibilityTarget, RuntimeGrants, RuntimeLanguage, RuntimeMode,
    RuntimeNodeSupportPhase, RuntimePreset,
};
use crate::runtime_capabilities::RuntimeContractPathsDescriptor;

use super::super::payloads::RuntimeHostCallEnvelope;
use super::super::state::{
    InstalledRuntimeCapabilityPolicy, InstalledRuntimeContract, InstalledRuntimeHostBridge,
    RuntimeCancellationState, RuntimeInvocationExecutionPlanBinding,
    RuntimeInvocationHostCallBinding, RuntimeWaitUntilState,
};

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct RuntimeContractDescriptor {
    compatibility_target: RuntimeCompatibilityTarget,
    node_api_contract: Option<RuntimeNodeApiContractDescriptor>,
    runtime_mode: RuntimeMode,
    runtime_language: RuntimeLanguage,
    runtime_preset: RuntimePreset,
    runtime_grants: RuntimeGrants,
    paths: RuntimeContractPathsDescriptor,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct RuntimeNodeApiContractDescriptor {
    lane_name: &'static str,
    support_phase: RuntimeNodeSupportPhase,
    version: &'static str,
    version_number: &'static str,
    release_name: &'static str,
    release_lts: Option<&'static str>,
    module_version: &'static str,
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_contract(state: &mut OpState) -> RuntimeContractDescriptor {
    let contract = state.borrow::<InstalledRuntimeContract>();
    let capability_policy = state.borrow::<InstalledRuntimeCapabilityPolicy>();
    let limits = &contract.limits;
    RuntimeContractDescriptor {
        compatibility_target: limits.compatibility_target,
        node_api_contract: limits
            .compatibility_target
            .node_lts_metadata()
            .map(|metadata| RuntimeNodeApiContractDescriptor {
                lane_name: metadata.lane_name.as_str(),
                support_phase: metadata.support_phase,
                version: metadata.upstream_tag.as_str(),
                version_number: metadata.upstream_version.as_str(),
                release_name: metadata.release_name.as_str(),
                release_lts: metadata.codename.as_deref(),
                module_version: metadata.node_module_version.as_str(),
            }),
        runtime_mode: limits.mode,
        runtime_language: limits.language,
        runtime_preset: limits.preset,
        runtime_grants: limits.grants.clone(),
        paths: capability_policy.paths.descriptor(),
    }
}

#[op2]
#[string]
pub(super) fn op_nimbus_runtime_host_call_session_id(
    state: &mut OpState,
) -> std::result::Result<String, JsErrorBox> {
    state
        .borrow::<RuntimeInvocationHostCallBinding>()
        .session_id()
        .map(str::to_owned)
        .ok_or_else(|| JsErrorBox::generic("runtime host-call session is not active"))
}

#[op2(fast)]
pub(super) fn op_nimbus_runtime_wait_until_pending(state: &mut OpState) {
    if let Some(wait_until) = state.try_borrow_mut::<RuntimeWaitUntilState>() {
        wait_until.mark_pending();
    }
}

struct HostCallPermitLease {
    permit: SharedInvocationPermit,
    completed: bool,
}

impl HostCallPermitLease {
    async fn new(permit: SharedInvocationPermit) -> Self {
        permit.begin_async_host_call().await;
        Self {
            permit,
            completed: false,
        }
    }

    async fn complete(&mut self) -> std::result::Result<(), JsErrorBox> {
        self.completed = true;
        self.permit
            .complete_async_host_call()
            .await
            .map_err(|error| JsErrorBox::generic(error.to_string()))
    }
}

impl Drop for HostCallPermitLease {
    fn drop(&mut self) {
        if !self.completed {
            self.permit.drop_async_host_call();
        }
    }
}

pub(super) async fn op_nimbus_async_host_call<T>(
    state: Rc<RefCell<OpState>>,
    operation: HostCallOperation,
    payload: T,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox>
where
    T: Serialize + Send + 'static,
{
    let (
        host_bridge,
        cancel_handle,
        cancellation_signal,
        permit,
        contract,
        host_call_binding,
        execution_plan_binding,
    ) = {
        let state = state.borrow();
        (
            state.borrow::<InstalledRuntimeHostBridge>().slot.current(),
            state
                .borrow::<RuntimeCancellationState>()
                .cancel_handle
                .clone(),
            state.borrow::<RuntimeCancellationState>().signal.clone(),
            state.borrow::<SharedInvocationPermit>().clone(),
            state.borrow::<InstalledRuntimeContract>().clone(),
            state.borrow::<RuntimeInvocationHostCallBinding>().clone(),
            state
                .borrow::<RuntimeInvocationExecutionPlanBinding>()
                .clone(),
        )
    };
    let payload_value =
        serde_json::to_value(payload).map_err(|error| JsErrorBox::generic(error.to_string()))?;
    enforce_live_host_call_session(operation, &payload_value, &host_call_binding)?;
    enforce_host_call_grants(operation, &payload_value, &contract)?;
    enforce_observed_host_call_effect(operation, &payload_value, &execution_plan_binding)?;
    permit.record_host_operation_started(operation);
    let mut permit_lease = HostCallPermitLease::new(permit.clone()).await;
    let host_bridge_started_at = Instant::now();
    let host_call = host_bridge
        .call_async(
            HostCallRequest::new(operation, payload_value),
            cancellation_signal.clone(),
        )
        .or_cancel(cancel_handle.clone());
    tokio::pin!(host_call);

    let mut canceled_in_flight = false;
    let result = tokio::select! {
        result = &mut host_call => {
            normalize_completed_host_call_result(result, permit_lease.complete().await)
        }
        _ = cancellation_signal.cancelled() => {
            canceled_in_flight = true;
            cancel_handle.cancel();
            let result = host_call.await;
            normalize_completed_host_call_result(result, permit_lease.complete().await)
        }
    };
    permit.record_host_bridge_call(operation, host_bridge_started_at.elapsed());
    if canceled_in_flight {
        permit.record_host_operation_canceled_in_flight(operation);
    } else if result.is_ok() {
        permit.record_host_operation_succeeded(operation);
    } else {
        permit.record_host_operation_failed(operation);
    }
    result
}

fn normalize_completed_host_call_result<E>(
    result: std::result::Result<crate::error::Result<Value>, E>,
    permit_result: std::result::Result<(), JsErrorBox>,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox>
where
    JsErrorBox: From<E>,
{
    permit_result?;
    normalize_host_call_value(
        result
            .map_err(JsErrorBox::from)?
            .map_err(|error| JsErrorBox::generic(error.to_string()))?,
    )
}

pub(super) fn op_nimbus_sync_host_call<T>(
    state: &mut OpState,
    operation: HostCallOperation,
    payload: T,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox>
where
    T: Serialize,
{
    let host_bridge = state.borrow::<InstalledRuntimeHostBridge>().slot.current();
    let permit = state.borrow::<SharedInvocationPermit>().clone();
    let contract = state.borrow::<InstalledRuntimeContract>().clone();
    let host_call_binding = state.borrow::<RuntimeInvocationHostCallBinding>().clone();
    let execution_plan_binding = state
        .borrow::<RuntimeInvocationExecutionPlanBinding>()
        .clone();
    let payload_value =
        serde_json::to_value(payload).map_err(|error| JsErrorBox::generic(error.to_string()))?;
    enforce_live_host_call_session(operation, &payload_value, &host_call_binding)?;
    enforce_host_call_grants(operation, &payload_value, &contract)?;
    enforce_observed_host_call_effect(operation, &payload_value, &execution_plan_binding)?;
    permit.record_host_operation_started(operation);
    let host_bridge_started_at = Instant::now();
    let result = host_bridge
        .call(HostCallRequest::new(operation, payload_value))
        .map_err(|error| JsErrorBox::generic(error.to_string()));
    permit.record_host_bridge_call(operation, host_bridge_started_at.elapsed());
    let value = match result {
        Ok(value) => {
            permit.record_host_operation_succeeded(operation);
            value
        }
        Err(error) => {
            permit.record_host_operation_failed(operation);
            return Err(error);
        }
    };
    normalize_host_call_value(value)
}

fn enforce_live_host_call_session(
    operation: HostCallOperation,
    payload: &Value,
    binding: &RuntimeInvocationHostCallBinding,
) -> std::result::Result<(), JsErrorBox> {
    if !operation_requires_host_call_session(operation) {
        return Ok(());
    }

    let Some(expected_session) = binding.session_id() else {
        return Err(JsErrorBox::generic(format!(
            "runtime host-call session is stale or forged for `{operation}`: no live invocation binding"
        )));
    };
    let provided_session = payload
        .get("host_call_session_id")
        .and_then(Value::as_str)
        .filter(|session| !session.is_empty());
    if provided_session == Some(expected_session) {
        return Ok(());
    }

    let invocation = binding
        .invocation_id()
        .map(|id| id.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let tenant = binding.tenant_label().unwrap_or("unknown");
    Err(JsErrorBox::generic(format!(
        "runtime host-call session is stale or forged for `{operation}`: expected `{expected_session}` for invocation {invocation} tenant `{tenant}`"
    )))
}

const fn operation_requires_host_call_session(operation: HostCallOperation) -> bool {
    !matches!(
        operation,
        HostCallOperation::HttpRoute | HostCallOperation::RuntimeExtensionCall
    )
}

fn enforce_host_call_grants(
    operation: HostCallOperation,
    payload: &Value,
    contract: &InstalledRuntimeContract,
) -> std::result::Result<(), JsErrorBox> {
    if operation != HostCallOperation::CtxServiceLookup {
        return Ok(());
    }

    let service_name = payload
        .get("service_name")
        .and_then(Value::as_str)
        .ok_or_else(|| JsErrorBox::generic("runtime service lookup is missing service_name"))?;
    if contract
        .limits
        .grants
        .service
        .iter()
        .any(|allowed| allowed == service_name)
    {
        return Ok(());
    }

    Err(JsErrorBox::generic(format!(
        "runtime service grant denied for `{service_name}`"
    )))
}

fn enforce_observed_host_call_effect(
    operation: HostCallOperation,
    payload: &Value,
    execution_plan_binding: &RuntimeInvocationExecutionPlanBinding,
) -> std::result::Result<(), JsErrorBox> {
    let Some(plan) = execution_plan_binding.plan() else {
        return Ok(());
    };
    let observed_effect_class = observed_host_call_effect_class(operation, payload);
    let Some(violation) = plan.observed_effect_violation(observed_effect_class) else {
        return Ok(());
    };

    Err(JsErrorBox::generic(format!(
        "runtime host-call effect violation for `{operation}`: planned {:?} but observed {:?} ({:?})",
        violation.planned_effect_class, violation.observed_effect_class, violation.reason
    )))
}

fn observed_host_call_effect_class(
    operation: HostCallOperation,
    payload: &Value,
) -> RuntimeEffectClass {
    if operation != HostCallOperation::CtxRuntimeEnterNestedCall {
        return operation.runtime_effect_class();
    }

    match payload.get("kind").and_then(Value::as_str) {
        Some("query" | "paginated_query") => RuntimeEffectClass::ObservableRead,
        Some("mutation") => RuntimeEffectClass::Write,
        Some("action" | "http_action") => RuntimeEffectClass::ServiceExternal,
        _ => operation.runtime_effect_class(),
    }
}

fn normalize_host_call_value(
    value: Value,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    match serde_json::from_value::<RuntimeHostCallEnvelope>(value.clone()) {
        Ok(envelope) => Ok(envelope),
        Err(_) => Ok(RuntimeHostCallEnvelope::Ok { value }),
    }
}
