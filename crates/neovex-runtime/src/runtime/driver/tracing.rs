use tracing::{debug, warn};

use crate::RuntimeInvocationContext;

use super::super::{InvocationRequest, RuntimeBundle, RuntimeConstructionMode};

pub(super) fn trace_snapshot_seeded_runtime_phase(
    construction_mode: RuntimeConstructionMode,
    bundle: &RuntimeBundle,
    context: Option<&RuntimeInvocationContext>,
    request: Option<&InvocationRequest>,
    phase: &'static str,
) {
    trace_snapshot_seeded_runtime_phase_with_optional_bundle(
        construction_mode,
        Some(bundle),
        context,
        request,
        phase,
    );
}

pub(super) fn trace_snapshot_seeded_runtime_phase_with_optional_bundle(
    construction_mode: RuntimeConstructionMode,
    bundle: Option<&RuntimeBundle>,
    context: Option<&RuntimeInvocationContext>,
    request: Option<&InvocationRequest>,
    phase: &'static str,
) {
    if !construction_mode.uses_startup_snapshot() {
        return;
    }
    debug!(
        construction_mode = construction_mode.as_str(),
        bundle = bundle.map(|bundle| bundle.entrypoint().display().to_string()),
        invocation_id = context.map(|context| context.invocation_id),
        tenant = context.and_then(|context| context.tenant_label.as_deref()),
        request_id = context.and_then(|context| context.server_request_id.as_deref()),
        context_function = context.map(|context| context.function_name.as_str()),
        request_function = request.map(|request| request.function_name.as_str()),
        request_kind = request.map(|request| request.kind.as_str()),
        phase,
        "snapshot-seeded runtime phase"
    );
}

pub(super) fn trace_snapshot_seeded_runtime_error(
    construction_mode: RuntimeConstructionMode,
    bundle: &RuntimeBundle,
    context: Option<&RuntimeInvocationContext>,
    request: Option<&InvocationRequest>,
    phase: &'static str,
    error: &impl std::fmt::Display,
) {
    trace_snapshot_seeded_runtime_error_with_optional_bundle(
        construction_mode,
        Some(bundle),
        context,
        request,
        phase,
        error,
    );
}

pub(super) fn trace_snapshot_seeded_runtime_error_with_optional_bundle(
    construction_mode: RuntimeConstructionMode,
    bundle: Option<&RuntimeBundle>,
    context: Option<&RuntimeInvocationContext>,
    request: Option<&InvocationRequest>,
    phase: &'static str,
    error: &impl std::fmt::Display,
) {
    if !construction_mode.uses_startup_snapshot() {
        return;
    }
    warn!(
        construction_mode = construction_mode.as_str(),
        bundle = bundle.map(|bundle| bundle.entrypoint().display().to_string()),
        invocation_id = context.map(|context| context.invocation_id),
        tenant = context.and_then(|context| context.tenant_label.as_deref()),
        request_id = context.and_then(|context| context.server_request_id.as_deref()),
        context_function = context.map(|context| context.function_name.as_str()),
        request_function = request.map(|request| request.function_name.as_str()),
        request_kind = request.map(|request| request.kind.as_str()),
        phase,
        error = %error,
        "snapshot-seeded runtime phase failed"
    );
}
