use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use crate::backends::v8::embedder::{JsErrorBox, OpState};
use crate::runtime::NimbusRuntime;
use crate::runtime::bootstrap::payloads::RuntimeHostCallEnvelope;
use crate::runtime::bootstrap::state::{InstalledRuntimeContract, InstalledRuntimeHostBridge};
use crate::{InvocationKind, InvocationRequest, RuntimePolicy};

use super::bundle::write_runtime_test_spawn_bundle;
use super::parser::runtime_test_spawn_mode;
use super::types::{
    PreparedRuntimeTestSpawnInvocation, RuntimeTestProcessStateSnapshot, RuntimeTestSpawnPayload,
    RuntimeTestSpawnResult,
};

pub(super) fn prepare_runtime_test_spawn_invocation(
    state: Rc<RefCell<OpState>>,
    payload: RuntimeTestSpawnPayload,
) -> std::result::Result<PreparedRuntimeTestSpawnInvocation, JsErrorBox> {
    let current_exec = std::env::current_exe().map_err(|error| {
        JsErrorBox::generic(format!(
            "failed to resolve current executable path: {error}"
        ))
    })?;
    let current_exec_string = current_exec.to_string_lossy().into_owned();
    let command_path = PathBuf::from(&payload.command);
    let canonical_current_exec =
        std::fs::canonicalize(&current_exec).unwrap_or_else(|_| current_exec.clone());
    let canonical_command_path =
        std::fs::canonicalize(&command_path).unwrap_or_else(|_| command_path.clone());
    let supports_command = payload.command == current_exec_string
        || canonical_command_path == canonical_current_exec
        || (command_path.is_absolute()
            && command_path.exists()
            && command_path.file_name() == current_exec.file_name());
    if !supports_command {
        return Err(JsErrorBox::generic(format!(
            "node_compat subprocess helper only supports process.execPath; received `{}`",
            payload.command
        )));
    }

    let plan = runtime_test_spawn_mode(payload)?;
    let (host, contract) = {
        let state = state.borrow();
        (
            state.borrow::<InstalledRuntimeHostBridge>().slot.current(),
            state.borrow::<InstalledRuntimeContract>().clone(),
        )
    };
    let limits = contract.limits;
    let runtime = NimbusRuntime::with_policy(host, Arc::new(RuntimePolicy::new(limits)));
    let (tempdir, bundle_path, file_output_syncs) = write_runtime_test_spawn_bundle(&plan)?;
    let process_state_snapshot = RuntimeTestProcessStateSnapshot::capture();
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "node_compat:spawn".to_string(),
        args: serde_json::Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    Ok(PreparedRuntimeTestSpawnInvocation {
        _tempdir: tempdir,
        runtime,
        bundle_path,
        file_output_syncs,
        request,
        process_state_snapshot,
    })
}

pub(super) fn runtime_test_spawn_result_from_value(
    result: crate::error::Result<serde_json::Value>,
) -> std::result::Result<RuntimeTestSpawnResult, JsErrorBox> {
    match result {
        Ok(value) => serde_json::from_value(value).map_err(|error| {
            JsErrorBox::generic(format!(
                "node_compat subprocess result should deserialize: {error}"
            ))
        }),
        Err(error) => Ok(RuntimeTestSpawnResult {
            pid: 0,
            code: 1,
            stdout: String::new(),
            stderr: format!("{error}\n"),
            signal: None,
        }),
    }
}

pub(super) fn runtime_test_spawn_envelope(
    result: RuntimeTestSpawnResult,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::to_value(result)
            .map_err(|error| JsErrorBox::generic(error.to_string()))?,
    })
}
