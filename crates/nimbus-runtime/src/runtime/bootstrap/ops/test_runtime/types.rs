use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::InvocationRequest;
use crate::backends::v8::embedder::JsErrorBox;
use crate::runtime::NimbusRuntime;

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeTestSpawnPayload {
    pub(super) command: String,
    #[serde(default)]
    pub(super) args: Vec<String>,
    #[serde(default)]
    pub(super) cwd: Option<String>,
    #[serde(default)]
    pub(super) env: Option<BTreeMap<String, String>>,
    #[serde(default, rename = "stdinBase64")]
    pub(super) stdin_base64: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RuntimeTestSpawnResult {
    pub(super) pid: u32,
    pub(super) code: i32,
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) signal: Option<String>,
}

pub(super) enum RuntimeTestSpawnMode {
    Eval {
        source: String,
        print_result: bool,
        input_type_module: bool,
    },
    LegacyInspectorFlagError,
    Script {
        script_path: PathBuf,
        relative_path: Option<String>,
        source: Option<String>,
        cli_args: Vec<String>,
    },
    TestRunner {
        file_patterns: Vec<String>,
        reporters: Vec<String>,
        reporter_destinations: Vec<String>,
        concurrency: Option<u32>,
        timeout: Option<u32>,
        isolation: RuntimeTestRunnerIsolation,
        randomize: bool,
        random_seed: Option<u32>,
        watch: bool,
        rerun_failures_file: Option<String>,
    },
}

#[derive(Clone, Copy)]
pub(super) enum RuntimeTestRunnerIsolation {
    Process,
    None,
}

pub(super) struct RuntimeTestInspectorOpen {
    pub(super) port: Option<u16>,
    pub(super) wait_for_session: bool,
}

pub(super) struct RuntimeTestSpawnPlan {
    pub(super) command: String,
    pub(super) mode: RuntimeTestSpawnMode,
    pub(super) cwd: Option<PathBuf>,
    pub(super) env: Option<BTreeMap<String, String>>,
    pub(super) stdin_bytes: Option<Vec<u8>>,
    pub(super) exec_argv: Vec<String>,
    pub(super) source_bundle_root: Option<PathBuf>,
    pub(super) preload_env_file: Option<PathBuf>,
    pub(super) permission_restricted: bool,
    pub(super) process_title: Option<String>,
    pub(super) expose_gc: bool,
    pub(super) inspector_open: Option<RuntimeTestInspectorOpen>,
}

pub(super) struct PreparedRuntimeTestSpawnInvocation {
    pub(super) _tempdir: tempfile::TempDir,
    pub(super) runtime: NimbusRuntime,
    pub(super) bundle_path: PathBuf,
    pub(super) file_output_syncs: Vec<(PathBuf, PathBuf)>,
    pub(super) request: InvocationRequest,
    pub(super) process_state_snapshot: RuntimeTestProcessStateSnapshot,
}

pub(super) struct RuntimeTestProcessStateSnapshot {
    cwd: Option<PathBuf>,
    env: Vec<(OsString, OsString)>,
}

impl RuntimeTestProcessStateSnapshot {
    pub(super) fn capture() -> Self {
        Self {
            cwd: std::env::current_dir().ok(),
            env: std::env::vars_os().collect(),
        }
    }

    pub(super) fn restore(&self) -> std::result::Result<(), JsErrorBox> {
        let current_keys = std::env::vars_os()
            .map(|(key, _)| key)
            .collect::<Vec<OsString>>();
        for key in current_keys {
            if self.env.iter().any(|(saved_key, _)| saved_key == &key) {
                continue;
            }
            // SAFETY: the node_compat subprocess helper runs in the
            // single-threaded test harness path and must restore host process
            // env mutations performed by Deno's process.loadEnvFile().
            unsafe {
                std::env::remove_var(&key);
            }
        }
        for (key, value) in &self.env {
            // SAFETY: see note above; restoring the captured env snapshot is
            // required to keep manifested fixture batches isolated.
            unsafe {
                std::env::set_var(key, value);
            }
        }
        if let Some(cwd) = &self.cwd {
            std::env::set_current_dir(cwd).map_err(|error| {
                JsErrorBox::generic(format!(
                    "node_compat subprocess helper should restore cwd {}: {error}",
                    cwd.display()
                ))
            })?;
        }
        Ok(())
    }
}
