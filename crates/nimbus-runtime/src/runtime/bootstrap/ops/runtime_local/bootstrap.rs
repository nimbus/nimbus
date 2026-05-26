use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};

use super::support::{capability_denied_error, runtime_target_triple};

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
pub(in super::super) fn op_nimbus_runtime_exec_path() -> std::result::Result<String, JsErrorBox> {
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
