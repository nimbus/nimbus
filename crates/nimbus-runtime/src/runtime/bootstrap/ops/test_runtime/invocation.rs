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
    let (tempdir, bundle_path, file_output_syncs) = write_runtime_test_spawn_bundle(&plan)?;
    let output_path_rewrites = if let Some(source_bundle_root) = plan.source_bundle_root.as_ref()
        && !plan.permission_restricted
        && let Some(bundle_root) = bundle_path.parent()
    {
        vec![(bundle_root.to_path_buf(), source_bundle_root.clone())]
    } else {
        Vec::new()
    };
    let mut limits = contract.limits;
    if let Some(source_bundle_root) = plan.source_bundle_root.as_ref()
        && !plan.permission_restricted
    {
        limits
            .grants
            .read
            .push(source_bundle_root.to_string_lossy().into_owned());
    }
    let runtime = NimbusRuntime::with_policy(host, Arc::new(RuntimePolicy::new(limits)));
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
        output_path_rewrites,
        request,
        process_state_snapshot,
    })
}

pub(super) fn runtime_test_spawn_result_from_value(
    result: crate::error::Result<serde_json::Value>,
) -> std::result::Result<RuntimeTestSpawnResult, JsErrorBox> {
    match result {
        Ok(value) => {
            let mut result: RuntimeTestSpawnResult =
                serde_json::from_value(value).map_err(|error| {
                    JsErrorBox::generic(format!(
                        "node_compat subprocess result should deserialize: {error}"
                    ))
                })?;
            result.stderr = normalize_subprocess_javascript_stderr(&result.stderr);
            Ok(result)
        }
        Err(error) => {
            // A child runtime that throws an uncaught JS exception must surface
            // Node-identical stderr (`Error: <msg>`), not the Nimbus-internal
            // `runtime JavaScript error: ` Display prefix that engine/host trace
            // snapshots and `classify_runtime_error` rely on. Strip the prefix
            // for the JavaScript variant only, by formatting its inner message;
            // every other variant keeps its full Display so timeout / heap /
            // capability / integrity diagnostics are unchanged.
            let stderr = match error {
                crate::error::NimbusRuntimeError::JavaScript(message) => {
                    format!("{}\n", normalize_subprocess_javascript_stderr(&message))
                }
                other => format!("{other}\n"),
            };
            Ok(RuntimeTestSpawnResult {
                pid: 0,
                code: 1,
                stdout: String::new(),
                stderr,
                signal: None,
            })
        }
    }
}

pub(super) fn normalize_runtime_test_spawn_result_paths(
    result: &mut RuntimeTestSpawnResult,
    rewrites: &[(PathBuf, PathBuf)],
) {
    for (from, to) in rewrites {
        let from = from.to_string_lossy();
        let to = to.to_string_lossy();
        result.stdout = result.stdout.replace(from.as_ref(), to.as_ref());
        result.stderr = result.stderr.replace(from.as_ref(), to.as_ref());
    }
}

fn normalize_subprocess_javascript_stderr(message: &str) -> String {
    let mut removed_internal_frame = false;
    let mut normalized = Vec::new();
    for line in message.lines() {
        if is_internal_subprocess_stack_frame(line) {
            removed_internal_frame = true;
            continue;
        }
        normalized.push(line);
    }
    if !removed_internal_frame {
        return message.to_string();
    }
    let mut normalized = normalized.join("\n");
    if message.ends_with('\n') {
        normalized.push('\n');
    }
    normalized
}

fn is_internal_subprocess_stack_frame(line: &str) -> bool {
    line.contains(" at __drainNextTickAndMacrotasks (ext:core/01_core.js:")
        || line.contains(" at <nimbus-runtime:invoke>:")
}

pub(super) fn runtime_test_spawn_envelope(
    result: RuntimeTestSpawnResult,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::to_value(result)
            .map_err(|error| JsErrorBox::generic(error.to_string()))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::NimbusRuntimeError;
    use std::time::Duration;

    #[test]
    fn javascript_error_stderr_matches_node_uncaught_shape() {
        // A thrown `Error: test_callback` in the child must surface as Node does
        // (`Error: test_callback`), without the Nimbus `runtime JavaScript error: `
        // Display prefix — this is what test/async-hooks/test-callback-error.js
        // asserts on `child.stderr.split(/[\r\n]+/g)[0]`.
        let result = runtime_test_spawn_result_from_value(Err(NimbusRuntimeError::JavaScript(
            "Error: test_callback".to_string(),
        )))
        .expect("error arm always yields a spawn result");
        let first_line = result.stderr.split(['\r', '\n']).next().unwrap_or_default();
        assert_eq!(first_line, "Error: test_callback");
        assert_eq!(result.code, 1);
    }

    #[test]
    fn non_javascript_error_stderr_keeps_full_display() {
        // Non-JS variants must retain their full Display so timeout / heap /
        // capability / integrity diagnostics are not silently truncated.
        let result = runtime_test_spawn_result_from_value(Err(
            NimbusRuntimeError::ExecutionTimeout(Duration::from_secs(5)),
        ))
        .expect("error arm always yields a spawn result");
        assert!(
            result
                .stderr
                .starts_with("runtime execution timed out after"),
            "timeout diagnostic must keep its prefix, got: {:?}",
            result.stderr
        );
    }

    #[test]
    fn javascript_error_message_containing_prefix_phrase_is_untouched() {
        // Variant-matched, not a blind string trim: a user message that happens
        // to contain the phrase keeps its own text verbatim after the strip.
        let result = runtime_test_spawn_result_from_value(Err(NimbusRuntimeError::JavaScript(
            "Error: runtime JavaScript error: nested".to_string(),
        )))
        .expect("error arm always yields a spawn result");
        let first_line = result.stderr.split(['\r', '\n']).next().unwrap_or_default();
        assert_eq!(first_line, "Error: runtime JavaScript error: nested");
    }

    #[test]
    fn javascript_error_stderr_drops_internal_runtime_drain_frame() {
        let result = runtime_test_spawn_result_from_value(Err(NimbusRuntimeError::JavaScript(
            "Error: child\n    at child.js:1:1\n    at __drainNextTickAndMacrotasks (ext:core/01_core.js:507:5)\n    at user.js:2:1\n    at <nimbus-runtime:invoke>:1:12".to_string(),
        )))
        .expect("error arm always yields a spawn result");
        assert_eq!(
            result.stderr,
            "Error: child\n    at child.js:1:1\n    at user.js:2:1\n"
        );
    }

    #[test]
    fn successful_spawn_result_stderr_drops_internal_runtime_drain_frame() {
        let result = runtime_test_spawn_result_from_value(Ok(serde_json::json!({
            "pid": 0,
            "code": 1,
            "stdout": "",
            "stderr": "Error: child\n    at child.js:1:1\n    at __drainNextTickAndMacrotasks (ext:core/01_core.js:507:5)\n    at <nimbus-runtime:invoke>:1:12\n",
            "signal": null,
        })))
        .expect("spawn result should deserialize");
        assert_eq!(result.stderr, "Error: child\n    at child.js:1:1\n");
    }

    #[test]
    fn successful_spawn_result_stderr_without_runtime_frame_stays_verbatim() {
        let result = runtime_test_spawn_result_from_value(Ok(serde_json::json!({
            "pid": 0,
            "code": 1,
            "stdout": "",
            "stderr": "Error: child\n",
            "signal": null,
        })))
        .expect("spawn result should deserialize");
        assert_eq!(result.stderr, "Error: child\n");
    }

    #[test]
    fn spawn_result_paths_normalize_child_bundle_root_to_parent_root() {
        let mut result = RuntimeTestSpawnResult {
            pid: 0,
            code: 1,
            stdout: "/private/var/folders/tmp/app/.nimbus/convex/test/out.js\n".to_string(),
            stderr: "Error: /private/var/folders/tmp/app/.nimbus/convex/test/child.js\n"
                .to_string(),
            signal: None,
        };

        normalize_runtime_test_spawn_result_paths(
            &mut result,
            &[(
                PathBuf::from("/private/var/folders/tmp/app/.nimbus/convex"),
                PathBuf::from("/private/tmp/nvx-parent/app/.nimbus/convex"),
            )],
        );

        assert_eq!(
            result.stdout,
            "/private/tmp/nvx-parent/app/.nimbus/convex/test/out.js\n"
        );
        assert_eq!(
            result.stderr,
            "Error: /private/tmp/nvx-parent/app/.nimbus/convex/test/child.js\n"
        );
    }
}
