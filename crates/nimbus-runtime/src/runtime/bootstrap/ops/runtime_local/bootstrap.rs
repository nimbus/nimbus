use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::runtime::bootstrap::state::{
    InstalledRuntimeCapabilityPolicy, InstalledRuntimeContract,
};

use super::support::{capability_denied_error, runtime_target_triple};

#[cfg(windows)]
const SYNTHETIC_RUNTIME_EXEC_PATH: &str = r"C:\nimbus\runtime\node.exe";
#[cfg(not(windows))]
const SYNTHETIC_RUNTIME_EXEC_PATH: &str = "/nimbus/runtime/node";

#[op2(fast)]
pub(in super::super) fn op_bootstrap_color_depth(_state: &mut OpState) -> i32 {
    // Nimbus runtimes do not own an interactive terminal surface today, so we
    // report the most conservative color capability until a grant-scoped
    // stdio contract exists.
    1
}

#[op2]
pub(in super::super) fn op_bootstrap_unstable_args(_state: &mut OpState) -> Vec<String> {
    Vec::new()
}

#[op2]
#[string]
pub(in super::super) fn op_nimbus_runtime_exec_path(
    state: &mut OpState,
) -> std::result::Result<String, JsErrorBox> {
    let Some((allows_self_exec, allows_host_exec)) = state
        .try_borrow::<InstalledRuntimeContract>()
        .map(|contract| {
            (
                contract
                    .limits
                    .grants
                    .run
                    .iter()
                    .any(|grant| grant == "$runtime_self_exec"),
                contract
                    .limits
                    .grants
                    .run
                    .iter()
                    .any(|grant| grant == "$runtime_host_exec"),
            )
        })
    else {
        return Ok(SYNTHETIC_RUNTIME_EXEC_PATH.to_string());
    };

    if allows_self_exec {
        let capability_policy = state
            .try_borrow::<InstalledRuntimeCapabilityPolicy>()
            .ok_or_else(|| {
                JsErrorBox::generic("runtime capability policy is not installed".to_string())
            })?;
        return capability_policy
            .paths
            .runtime_self_exec_target()
            .map(|path| path.display().to_string())
            .map_err(|error| JsErrorBox::generic(error.to_string()));
    }

    if !allows_host_exec {
        return Ok(SYNTHETIC_RUNTIME_EXEC_PATH.to_string());
    }

    std::env::current_exe()
        .map_err(|error| {
            JsErrorBox::generic(format!(
                "failed to resolve current executable path: {error}"
            ))
        })
        .map(|path| path.display().to_string())
}

#[op2]
#[string]
pub(in super::super) fn op_nimbus_runtime_target_triple() -> String {
    runtime_target_triple()
}

#[op2(fast)]
pub(in super::super) fn op_set_raw(
    _state: &mut OpState,
    _rid: u32,
    _is_raw: bool,
    _cbreak: bool,
) -> std::result::Result<(), JsErrorBox> {
    Err(capability_denied_error(
        "raw terminal mode is not available inside the Nimbus runtime",
    ))
}

#[op2(fast)]
#[smi]
pub(in super::super) fn op_http_start(
    _state: &mut OpState,
    #[smi] _conn_rid: u32,
) -> std::result::Result<u32, JsErrorBox> {
    Err(capability_denied_error(
        "http connection upgrade APIs are not available inside the Nimbus runtime",
    ))
}
