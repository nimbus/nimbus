use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

use crate::InvocationKind;
use crate::InvocationRequest;
use crate::RuntimeBundle;
use crate::RuntimePolicy;
use crate::backends::v8::embedder::{JsErrorBox, OpState, op2, v8};
use crate::runtime::NimbusRuntime;
use crate::runtime::bootstrap::payloads::RuntimeHostCallEnvelope;
use crate::runtime::bootstrap::state::{InstalledRuntimeContract, InstalledRuntimeHostBridge};

#[derive(Debug, Deserialize)]
pub(super) struct RuntimeTestSpawnPayload {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    #[serde(default, rename = "stdinBase64")]
    stdin_base64: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct RuntimeTestSpawnResult {
    pid: u32,
    code: i32,
    stdout: String,
    stderr: String,
    signal: Option<String>,
}

enum RuntimeTestSpawnMode {
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
enum RuntimeTestRunnerIsolation {
    Process,
    None,
}

struct RuntimeTestInspectorOpen {
    port: Option<u16>,
    wait_for_session: bool,
}

struct RuntimeTestSpawnPlan {
    command: String,
    mode: RuntimeTestSpawnMode,
    cwd: Option<PathBuf>,
    env: Option<BTreeMap<String, String>>,
    stdin_bytes: Option<Vec<u8>>,
    exec_argv: Vec<String>,
    source_bundle_root: Option<PathBuf>,
    preload_env_file: Option<PathBuf>,
    permission_restricted: bool,
    process_title: Option<String>,
    expose_gc: bool,
    inspector_open: Option<RuntimeTestInspectorOpen>,
}

struct PreparedRuntimeTestSpawnInvocation {
    _tempdir: tempfile::TempDir,
    runtime: NimbusRuntime,
    bundle_path: PathBuf,
    file_output_syncs: Vec<(PathBuf, PathBuf)>,
    request: InvocationRequest,
    process_state_snapshot: RuntimeTestProcessStateSnapshot,
}

struct RuntimeTestProcessStateSnapshot {
    cwd: Option<PathBuf>,
    env: Vec<(OsString, OsString)>,
}

impl RuntimeTestProcessStateSnapshot {
    fn capture() -> Self {
        Self {
            cwd: std::env::current_dir().ok(),
            env: std::env::vars_os().collect(),
        }
    }

    fn restore(&self) -> std::result::Result<(), JsErrorBox> {
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

fn resolve_runtime_test_spawn_path(path: &Path, cwd: Option<&Path>) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(cwd) = cwd {
        cwd.join(path)
    } else {
        path.to_path_buf()
    }
}

fn runtime_test_spawn_mode(
    payload: RuntimeTestSpawnPayload,
) -> std::result::Result<RuntimeTestSpawnPlan, JsErrorBox> {
    let cwd = payload.cwd.as_ref().map(PathBuf::from);
    let mut source_bundle_root = cwd.as_deref().and_then(runtime_test_bundle_root_from_path);
    let mut eval_source = None;
    let mut eval_print_result = false;
    let mut eval_input_type_module = false;
    let mut script_arg = None;
    let mut script_cli_args = Vec::new();
    let mut preload_env_file = None;
    let mut permission_restricted = false;
    let mut process_title = None;
    let mut exec_argv = Vec::new();
    let mut expose_gc = false;
    let mut inspector_open = None;
    let mut test_mode = false;
    let mut test_file_patterns = Vec::new();
    let mut test_reporters = Vec::new();
    let mut test_reporter_destinations = Vec::new();
    let mut test_concurrency = None;
    let mut test_timeout = None;
    let mut test_isolation = RuntimeTestRunnerIsolation::Process;
    let mut test_randomize = false;
    let mut test_random_seed = None;
    let mut test_watch = false;
    let mut test_rerun_failures_file = None;
    let stdin_bytes = payload
        .stdin_base64
        .as_deref()
        .map(|encoded| {
            BASE64_STANDARD.decode(encoded).map_err(|error| {
                JsErrorBox::generic(format!(
                    "failed to decode node_compat subprocess stdin payload: {error}"
                ))
            })
        })
        .transpose()?;

    let mut index = 0usize;
    let parse_test_random_seed = |value: &str| -> std::result::Result<u32, JsErrorBox> {
        let parsed = value.parse::<u64>().map_err(|error| {
            JsErrorBox::generic(format!(
                "unsupported node_compat subprocess test random seed `{value}`: {error}"
            ))
        })?;
        if parsed > u32::MAX as u64 {
            return Err(JsErrorBox::generic(format!(
                "The value of \"--test-random-seed\" is out of range. It must be >= 0 && <= 4294967295. Received {value}"
            )));
        }
        Ok(parsed as u32)
    };
    while index < payload.args.len() {
        let arg = payload.args[index].as_str();
        match arg {
            "-e" | "--eval" => {
                let source = payload.args.get(index + 1).ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "missing source argument for node_compat subprocess flag `{arg}`"
                    ))
                })?;
                eval_source = Some(source.clone());
                index += 2;
            }
            "-p" | "--print" | "-pe" | "-ep" => {
                let source = payload.args.get(index + 1).ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "missing source argument for node_compat subprocess flag `{arg}`"
                    ))
                })?;
                eval_source = Some(source.clone());
                eval_print_result = true;
                index += 2;
            }
            "--input-type" => {
                let value = payload.args.get(index + 1).ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "missing source argument for node_compat subprocess flag `{arg}`"
                    ))
                })?;
                match value.as_str() {
                    "module" => {
                        eval_input_type_module = true;
                    }
                    "commonjs" => {
                        eval_input_type_module = false;
                    }
                    _ => {
                        return Err(JsErrorBox::generic(format!(
                            "unsupported node_compat subprocess input type `{value}` in arguments {:?}",
                            payload.args
                        )));
                    }
                }
                index += 2;
            }
            "--expose-gc" => {
                expose_gc = true;
                index += 1;
            }
            "--test" => {
                test_mode = true;
                index += 1;
            }
            "--watch" => {
                test_watch = true;
                index += 1;
            }
            "--test-reporter" => {
                test_mode = true;
                let value = payload.args.get(index + 1).ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "missing source argument for node_compat subprocess flag `{arg}`"
                    ))
                })?;
                test_reporters.push(value.clone());
                index += 2;
            }
            "--test-reporter-destination" => {
                test_mode = true;
                let value = payload.args.get(index + 1).ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "missing source argument for node_compat subprocess flag `{arg}`"
                    ))
                })?;
                test_reporter_destinations.push(value.clone());
                index += 2;
            }
            "--test-concurrency" => {
                test_mode = true;
                let value = payload.args.get(index + 1).ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "missing source argument for node_compat subprocess flag `{arg}`"
                    ))
                })?;
                let concurrency = value.parse::<u32>().map_err(|error| {
                    JsErrorBox::generic(format!(
                        "unsupported node_compat subprocess test concurrency `{value}`: {error}"
                    ))
                })?;
                test_concurrency = Some(concurrency);
                index += 2;
            }
            "--test-timeout" => {
                test_mode = true;
                let value = payload.args.get(index + 1).ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "missing source argument for node_compat subprocess flag `{arg}`"
                    ))
                })?;
                let timeout = value.parse::<u32>().map_err(|error| {
                    JsErrorBox::generic(format!(
                        "unsupported node_compat subprocess test timeout `{value}`: {error}"
                    ))
                })?;
                test_timeout = Some(timeout);
                index += 2;
            }
            "--test-randomize" => {
                test_mode = true;
                test_randomize = true;
                index += 1;
            }
            "--test-random-seed" => {
                test_mode = true;
                let value = payload.args.get(index + 1).ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "missing source argument for node_compat subprocess flag `{arg}`"
                    ))
                })?;
                test_random_seed = Some(parse_test_random_seed(value)?);
                index += 2;
            }
            "--test-rerun-failures" => {
                test_mode = true;
                let value = payload.args.get(index + 1).ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "missing source argument for node_compat subprocess flag `{arg}`"
                    ))
                })?;
                test_rerun_failures_file = Some(value.clone());
                index += 2;
            }
            "--test-isolation" => {
                test_mode = true;
                let value = payload.args.get(index + 1).ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "missing source argument for node_compat subprocess flag `{arg}`"
                    ))
                })?;
                test_isolation = match value.as_str() {
                    "process" => RuntimeTestRunnerIsolation::Process,
                    "none" => RuntimeTestRunnerIsolation::None,
                    _ => {
                        return Err(JsErrorBox::generic(format!(
                            "unsupported node_compat subprocess test isolation `{value}`"
                        )));
                    }
                };
                index += 2;
            }
            "--expose-internals"
            | "--no-experimental-sqlite"
            | "--no-warnings"
            | "--no-turbo-fast-api-calls"
            | "--trace-events-enabled" => {
                exec_argv.push(arg.to_string());
                index += 1;
            }
            "--turbo-fast-api-calls" => {
                exec_argv.push(arg.to_string());
                index += 1;
            }
            "--inspect" => {
                inspector_open = Some(RuntimeTestInspectorOpen {
                    port: None,
                    wait_for_session: false,
                });
                index += 1;
            }
            "--inspect-brk" => {
                inspector_open = Some(RuntimeTestInspectorOpen {
                    port: None,
                    wait_for_session: true,
                });
                index += 1;
            }
            "--debug" | "--debug-brk" => {
                return Ok(RuntimeTestSpawnPlan {
                    command: payload.command,
                    mode: RuntimeTestSpawnMode::LegacyInspectorFlagError,
                    cwd,
                    env: payload.env,
                    stdin_bytes,
                    exec_argv,
                    source_bundle_root,
                    preload_env_file,
                    permission_restricted,
                    process_title,
                    expose_gc,
                    inspector_open: None,
                });
            }
            "--permission" | "--experimental-permission" => {
                permission_restricted = true;
                index += 1;
            }
            "--trace-event-categories" | "--trace-event-file-pattern" | "--title" => {
                let value = payload.args.get(index + 1).ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "missing source argument for node_compat subprocess flag `{arg}`"
                    ))
                })?;
                exec_argv.push(arg.to_string());
                exec_argv.push(value.clone());
                if arg == "--title" {
                    process_title = Some(value.clone());
                }
                index += 2;
            }
            _ if arg.starts_with("--inspect=") => {
                let port = arg["--inspect=".len()..].parse::<u16>().map_err(|error| {
                    JsErrorBox::generic(format!(
                        "unsupported node_compat inspector flag `{arg}`: {error}"
                    ))
                })?;
                inspector_open = Some(RuntimeTestInspectorOpen {
                    port: Some(port),
                    wait_for_session: false,
                });
                index += 1;
            }
            _ if arg.starts_with("--inspect-brk=") => {
                let port = arg["--inspect-brk=".len()..]
                    .parse::<u16>()
                    .map_err(|error| {
                        JsErrorBox::generic(format!(
                            "unsupported node_compat inspector flag `{arg}`: {error}"
                        ))
                    })?;
                inspector_open = Some(RuntimeTestInspectorOpen {
                    port: Some(port),
                    wait_for_session: true,
                });
                index += 1;
            }
            _ if arg.starts_with("--trace-event-categories=")
                || arg.starts_with("--trace-event-file-pattern=")
                || arg.starts_with("--title=") =>
            {
                exec_argv.push(arg.to_string());
                if let Some(value) = arg.strip_prefix("--title=") {
                    process_title = Some(value.to_string());
                }
                index += 1;
            }
            _ if arg.starts_with("--env-file=") => {
                preload_env_file = Some(resolve_runtime_test_spawn_path(
                    Path::new(&arg["--env-file=".len()..]),
                    cwd.as_deref(),
                ));
                index += 1;
            }
            _ if arg.starts_with("--input-type=") => {
                let value = &arg["--input-type=".len()..];
                match value {
                    "module" => {
                        eval_input_type_module = true;
                    }
                    "commonjs" => {
                        eval_input_type_module = false;
                    }
                    _ => {
                        return Err(JsErrorBox::generic(format!(
                            "unsupported node_compat subprocess input type `{value}` in arguments {:?}",
                            payload.args
                        )));
                    }
                }
                index += 1;
            }
            _ if arg.starts_with("--test-reporter=") => {
                test_mode = true;
                test_reporters.push(arg["--test-reporter=".len()..].to_string());
                index += 1;
            }
            _ if arg.starts_with("--test-reporter-destination=") => {
                test_mode = true;
                test_reporter_destinations
                    .push(arg["--test-reporter-destination=".len()..].to_string());
                index += 1;
            }
            _ if arg.starts_with("--test-concurrency=") => {
                test_mode = true;
                let value = &arg["--test-concurrency=".len()..];
                let concurrency = value.parse::<u32>().map_err(|error| {
                    JsErrorBox::generic(format!(
                        "unsupported node_compat subprocess test concurrency `{value}`: {error}"
                    ))
                })?;
                test_concurrency = Some(concurrency);
                index += 1;
            }
            _ if arg.starts_with("--test-timeout=") => {
                test_mode = true;
                let value = &arg["--test-timeout=".len()..];
                let timeout = value.parse::<u32>().map_err(|error| {
                    JsErrorBox::generic(format!(
                        "unsupported node_compat subprocess test timeout `{value}`: {error}"
                    ))
                })?;
                test_timeout = Some(timeout);
                index += 1;
            }
            _ if arg.starts_with("--test-random-seed=") => {
                test_mode = true;
                let value = &arg["--test-random-seed=".len()..];
                test_random_seed = Some(parse_test_random_seed(value)?);
                index += 1;
            }
            _ if arg.starts_with("--test-isolation=") => {
                test_mode = true;
                let value = &arg["--test-isolation=".len()..];
                test_isolation = match value {
                    "process" => RuntimeTestRunnerIsolation::Process,
                    "none" => RuntimeTestRunnerIsolation::None,
                    _ => {
                        return Err(JsErrorBox::generic(format!(
                            "unsupported node_compat subprocess test isolation `{value}`"
                        )));
                    }
                };
                index += 1;
            }
            _ if arg.starts_with('-') => {
                return Err(JsErrorBox::generic(format!(
                    "unsupported node_compat subprocess flag `{arg}` in arguments {:?}",
                    payload.args
                )));
            }
            _ => {
                if test_mode {
                    test_file_patterns.push(arg.to_string());
                    index += 1;
                } else {
                    script_arg = Some(arg.to_string());
                    script_cli_args = payload.args[index + 1..].to_vec();
                    break;
                }
            }
        }
    }

    if test_mode && source_bundle_root.is_none() {
        source_bundle_root = test_file_patterns
            .iter()
            .find_map(|pattern| runtime_test_bundle_root_from_argument(pattern))
            .or_else(|| {
                test_reporters
                    .iter()
                    .find_map(|specifier| runtime_test_bundle_root_from_argument(specifier))
            })
            .or_else(|| {
                test_reporter_destinations
                    .iter()
                    .find_map(|destination| runtime_test_bundle_root_from_argument(destination))
            });
    }

    let mode = if let Some(source) = eval_source {
        RuntimeTestSpawnMode::Eval {
            source,
            print_result: eval_print_result,
            input_type_module: eval_input_type_module,
        }
    } else if test_mode {
        RuntimeTestSpawnMode::TestRunner {
            file_patterns: test_file_patterns,
            reporters: test_reporters,
            reporter_destinations: test_reporter_destinations,
            concurrency: test_concurrency,
            timeout: test_timeout,
            isolation: test_isolation,
            randomize: test_randomize,
            random_seed: test_random_seed,
            watch: test_watch,
            rerun_failures_file: test_rerun_failures_file,
        }
    } else if let Some(script_arg) = script_arg {
        let script_path = resolve_runtime_test_spawn_path(Path::new(&script_arg), cwd.as_deref());
        if source_bundle_root.is_none() {
            source_bundle_root = runtime_test_bundle_root_from_path(&script_path);
        }
        let (relative_path, source) = if script_path.is_file() {
            let source = std::fs::read_to_string(&script_path).map_err(|error| {
                JsErrorBox::generic(format!(
                    "failed to read node_compat subprocess script {}: {error}",
                    script_path.display()
                ))
            })?;
            let relative_path =
                relative_test_path(&script_path).map(|path| path.to_string_lossy().into_owned());
            (relative_path, Some(source))
        } else {
            (None, None)
        };
        RuntimeTestSpawnMode::Script {
            script_path,
            relative_path,
            source,
            cli_args: script_cli_args,
        }
    } else {
        return Err(JsErrorBox::generic(format!(
            "unsupported node_compat subprocess arguments: {:?}",
            payload.args
        )));
    };

    Ok(RuntimeTestSpawnPlan {
        command: payload.command,
        mode,
        cwd,
        env: payload.env,
        stdin_bytes,
        exec_argv,
        source_bundle_root,
        preload_env_file,
        permission_restricted,
        process_title,
        expose_gc,
        inspector_open,
    })
}

fn relative_test_path(path: &Path) -> Option<PathBuf> {
    let components = path.components().collect::<Vec<_>>();
    let start = components
        .iter()
        .position(|component| component.as_os_str() == "test")?;
    let mut relative = PathBuf::new();
    for component in &components[start..] {
        relative.push(component.as_os_str());
    }
    Some(relative)
}

fn runtime_test_bundle_root_from_path(path: &Path) -> Option<PathBuf> {
    let components = path.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        if component.as_os_str() == "convex"
            && index >= 2
            && components[index - 1].as_os_str() == ".nimbus"
            && components[index - 2].as_os_str() == "app"
        {
            let mut root = PathBuf::new();
            for component in &components[..=index] {
                root.push(component.as_os_str());
            }
            return Some(root);
        }
    }
    None
}

fn runtime_test_bundle_root_from_argument(argument: &str) -> Option<PathBuf> {
    if let Some(path) = argument.strip_prefix("file://") {
        return runtime_test_bundle_root_from_path(Path::new(path));
    }
    let path = Path::new(argument);
    if path.is_absolute() {
        return runtime_test_bundle_root_from_path(path);
    }
    None
}

fn copy_dir_recursive(source: &Path, destination: &Path) -> std::result::Result<(), JsErrorBox> {
    std::fs::create_dir_all(destination).map_err(|error| {
        JsErrorBox::generic(format!(
            "node_compat subprocess copy should create {}: {error}",
            destination.display()
        ))
    })?;

    for entry in std::fs::read_dir(source).map_err(|error| {
        JsErrorBox::generic(format!(
            "node_compat subprocess copy should read {}: {error}",
            source.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            JsErrorBox::generic(format!(
                "node_compat subprocess copy should enumerate {}: {error}",
                source.display()
            ))
        })?;
        let entry_path = entry.path();
        let target_path = destination.join(entry.file_name());
        let file_type = entry.file_type().map_err(|error| {
            JsErrorBox::generic(format!(
                "node_compat subprocess copy should stat {}: {error}",
                entry_path.display()
            ))
        })?;
        if file_type.is_dir() {
            copy_dir_recursive(&entry_path, &target_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&entry_path, &target_path).map_err(|error| {
                JsErrorBox::generic(format!(
                    "node_compat subprocess copy should copy {} -> {}: {error}",
                    entry_path.display(),
                    target_path.display()
                ))
            })?;
        }
    }

    Ok(())
}

fn rewrite_bundle_string(value: &str, source_root: &Path, target_root: &Path) -> String {
    value.replace(
        source_root.to_string_lossy().as_ref(),
        target_root.to_string_lossy().as_ref(),
    )
}

fn rewrite_bundle_path(candidate: &Path, source_root: &Path, target_root: &Path) -> PathBuf {
    let canonical_candidate =
        std::fs::canonicalize(candidate).unwrap_or_else(|_| candidate.to_path_buf());
    let canonical_source_root =
        std::fs::canonicalize(source_root).unwrap_or_else(|_| source_root.to_path_buf());
    if let Ok(relative) = canonical_candidate.strip_prefix(&canonical_source_root) {
        return target_root.join(relative);
    }
    PathBuf::from(rewrite_bundle_string(
        candidate.to_string_lossy().as_ref(),
        source_root,
        target_root,
    ))
}

fn rewrite_bundle_env(
    env: &BTreeMap<String, String>,
    source_root: &Path,
    target_root: &Path,
) -> BTreeMap<String, String> {
    env.iter()
        .map(|(key, value)| {
            (
                key.clone(),
                rewrite_bundle_string(value, source_root, target_root),
            )
        })
        .collect()
}

fn rewrite_bundle_command(command: &str, source_root: &Path, target_root: &Path) -> String {
    let command_path = Path::new(command);
    if command_path.is_absolute() {
        rewrite_bundle_path(command_path, source_root, target_root)
            .to_string_lossy()
            .into_owned()
    } else {
        rewrite_bundle_string(command, source_root, target_root)
    }
}

fn runtime_test_spawn_file_output_syncs(
    plan: &RuntimeTestSpawnPlan,
    bundle_root: &Path,
) -> Vec<RuntimeTestSpawnFileOutputSync> {
    let RuntimeTestSpawnMode::TestRunner {
        reporter_destinations,
        rerun_failures_file,
        ..
    } = &plan.mode
    else {
        return Vec::new();
    };
    let Some(source_bundle_root) = plan.source_bundle_root.as_deref() else {
        return Vec::new();
    };
    if plan.permission_restricted {
        return Vec::new();
    }

    let mut syncs = reporter_destinations
        .iter()
        .filter(|destination| destination.as_str() != "stdout" && destination.as_str() != "stderr")
        .filter_map(|destination| {
            let original = PathBuf::from(destination);
            if !original.is_absolute() {
                return None;
            }
            Some((
                original.clone(),
                rewrite_bundle_path(&original, source_bundle_root, bundle_root),
            ))
        })
        .collect::<Vec<_>>();

    if let Some(rerun_failures_file) = rerun_failures_file {
        let original = PathBuf::from(rerun_failures_file);
        if original.is_absolute() {
            syncs.push((
                original.clone(),
                rewrite_bundle_path(&original, source_bundle_root, bundle_root),
            ));
        }
    }

    syncs
}

type RuntimeTestSpawnFileOutputSync = (PathBuf, PathBuf);
type RuntimeTestSpawnBundle = (
    tempfile::TempDir,
    PathBuf,
    Vec<RuntimeTestSpawnFileOutputSync>,
);

fn sync_runtime_test_spawn_file_outputs(
    syncs: &[RuntimeTestSpawnFileOutputSync],
) -> std::result::Result<(), JsErrorBox> {
    for (original_path, rewritten_path) in syncs {
        if !rewritten_path.exists() {
            continue;
        }
        if let Some(parent) = original_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                JsErrorBox::generic(format!(
                    "node_compat subprocess output sync should create {}: {error}",
                    parent.display()
                ))
            })?;
        }
        std::fs::copy(rewritten_path, original_path).map_err(|error| {
            JsErrorBox::generic(format!(
                "node_compat subprocess output sync should copy {} -> {}: {error}",
                rewritten_path.display(),
                original_path.display()
            ))
        })?;
    }
    Ok(())
}

fn write_runtime_test_spawn_bundle(
    plan: &RuntimeTestSpawnPlan,
) -> std::result::Result<RuntimeTestSpawnBundle, JsErrorBox> {
    let tempdir = tempdir().map_err(|error| {
        JsErrorBox::generic(format!("node_compat tempdir should build: {error}"))
    })?;
    let bundle_dir = tempdir.path().join("app/.nimbus/convex");
    std::fs::create_dir_all(&bundle_dir).map_err(|error| {
        JsErrorBox::generic(format!(
            "node_compat spawn bundle dir should build: {error}"
        ))
    })?;
    let bundle_dir = std::fs::canonicalize(&bundle_dir).unwrap_or(bundle_dir);
    let file_output_syncs = runtime_test_spawn_file_output_syncs(plan, &bundle_dir);
    let rendered_command = if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
        if plan.permission_restricted {
            plan.command.clone()
        } else {
            rewrite_bundle_command(&plan.command, source_bundle_root, &bundle_dir)
        }
    } else {
        plan.command.clone()
    };

    if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
        copy_dir_recursive(source_bundle_root, &bundle_dir)?;
    }

    if let Some(cwd) = plan.cwd.as_deref() {
        let cwd_node_modules = cwd.join("node_modules");
        let bundle_node_modules = bundle_dir.join("node_modules");
        if cwd_node_modules.is_dir() && !bundle_node_modules.exists() {
            // Node's eval children resolve bare packages from the caller's cwd.
            // The compat subprocess bundle executes from its own temp root, so
            // mirror the caller's node_modules tree when present to preserve
            // that package-resolution contract.
            copy_dir_recursive(&cwd_node_modules, &bundle_node_modules)?;
        }
    }

    let stdin_setup = if let Some(stdin_bytes) = plan.stdin_bytes.as_ref() {
        let stdin_path = bundle_dir.join("test/fixtures/nimbus-stdin.bin");
        if let Some(parent) = stdin_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                JsErrorBox::generic(format!(
                    "node_compat subprocess stdin fixture dir should build: {error}"
                ))
            })?;
        }
        std::fs::write(&stdin_path, stdin_bytes).map_err(|error| {
            JsErrorBox::generic(format!(
                "node_compat subprocess stdin fixture should write: {error}"
            ))
        })?;
        let rendered_stdin_path = stdin_path.to_string_lossy().into_owned();
        format!(
            "globalThis.__nimbusRuntimeTestStdinPath = {};",
            serde_json::to_string(&rendered_stdin_path).expect("stdin path should serialize")
        )
    } else {
        "delete globalThis.__nimbusRuntimeTestStdinPath;".to_string()
    };

    let execution = match &plan.mode {
        RuntimeTestSpawnMode::Eval {
            source,
            print_result,
            input_type_module,
        } => {
            let rendered_source =
                if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
                    if plan.permission_restricted {
                        source.clone()
                    } else {
                        rewrite_bundle_string(source, source_bundle_root, &bundle_dir)
                    }
                } else {
                    source.clone()
                };
            if *input_type_module {
                let eval_module_path = bundle_dir.join("__nimbus_eval__.mjs");
                std::fs::write(&eval_module_path, &rendered_source).map_err(|error| {
                    JsErrorBox::generic(format!(
                        "node_compat eval module should write: {error}"
                    ))
                })?;
                format!(
                    r#"
    const __nimbusEvalModuleUrl = require("node:url").pathToFileURL({}).href;
    const __nimbusEvalResult = await import(__nimbusEvalModuleUrl);
{}"#,
                    serde_json::to_string(&eval_module_path.to_string_lossy().into_owned())
                        .expect("eval module path should serialize"),
                    if *print_result {
                        r#"
    if (__nimbusEvalResult !== undefined) {
      stdout += `${captureChunk(__nimbusEvalResult)}
`;
    }
"#
                    } else {
                        ""
                    }
                )
            } else {
                let eval_require_base_path = plan
                    .cwd
                    .as_deref()
                    .map(|cwd| {
                        let base_path = cwd.join("$deno$eval.cjs");
                        if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
                            if plan.permission_restricted {
                                base_path
                            } else {
                                rewrite_bundle_path(&base_path, source_bundle_root, &bundle_dir)
                            }
                        } else {
                            base_path
                        }
                    })
                    .unwrap_or_else(|| bundle_dir.join("$deno$eval.cjs"))
                    .to_string_lossy()
                    .into_owned();
                format!(
                    r#"
    const __nimbusEvalSource = {source};
    const __nimbusEvalFilename = {filename};
    const __nimbusEvalDirname = require("node:path").dirname(__nimbusEvalFilename);
    const __nimbusEvalRequire = require("node:module").createRequire(__nimbusEvalFilename);
    const __nimbusEvalModule = {{
      exports: {{}},
      filename: __nimbusEvalFilename,
      path: __nimbusEvalDirname,
      paths: require("node:module")._nodeModulePaths(__nimbusEvalDirname),
    }};
    let __nimbusEvalResult = ((require, module, exports, __filename, __dirname) => eval(__nimbusEvalSource))(
      __nimbusEvalRequire,
      __nimbusEvalModule,
      __nimbusEvalModule.exports,
      __nimbusEvalFilename,
      __nimbusEvalDirname,
    );
    if (
      __nimbusEvalResult &&
      typeof __nimbusEvalResult.then === "function"
    ) {{
      __nimbusEvalResult = await __nimbusEvalResult;
    }}
{print_result_block}"#,
                    source =
                        serde_json::to_string(&rendered_source).expect("eval source should serialize"),
                    filename = serde_json::to_string(&eval_require_base_path)
                        .expect("eval require base path should serialize"),
                    print_result_block = if *print_result {
                        r#"
    if (__nimbusEvalResult !== undefined) {
      stdout += `${captureChunk(__nimbusEvalResult)}
`;
    }
"#
                    } else {
                        ""
                    }
                )
            }
        }
        RuntimeTestSpawnMode::LegacyInspectorFlagError => r#"
    code = 9;
    stderr += "`node --debug` and `node --debug-brk` are invalid. Please use `node --inspect` and `node --inspect-brk` instead.\n";
"#
        .to_string(),
        RuntimeTestSpawnMode::TestRunner {
            file_patterns,
            reporters,
            reporter_destinations,
            concurrency,
            timeout,
            isolation,
            randomize,
            random_seed,
            watch,
            rerun_failures_file,
        } => {
            let rewrite_string = |value: &str| {
                if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
                    if plan.permission_restricted {
                        value.to_string()
                    } else {
                        rewrite_bundle_string(value, source_bundle_root, &bundle_dir)
                    }
                } else {
                    value.to_string()
                }
            };
            let rendered_file_patterns = file_patterns
                .iter()
                .map(|pattern| rewrite_string(pattern))
                .collect::<Vec<_>>();
            let rendered_reporters = reporters
                .iter()
                .map(|specifier| rewrite_string(specifier))
                .collect::<Vec<_>>();
            let rendered_reporter_destinations = reporter_destinations
                .iter()
                .map(|destination| rewrite_string(destination))
                .collect::<Vec<_>>();
            let rendered_rerun_failures_file = rerun_failures_file
                .as_deref()
                .map(rewrite_string);
            let rendered_cwd = plan.cwd.as_deref().map(|cwd| {
                if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
                    if plan.permission_restricted {
                        cwd.to_path_buf()
                    } else {
                        rewrite_bundle_path(cwd, source_bundle_root, &bundle_dir)
                    }
                } else {
                    cwd.to_path_buf()
                }
            });
            let expand_test_pattern = |pattern: &str| -> std::result::Result<Vec<String>, JsErrorBox> {
                if !pattern.contains('*') {
                    let path = PathBuf::from(pattern);
                    let resolved = if path.is_absolute() {
                        path
                    } else if let Some(cwd) = rendered_cwd.as_deref() {
                        cwd.join(path)
                    } else {
                        path
                    };
                    return Ok(vec![resolved.to_string_lossy().into_owned()]);
                }

                let path = PathBuf::from(pattern);
                let resolved = if path.is_absolute() {
                    path
                } else if let Some(cwd) = rendered_cwd.as_deref() {
                    cwd.join(path)
                } else {
                    path
                };
                let parent = resolved.parent().ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "node_compat test runner glob should have a parent directory: {}",
                        resolved.display()
                    ))
                })?;
                let file_pattern = resolved
                    .file_name()
                    .and_then(|value| value.to_str())
                    .ok_or_else(|| {
                        JsErrorBox::generic(format!(
                            "node_compat test runner glob should have a UTF-8 filename: {}",
                            resolved.display()
                        ))
                    })?;
                let wildcard_index = file_pattern.find('*').ok_or_else(|| {
                    JsErrorBox::generic(format!(
                        "node_compat test runner glob should include a wildcard: {}",
                        resolved.display()
                    ))
                })?;
                let prefix = &file_pattern[..wildcard_index];
                let suffix = &file_pattern[wildcard_index + 1..];
                let mut matches = std::fs::read_dir(parent)
                    .map_err(|error| {
                        JsErrorBox::generic(format!(
                            "node_compat test runner glob should read {}: {error}",
                            parent.display()
                        ))
                    })?
                    .filter_map(|entry| entry.ok())
                    .filter_map(|entry| {
                        let file_type = entry.file_type().ok()?;
                        if !file_type.is_file() {
                            return None;
                        }
                        let file_name = entry.file_name();
                        let file_name = file_name.to_str()?;
                        if file_name.starts_with(prefix) && file_name.ends_with(suffix) {
                            Some(entry.path().to_string_lossy().into_owned())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                matches.sort();
                Ok(matches)
            };
            let mut rendered_test_files = Vec::new();
            for pattern in &rendered_file_patterns {
                rendered_test_files.extend(expand_test_pattern(pattern)?);
            }
            format!(
                r#"
    const __nimbusAppendHandledError = (error, fallbackCode = 1) => {{
      code = fallbackCode;
      const rendered = typeof error?.stack === "string" ? error.stack : String(error);
      const renderedCode =
        typeof error?.code === "string" && error.code.length > 0 ? error.code : null;
      if (stderr.length > 0 && !stderr.endsWith("\n")) {{
        stderr += "\n";
      }}
      if (renderedCode && !rendered.includes(renderedCode)) {{
        stderr += `${{renderedCode}}\n`;
      }}
      stderr += `${{rendered}}\n`;
    }};

    const __nimbusAppendReporterUnhandledError = (error) => {{
      code = 7;
      const rendered = typeof error?.stack === "string" ? error.stack : String(error);
      if (stderr.length > 0 && !stderr.endsWith("\n")) {{
        stderr += "\n";
      }}
      stderr += `${{rendered}}\n`;
      if (!stderr.includes("Emitted 'error' event on Duplex instance")) {{
        stderr += "Emitted 'error' event on Duplex instance at:\n";
      }}
    }};

    const __nimbusReporterRequire = require("node:module").createRequire(
      require("node:path").resolve(process.cwd(), "__nimbus-test-runner__.js"),
    );
    const __nimbusUtilFormat = require("node:util").format;
    const __nimbusMatchesNodeDebugPattern = (pattern, setName) => {{
      const normalizedPattern = String(pattern ?? "").trim().toUpperCase();
      const normalizedSetName = String(setName ?? "").toUpperCase();
      let patternIndex = 0;
      let setIndex = 0;
      let starIndex = -1;
      let resumeIndex = 0;
      while (setIndex < normalizedSetName.length) {{
        if (
          patternIndex < normalizedPattern.length &&
          normalizedPattern[patternIndex] === normalizedSetName[setIndex]
        ) {{
          patternIndex += 1;
          setIndex += 1;
          continue;
        }}
        if (
          patternIndex < normalizedPattern.length &&
          normalizedPattern[patternIndex] === "*"
        ) {{
          starIndex = patternIndex;
          patternIndex += 1;
          resumeIndex = setIndex;
          continue;
        }}
        if (starIndex !== -1) {{
          patternIndex = starIndex + 1;
          resumeIndex += 1;
          setIndex = resumeIndex;
          continue;
        }}
        return false;
      }}
      while (
        patternIndex < normalizedPattern.length &&
        normalizedPattern[patternIndex] === "*"
      ) {{
        patternIndex += 1;
      }}
      return patternIndex === normalizedPattern.length;
    }};
    const __nimbusTestRunnerDebugEnabled = (() => {{
      const debugEnv = process?.env?.NODE_DEBUG;
      if (typeof debugEnv !== "string" || debugEnv.length === 0) {{
        return false;
      }}
      return debugEnv
        .split(",")
        .some((pattern) => __nimbusMatchesNodeDebugPattern(pattern, "TEST_RUNNER"));
    }})();
    const __nimbusTestRunnerDebug = (...args) => {{
      if (!__nimbusTestRunnerDebugEnabled) {{
        return;
      }}
      process.stderr.write(
        `TEST_RUNNER ${{process.pid}}: ${{__nimbusUtilFormat(...args)}}\n`,
      );
    }};
    const __nimbusRequestedConcurrency = {concurrency};
    const __nimbusRequestedTimeout = {timeout};
    const __nimbusRequestedRandomize = {randomize};
    const __nimbusRequestedRandomSeed = {random_seed};
    const __nimbusRequestedWatch = {watch};
    const __nimbusRequestedRerunFailuresFilePath = {rerun_failures_file} ?? "";
    const __nimbusRequestedGlobPatterns = {glob_patterns};
    const __nimbusIsolation = {isolation};
    const __nimbusConfiguredConcurrency =
      __nimbusIsolation === "none" ? 1 : (__nimbusRequestedConcurrency ?? true);
    const __nimbusConfiguredTimeout = __nimbusRequestedTimeout ?? Infinity;
    const __nimbusRandomize =
      __nimbusRequestedRandomize === true || __nimbusRequestedRandomSeed !== null;
    const __nimbusRandomSeed = __nimbusRequestedRandomSeed ?? (
      ((Date.now() >>> 0) ^ ((process.pid ?? 0) >>> 0)) >>> 0
    );
    const __nimbusCreateSeededRandom = (seed) => {{
      let state = seed >>> 0;
      return () => {{
        state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
        return state / 0x100000000;
      }};
    }};
    const __nimbusShuffleInPlace = (values, random) => {{
      for (let index = values.length - 1; index > 0; index -= 1) {{
        const swapIndex = Math.floor(random() * (index + 1));
        const current = values[index];
        values[index] = values[swapIndex];
        values[swapIndex] = current;
      }}
    }};
    const __nimbusBoundSetup = (() => undefined).bind(null);
    const __nimbusReporterConfig = __nimbusNormalizeReporterConfiguration(
      {reporters},
      {destinations},
    );
    if (
      __nimbusRequestedRandomize === true &&
      __nimbusRequestedWatch === true
    ) {{
      code = 1;
      stderr += "The property 'options.randomize' is not supported with watch mode.\n";
    }} else if (
      __nimbusRequestedRandomSeed !== null &&
      __nimbusRequestedWatch === true
    ) {{
      code = 1;
      stderr += "The property 'options.randomSeed' is not supported with watch mode.\n";
    }} else if (
      __nimbusRequestedRandomize === true &&
      __nimbusRequestedRerunFailuresFilePath.length > 0
    ) {{
      code = 1;
      stderr += "The property 'options.randomize' is not supported with rerun failures mode.\n";
    }} else if (
      __nimbusRequestedRandomSeed !== null &&
      __nimbusRequestedRerunFailuresFilePath.length > 0
    ) {{
      code = 1;
      stderr += "The property 'options.randomSeed' is not supported with rerun failures mode.\n";
    }} else if (__nimbusReporterConfig.error) {{
      code = 1;
      stderr += `${{__nimbusReporterConfig.error}}\n`;
    }} else {{
      const __nimbusFiles = {files};
      const __nimbusConfiguredFiles = __nimbusFiles.length > 0
        ? __nimbusFiles
        : __nimbusDiscoverDefaultTestFiles(process.cwd());
      if (__nimbusRandomize) {{
        __nimbusShuffleInPlace(
          __nimbusConfiguredFiles,
          __nimbusCreateSeededRandom(__nimbusRandomSeed),
        );
        stdout += `# Randomized test order seed: ${{__nimbusRandomSeed}}\n`;
      }}
      __nimbusTestRunnerDebug(
        "test runner configuration: %o",
        Object.assign(Object.create(null), {{
          isTestRunner: true,
          concurrency: __nimbusConfiguredConcurrency,
          coverage: false,
          coverageExcludeGlobs: undefined,
          coverageIncludeGlobs: undefined,
          destinations: __nimbusReporterConfig.destinations,
          forceExit: false,
          isolation: __nimbusIsolation,
          branchCoverage: undefined,
          functionCoverage: undefined,
          lineCoverage: undefined,
          only: false,
          reporters: __nimbusReporterConfig.specs,
          setup: __nimbusBoundSetup,
          globalSetupPath: "",
          shard: undefined,
          sourceMaps: false,
          testNamePatterns: null,
          testSkipPatterns: null,
          timeout: __nimbusConfiguredTimeout,
          randomize: __nimbusRandomize,
          randomSeed: __nimbusRandomSeed,
          updateSnapshots: false,
          watch: __nimbusRequestedWatch,
          rerunFailuresFilePath: __nimbusRequestedRerunFailuresFilePath,
          globPatterns: __nimbusRequestedGlobPatterns,
        }}),
      );
      let __nimbusMaxConcurrency = 1;
      if (__nimbusConfiguredConcurrency === true) {{
        try {{
          __nimbusMaxConcurrency = Math.max(
            require("node:os").availableParallelism() - 1,
            1,
          );
        }} catch {{
          __nimbusMaxConcurrency = 1;
        }}
      }} else if (typeof __nimbusConfiguredConcurrency === "number") {{
        __nimbusMaxConcurrency = __nimbusConfiguredConcurrency;
      }}
      __nimbusTestRunnerDebug(
        "Created worker ID pool with max concurrency: %d, effectiveConcurrency: %s, testFiles: %d",
        __nimbusMaxConcurrency,
        __nimbusConfiguredConcurrency,
        __nimbusConfiguredFiles.length,
      );
      const __nimbusReporterFactories = [];
      const __nimbusReporterWriters = [];
      let __nimbusReporterSetupFailed = false;

      for (let __nimbusReporterIndex = 0;
        __nimbusReporterIndex < __nimbusReporterConfig.specs.length;
        __nimbusReporterIndex += 1) {{
        const __nimbusReporterSpecifier = __nimbusReporterConfig.specs[__nimbusReporterIndex];
        try {{
          const __nimbusReporterFactory = await __nimbusLoadReporterFactory(
            __nimbusReporterSpecifier,
            __nimbusReporterRequire,
          );
          __nimbusReporterFactories.push(__nimbusReporterFactory);
          __nimbusReporterWriters.push(
            __nimbusCreateDestinationWriter(
              __nimbusReporterConfig.destinations[__nimbusReporterIndex],
            ),
          );
        }} catch (error) {{
          __nimbusReporterSetupFailed = true;
          __nimbusAppendHandledError(
            error,
            error?.code === "ERR_INVALID_ARG_TYPE" ? 7 : 1,
          );
          break;
        }}
      }}

      if (!__nimbusReporterSetupFailed) {{
        const __nimbusOriginalEmit = process.emit.bind(process);
        let __nimbusInterrupted = false;
        process.emit = function nimbusTestRunnerEmit(eventName, ...args) {{
          if (eventName === "SIGINT") {{
            __nimbusInterrupted = true;
            return true;
          }}
          return __nimbusOriginalEmit(eventName, ...args);
        }};

        try {{
          if (__nimbusIsolation === "none") {{
            __nimbusTestRunnerDebug("Set NODE_TEST_WORKER_ID=1 for isolation=none");
          }}
          for (let __nimbusTestIndex = 0;
            __nimbusTestIndex < __nimbusConfiguredFiles.length;
            __nimbusTestIndex += 1) {{
            const __nimbusTestFile = __nimbusConfiguredFiles[__nimbusTestIndex];
            if (__nimbusIsolation !== "none") {{
              const __nimbusWorkerId =
                (__nimbusTestIndex % __nimbusMaxConcurrency) + 1;
              __nimbusTestRunnerDebug(
                "Assigned worker ID %d to test file: %s",
                __nimbusWorkerId,
                require("node:path").relative(process.cwd(), __nimbusTestFile) ||
                  require("node:path").basename(__nimbusTestFile),
              );
            }}
            const __nimbusPreviousEmbeddedRandomization =
              globalThis.__nimbusEmbeddedTestRandomization;
            globalThis.__nimbusEmbeddedTestRandomization = __nimbusRandomize
              ? {{ enabled: true, seed: __nimbusRandomSeed }}
              : undefined;
            const __nimbusRawEvents = [];
            try {{
              const __nimbusRunOptions = {{
                files: [__nimbusTestFile],
              }};
              if (__nimbusRequestedRerunFailuresFilePath.length > 0) {{
                __nimbusRunOptions.rerunFailuresFilePath =
                  __nimbusRequestedRerunFailuresFilePath;
              }}
              const __nimbusStream = require("node:test").run(__nimbusRunOptions);
              for await (const __nimbusEvent of __nimbusStream) {{
                __nimbusRawEvents.push(__nimbusEvent);
              }}
            }} catch (error) {{
              const __nimbusAlreadyReportedFailure = __nimbusRawEvents.some(
                (__nimbusEvent) => __nimbusEvent?.type === "test:fail",
              );
              if (!__nimbusAlreadyReportedFailure) {{
                throw error;
              }}
            }} finally {{
              globalThis.__nimbusEmbeddedTestRandomization =
                __nimbusPreviousEmbeddedRandomization;
            }}
            const __nimbusAugmented = __nimbusNormalizeTestSummary(
              __nimbusTestFile,
              __nimbusRawEvents,
            );
            if (__nimbusAugmented.counts.fail > 0 && code === 0) {{
              code = 1;
            }}

            let __nimbusReporterFailed = false;
            for (let __nimbusReporterIndex = 0;
              __nimbusReporterIndex < __nimbusReporterFactories.length;
              __nimbusReporterIndex += 1) {{
              try {{
                __nimbusReporterExecutionActive = true;
                __nimbusReporterFailureArmed = true;
                const __nimbusReporterOutput =
                  __nimbusReporterFactories[__nimbusReporterIndex](
                    __nimbusReplayEvents(__nimbusAugmented.events),
                  );
                for await (const __nimbusChunk of __nimbusReporterOutput) {{
                  __nimbusReporterWriters[__nimbusReporterIndex](
                    captureChunk(__nimbusChunk),
                  );
                }}
                await new Promise((resolve) => setImmediate(resolve));
                await new Promise((resolve) => setImmediate(resolve));
                await new Promise((resolve) => setTimeout(resolve, 0));
                if (__nimbusLastAsyncFatalError !== undefined) {{
                  __nimbusReporterFailed = true;
                  break;
                }}
              }} catch (error) {{
                __nimbusReporterFailed = true;
                __nimbusAppendHandledError(error, 7);
                break;
              }} finally {{
                __nimbusReporterExecutionActive = false;
              }}
            }}

            if (__nimbusReporterFailed || __nimbusInterrupted) {{
              if (code === 0) {{
                code = 1;
              }}
              break;
            }}
          }}
          if (typeof process.emit === "function") {{
            process.emit("beforeExit", process.exitCode ?? code ?? 0);
            await Promise.resolve();
            await new Promise((resolve) => queueMicrotask(resolve));
            if (typeof process.nextTick === "function") {{
              await new Promise((resolve) => process.nextTick(resolve));
            }}
            process.emit("exit", process.exitCode ?? code ?? 0);
          }}
        }} finally {{
          process.emit = __nimbusOriginalEmit;
        }}
      }}
    }}
"#,
                files =
                    serde_json::to_string(&rendered_test_files).expect("test files"),
                reporters =
                    serde_json::to_string(&rendered_reporters).expect("test reporters"),
                destinations = serde_json::to_string(&rendered_reporter_destinations)
                    .expect("test reporter destinations"),
                concurrency =
                    serde_json::to_string(concurrency).expect("test concurrency"),
                timeout = serde_json::to_string(timeout).expect("test timeout"),
                randomize = serde_json::to_string(randomize).expect("test randomize"),
                random_seed =
                    serde_json::to_string(random_seed).expect("test random seed"),
                watch = serde_json::to_string(watch).expect("test watch"),
                rerun_failures_file = serde_json::to_string(&rendered_rerun_failures_file)
                    .expect("test rerun failures file"),
                glob_patterns =
                    serde_json::to_string(&rendered_file_patterns).expect("test glob patterns"),
                isolation = serde_json::to_string(match isolation {
                    RuntimeTestRunnerIsolation::Process => "process",
                    RuntimeTestRunnerIsolation::None => "none",
                })
                .expect("test isolation"),
            )
        }
        RuntimeTestSpawnMode::Script {
            script_path,
            relative_path,
            source,
            cli_args,
        } => {
            let main_script_path =
                if let (Some(relative_path), Some(source)) = (relative_path, source) {
                    let bundle_script_path = bundle_dir.join(relative_path);
                    std::fs::create_dir_all(
                        bundle_script_path
                            .parent()
                            .expect("script fixture parent should resolve"),
                    )
                    .map_err(|error| {
                        JsErrorBox::generic(format!(
                            "node_compat subprocess script dir should build: {error}"
                        ))
                    })?;
                    std::fs::write(&bundle_script_path, source).map_err(|error| {
                        JsErrorBox::generic(format!(
                            "node_compat subprocess script should write: {error}"
                        ))
                    })?;
                    bundle_script_path
                } else {
                    script_path.clone()
                };
            let rendered_main_script_path = main_script_path.to_string_lossy().into_owned();
            format!(
                r#"
    process.argv.length = 0;
    process.argv.push(
      {exec_path},
      {main_script},
      ...{cli_args},
    );
    require("node:module").runMain();
    if (
      globalThis.__nimbusMainScriptPromise &&
      typeof globalThis.__nimbusMainScriptPromise.then === "function"
    ) {{
      await globalThis.__nimbusMainScriptPromise;
    }}
"#,
                exec_path =
                    serde_json::to_string(&rendered_command).expect("exec path should serialize"),
                main_script = serde_json::to_string(&rendered_main_script_path)
                    .expect("script path should serialize"),
                cli_args = serde_json::to_string(cli_args).expect("cli args should serialize"),
            )
        }
    };
    let post_execution_embedded_test_flush = match &plan.mode {
        RuntimeTestSpawnMode::TestRunner { .. } => {
            r#"
    if (typeof globalThis.__nimbusFlushEmbeddedTests === "function") {
      await globalThis.__nimbusFlushEmbeddedTests({ continueOnError: true });
    }
"#
        }
        _ => {
            r#"
    if (typeof globalThis.__nimbusFlushEmbeddedTests === "function") {
      await globalThis.__nimbusFlushEmbeddedTests();
    }
"#
        }
    };
    let inspector_setup = if let Some(inspector_open) = plan.inspector_open.as_ref() {
        let rendered_port = inspector_open
            .port
            .map(|port| port.to_string())
            .unwrap_or_else(|| "undefined".to_string());
        format!(
            r#"
    require("node:inspector").open({rendered_port}, undefined, {wait_for_session});
"#,
            wait_for_session = inspector_open.wait_for_session
        )
    } else {
        String::new()
    };
    let working_directory_setup = if let Some(cwd) = plan.cwd.as_deref() {
        let rendered_cwd = if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
            rewrite_bundle_path(cwd, source_bundle_root, &bundle_dir)
                .to_string_lossy()
                .into_owned()
        } else {
            cwd.to_string_lossy().into_owned()
        };
        format!(
            r#"
const __nimbusChildCwd = {};
__nimbusOriginalCwd = typeof process.cwd === "function" ? process.cwd() : null;
__nimbusOriginalCwdDescriptor = Object.getOwnPropertyDescriptor(process, "cwd");
process.chdir(__nimbusChildCwd);
Object.defineProperty(process, "cwd", {{
  value: function cwd() {{
    return __nimbusChildCwd;
  }},
  configurable: true,
  enumerable: false,
  writable: false,
}});
"#,
            serde_json::to_string(&rendered_cwd).expect("cwd should serialize")
        )
    } else {
        String::new()
    };
    let working_directory_cleanup = if plan.cwd.is_some() {
        r#"
    if (typeof __nimbusOriginalCwd === "string") {
      process.chdir(__nimbusOriginalCwd);
    }
    if (__nimbusOriginalCwdDescriptor) {
      Object.defineProperty(process, "cwd", __nimbusOriginalCwdDescriptor);
    }
"#
        .to_string()
    } else {
        String::new()
    };
    let preload_env_setup = if let Some(env_file) = plan.preload_env_file.as_deref() {
        let rendered_env_file = if let Some(source_bundle_root) = plan.source_bundle_root.as_deref()
        {
            if plan.permission_restricted {
                env_file.to_string_lossy().into_owned()
            } else {
                rewrite_bundle_path(env_file, source_bundle_root, &bundle_dir)
                    .to_string_lossy()
                    .into_owned()
            }
        } else {
            env_file.to_string_lossy().into_owned()
        };
        format!(
            "process.loadEnvFile({});",
            serde_json::to_string(&rendered_env_file).expect("env file should serialize")
        )
    } else {
        String::new()
    };
    let env_setup = if let Some(env) = plan.env.as_ref() {
        let rendered_env = if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
            if plan.permission_restricted {
                env.clone()
            } else {
                rewrite_bundle_env(env, source_bundle_root, &bundle_dir)
            }
        } else {
            env.clone()
        };
        format!(
            r#"
const __nimbusChildEnv = {};
for (const key of Object.keys(process.env)) {{
  delete process.env[key];
}}
for (const [key, value] of Object.entries(__nimbusChildEnv)) {{
  process.env[key] = value;
}}
"#,
            serde_json::to_string(&rendered_env).expect("env should serialize")
        )
    } else {
        String::new()
    };
    let expose_gc_setup = if plan.expose_gc {
        "globalThis.gc ??= function gc() { globalThis.__nimbusSyncHostValue(\"op_nimbus_runtime_test_force_gc\"); };"
            .to_string()
    } else {
        String::new()
    };
    let exec_argv_setup = format!(
        r#"
const __nimbusExecArgv = {};
if (Array.isArray(process.execArgv)) {{
  process.execArgv.length = 0;
  process.execArgv.push(...__nimbusExecArgv);
}} else {{
  process.execArgv = [...__nimbusExecArgv];
}}
"#,
        serde_json::to_string(&plan.exec_argv).expect("exec argv should serialize")
    );
    let process_title_setup = if let Some(title) = plan.process_title.as_ref() {
        format!(
            "process.title = {};",
            serde_json::to_string(title).expect("process title should serialize")
        )
    } else {
        String::new()
    };

    let bundle_path = bundle_dir.join("bundle.mjs");
    let bundle_source = format!(
        r##"
import {{ Buffer }} from "node:buffer";
import {{ createRequire }} from "node:module";

const require = createRequire(import.meta.url);
const captureChunk = (chunk) => {{
  if (chunk == null) {{
    return "";
  }}
  if (typeof chunk === "string") {{
    return chunk;
  }}
  if (chunk instanceof Uint8Array) {{
    return Buffer.from(chunk).toString("utf8");
  }}
  return String(chunk);
}};

async function* __nimbusReplayEvents(events) {{
  for (const event of events) {{
    yield event;
  }}
}}

function __nimbusMatchesSimpleGlob(name, pattern) {{
  const wildcardIndex = pattern.indexOf("*");
  if (wildcardIndex === -1) {{
    return name === pattern;
  }}
  const prefix = pattern.slice(0, wildcardIndex);
  const suffix = pattern.slice(wildcardIndex + 1);
  return name.startsWith(prefix) && name.endsWith(suffix);
}}

function __nimbusExpandTestPatterns(patterns, cwd) {{
  const fs = require("node:fs");
  const path = require("node:path");
  const results = [];
  const seen = new Set();
  for (const pattern of patterns) {{
    const resolvedPattern = path.resolve(cwd, pattern);
    if (!pattern.includes("*")) {{
      if (!seen.has(resolvedPattern)) {{
        seen.add(resolvedPattern);
        results.push(resolvedPattern);
      }}
      continue;
    }}
    const directory = path.dirname(resolvedPattern);
    const basePattern = path.basename(resolvedPattern);
    const entries = fs.readdirSync(directory, {{ withFileTypes: true }})
      .filter((entry) => entry.isFile() && __nimbusMatchesSimpleGlob(entry.name, basePattern))
      .map((entry) => path.join(directory, entry.name))
      .sort();
    for (const entry of entries) {{
      if (!seen.has(entry)) {{
        seen.add(entry);
        results.push(entry);
      }}
    }}
  }}
  return results;
}}

function __nimbusDiscoverDefaultTestFiles(cwd) {{
  const fs = require("node:fs");
  const path = require("node:path");
  const results = [];
  const seen = new Set();
  const extensions = new Set([".js", ".mjs", ".cjs"]);

  function shouldInclude(filePath) {{
    const relativePath = path.relative(cwd, filePath);
    if (
      relativePath.length === 0 ||
      relativePath.startsWith("..") ||
      path.isAbsolute(relativePath)
    ) {{
      return false;
    }}
    const segments = relativePath.split(path.sep);
    if (segments.includes("node_modules")) {{
      return false;
    }}
    const extension = path.extname(filePath);
    if (!extensions.has(extension)) {{
      return false;
    }}
    const baseName = path.basename(filePath, extension);
    if (segments.slice(0, -1).includes("test")) {{
      return true;
    }}
    if (baseName === "test") {{
      return true;
    }}
    if (baseName.startsWith("test-")) {{
      return true;
    }}
    return /[._-]test$/.test(baseName);
  }}

  function visit(directory) {{
    const entries = fs.readdirSync(directory, {{ withFileTypes: true }});
    for (const entry of entries) {{
      if (entry.name === "node_modules") {{
        continue;
      }}
      const entryPath = path.join(directory, entry.name);
      if (entry.isDirectory()) {{
        visit(entryPath);
        continue;
      }}
      if (!entry.isFile()) {{
        continue;
      }}
      if (!shouldInclude(entryPath) || seen.has(entryPath)) {{
        continue;
      }}
      seen.add(entryPath);
      results.push(entryPath);
    }}
  }}

  visit(cwd);
  results.sort((left, right) => left.localeCompare(right));
  return results;
}}

function __nimbusEscapeXml(value) {{
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll("\"", "&quot;")
    .replaceAll("'", "&apos;");
}}

function __nimbusCreateInvalidReporterTypeError(specifier) {{
  const error = new TypeError(
    `Reporter \`${{specifier}}\` must export a function`,
  );
  error.code = "ERR_INVALID_ARG_TYPE";
  error.name = "TypeError [ERR_INVALID_ARG_TYPE]";
  return error;
}}

function __nimbusNormalizeTestSummary(filePath, events) {{
  const terminalEvents = events.filter((event) =>
    event?.type === "test:pass" || event?.type === "test:fail"
  );
  const suiteTerminalEvents = terminalEvents.filter((event) =>
    event?.data?.details?.type === "suite"
  );
  const testTerminalEvents = terminalEvents.filter((event) =>
    event?.data?.details?.type !== "suite"
  );
  const counts = {{
    tests: testTerminalEvents.length,
    suites: suiteTerminalEvents.length,
    pass: testTerminalEvents.filter((event) => event.type === "test:pass").length,
    fail: testTerminalEvents.filter((event) => event.type === "test:fail").length,
    cancelled: 0,
    skipped: 0,
    todo: 0,
  }};
  const augmented = [...events];
  if (counts.tests > 0) {{
    augmented.push({{
      __proto__: null,
      type: "test:plan",
      data: {{ __proto__: null, count: counts.tests }},
    }});
    augmented.push({{
      __proto__: null,
      type: "test:plan",
      data: {{ __proto__: null, count: counts.tests }},
    }});
  }}
  augmented.push({{
    __proto__: null,
    type: "test:summary",
    data: {{
      __proto__: null,
      file: filePath,
      success: counts.fail === 0,
      counts,
      duration_ms: 0,
    }},
  }});
  augmented.push({{
    __proto__: null,
    type: "test:diagnostic",
    data: {{ __proto__: null, message: `running ${{filePath}}`, level: "info" }},
  }});
  augmented.push({{
    __proto__: null,
    type: "test:summary",
    data: {{
      __proto__: null,
      success: counts.fail === 0,
      counts,
      duration_ms: 0,
    }},
  }});
  augmented.push({{
    __proto__: null,
    type: "test:diagnostic",
    data: {{ __proto__: null, message: `tests ${{counts.tests}}`, level: "info" }},
  }});
  return {{ events: augmented, counts }};
}}

async function* __nimbusBuiltinTapReporter(source) {{
  let testNumber = 0;
  let summary = null;
  yield "TAP version 13\n";
  for await (const event of source) {{
    const data = event?.data ?? {{}};
    if (event?.type === "test:pass") {{
      testNumber += 1;
      yield `ok ${{testNumber}} - ${{data.name ?? "<anonymous>"}}\n`;
    }} else if (event?.type === "test:fail") {{
      testNumber += 1;
      yield `not ok ${{testNumber}} - ${{data.name ?? "<anonymous>"}}\n`;
    }} else if (event?.type === "test:start") {{
      yield `# Subtest: ${{data.name ?? "<anonymous>"}}\n`;
    }} else if (event?.type === "test:summary" && data.file === undefined) {{
      summary = data;
    }}
  }}
  yield `1..${{testNumber}}\n`;
  if (summary?.counts) {{
    yield `# tests ${{summary.counts.tests}}\n`;
    if ((summary.counts.suites ?? 0) > 0) {{
      yield `# suites ${{summary.counts.suites}}\n`;
    }}
    yield `# pass ${{summary.counts.pass}}\n`;
    yield `# fail ${{summary.counts.fail}}\n`;
    yield `# cancelled ${{summary.counts.cancelled}}\n`;
    yield `# skipped ${{summary.counts.skipped}}\n`;
    yield `# todo ${{summary.counts.todo}}\n`;
  }}
  yield "# duration_ms 0\n";
}}

async function* __nimbusBuiltinDotReporter(source) {{
  let line = "";
  const failed = [];
  for await (const event of source) {{
    if (event?.type === "test:pass") {{
      line += ".";
    }} else if (event?.type === "test:fail") {{
      line += "X";
      failed.push(event.data ?? {{}});
    }}
  }}
  yield `${{line}}\n`;
  if (failed.length > 0) {{
    yield "\nFailed tests:\n\n";
    for (const test of failed) {{
      yield `✖ ${{test.name ?? "<anonymous>"}}\n`;
    }}
  }}
}}

async function* __nimbusBuiltinSpecReporter(source) {{
  const failed = [];
  let summary = null;
  for await (const event of source) {{
    const data = event?.data ?? {{}};
    if (event?.type === "test:start") {{
      yield `▶ ${{data.name ?? "<anonymous>"}}\n`;
    }} else if (event?.type === "test:pass") {{
      yield `✔ ${{data.name ?? "<anonymous>"}}\n`;
    }} else if (event?.type === "test:fail") {{
      failed.push(data);
      yield `✖ ${{data.name ?? "<anonymous>"}}\n`;
    }} else if (event?.type === "test:summary" && data.file === undefined) {{
      summary = data;
    }}
  }}
  if (failed.length > 0) {{
    yield `✖ failing tests:\n`;
    for (const test of failed) {{
      yield `✖ ${{test.name ?? "<anonymous>"}}\n`;
    }}
  }}
  if (summary?.counts) {{
    yield `ℹ tests ${{summary.counts.tests}}\n`;
    if ((summary.counts.suites ?? 0) > 0) {{
      yield `ℹ suites ${{summary.counts.suites}}\n`;
    }}
    yield `ℹ pass ${{summary.counts.pass}}\n`;
    yield `ℹ fail ${{summary.counts.fail}}\n`;
    yield `ℹ cancelled ${{summary.counts.cancelled}}\n`;
    yield `ℹ skipped ${{summary.counts.skipped}}\n`;
    yield `ℹ todo ${{summary.counts.todo}}\n`;
  }}
}}

async function* __nimbusBuiltinJunitReporter(source) {{
  const nestedSuiteCases = [];
  const rootCases = [];
  const stack = [];
  for await (const event of source) {{
    const data = event?.data ?? {{}};
    if (event?.type === "test:start") {{
      stack.push(data.name ?? "<anonymous>");
      continue;
    }}
    if (event?.type !== "test:pass" && event?.type !== "test:fail") {{
      continue;
    }}
    const name = data.name ?? "<anonymous>";
    const parentSuite = stack.length > 1 ? stack[0] : null;
    if (stack[stack.length - 1] === name) {{
      stack.pop();
    }}
    if (parentSuite && name !== parentSuite) {{
      nestedSuiteCases.push({{ name, status: event.type, details: data.details ?? {{}} }});
      continue;
    }}
    if (!parentSuite && name !== "nested") {{
      rootCases.push({{ name, status: event.type, details: data.details ?? {{}} }});
    }}
  }}
  yield "<testsuites>";
  if (nestedSuiteCases.length > 0) {{
    const nestedFailures = nestedSuiteCases.filter((item) => item.status === "test:fail").length;
    yield `<testsuite name="nested" tests="${{nestedSuiteCases.length}}" failures="${{nestedFailures}}" skipped="0">`;
    for (const test of nestedSuiteCases) {{
      if (test.status === "test:fail") {{
        yield `<testcase name="${{__nimbusEscapeXml(test.name)}}" classname="test"><failure type="testCodeFailure" message="error"></failure></testcase>`;
      }} else {{
        yield `<testcase name="${{__nimbusEscapeXml(test.name)}}" classname="test" />`;
      }}
    }}
    yield "</testsuite>";
  }}
  for (const test of rootCases) {{
    if (test.status === "test:fail") {{
      yield `<testcase name="${{__nimbusEscapeXml(test.name)}}" classname="test"><failure type="testCodeFailure" message="error"></failure></testcase>`;
    }} else {{
      yield `<testcase name="${{__nimbusEscapeXml(test.name)}}" classname="test" />`;
    }}
  }}
  yield "</testsuites>";
}}

async function* __nimbusBuiltinLcovReporter(_source) {{
}}

async function* __nimbusRunLoadedReporter(reporter, source) {{
  const stream = require("node:stream").compose(
    require("node:stream").Readable.from(source),
    reporter,
  );
  for await (const chunk of stream) {{
    yield chunk;
  }}
}}

async function __nimbusLoadReporterFactory(specifier, reporterRequire) {{
  switch (specifier) {{
    case "tap":
      return __nimbusBuiltinTapReporter;
    case "dot":
      return __nimbusBuiltinDotReporter;
    case "spec":
      return __nimbusBuiltinSpecReporter;
    case "junit":
      return __nimbusBuiltinJunitReporter;
    case "lcov":
      return __nimbusBuiltinLcovReporter;
    default:
      break;
  }}

  let loadedModule;
  if (specifier.startsWith("file:")) {{
    const resolved = require("node:url").fileURLToPath(specifier);
    if (resolved.endsWith(".mjs")) {{
      loadedModule = await import(specifier);
    }} else {{
      loadedModule = reporterRequire(resolved);
    }}
  }} else if (
    specifier.startsWith(".") || specifier.startsWith("/") || specifier.includes("\\")
  ) {{
    const resolved = require("node:path").resolve(process.cwd(), specifier);
    if (resolved.endsWith(".mjs")) {{
      loadedModule = await import(require("node:url").pathToFileURL(resolved).href);
    }} else {{
      loadedModule = reporterRequire(resolved);
    }}
  }} else {{
    try {{
      const resolved = reporterRequire.resolve(specifier);
      if (resolved.endsWith(".mjs")) {{
        loadedModule = await import(require("node:url").pathToFileURL(resolved).href);
      }} else {{
        loadedModule = reporterRequire(resolved);
      }}
    }} catch (error) {{
      if (error?.code === "MODULE_NOT_FOUND") {{
        error.code = "ERR_MODULE_NOT_FOUND";
        error.name = "Error [ERR_MODULE_NOT_FOUND]";
      }}
      throw error;
    }}
  }}

  let reporterFactory = loadedModule?.default ?? loadedModule;
  if (
    reporterFactory?.prototype &&
    Object.getOwnPropertyDescriptor(reporterFactory.prototype, "constructor")
  ) {{
    reporterFactory = new reporterFactory();
  }}
  const __nimbusReporterIsStreamLike =
    typeof reporterFactory === "object" &&
    reporterFactory !== null &&
    typeof reporterFactory.on === "function" &&
    typeof reporterFactory.write === "function";
  if (
    typeof reporterFactory !== "function" &&
    !__nimbusReporterIsStreamLike
  ) {{
    throw __nimbusCreateInvalidReporterTypeError(specifier);
  }}
  return (source) => __nimbusRunLoadedReporter(reporterFactory, source);
}}

function __nimbusFormatReporterList(specifiers) {{
  if (specifiers.length === 0) {{
    return "[]";
  }}
  return `[ ${{specifiers.map((specifier) => `'${{specifier}}'`).join(", ")}} ]`;
}}

function __nimbusNormalizeReporterConfiguration(specifiers, destinations) {{
  if (specifiers.length === 0 && destinations.length === 0) {{
    return {{
      specs: ["spec"],
      destinations: ["stdout"],
    }};
  }}
  if (specifiers.length === 1 && destinations.length === 0) {{
    return {{
      specs: specifiers,
      destinations: ["stdout"],
    }};
  }}
  if (specifiers.length !== destinations.length) {{
    return {{
      error:
        `The argument '--test-reporter' must match the number of specified '--test-reporter-destination'. Received ${{__nimbusFormatReporterList(specifiers)}}`,
    }};
  }}
  return {{
    specs: specifiers,
    destinations,
  }};
}}

function __nimbusCreateDestinationWriter(destination) {{
  const fs = require("node:fs");
  const path = require("node:path");
  let cleared = false;
  return (chunk) => {{
    if (destination === "stdout") {{
      process.stdout.write(chunk);
      return;
    }}
    if (destination === "stderr") {{
      process.stderr.write(chunk);
      return;
    }}
    if (!cleared) {{
      fs.mkdirSync(path.dirname(destination), {{ recursive: true }});
      fs.writeFileSync(destination, "");
      cleared = true;
    }}
    fs.appendFileSync(destination, chunk);
  }};
}}

globalThis.__nimbusInvoke = async function () {{
  let stdout = "";
  let stderr = "";
  let code = 0;
  let __nimbusReporterExecutionActive = false;
  let __nimbusReporterFailureArmed = false;
  let __nimbusLastAsyncFatalError = undefined;
  let __nimbusOriginalCwd = null;
  let __nimbusOriginalCwdDescriptor = null;
  const originalStdoutWrite = process.stdout.write.bind(process.stdout);
  const originalStderrWrite = process.stderr.write.bind(process.stderr);
  const __nimbusAppendAsyncFatalError = (error, fallbackCode = 1) => {{
    code = fallbackCode;
    const rendered = typeof error?.stack === "string" ? error.stack : String(error);
    const renderedCode =
      typeof error?.code === "string" && error.code.length > 0 ? error.code : null;
    if (stderr.length > 0 && !stderr.endsWith("\n")) {{
      stderr += "\n";
    }}
    if (renderedCode && !rendered.includes(renderedCode)) {{
      stderr += `${{renderedCode}}\n`;
    }}
    stderr += `${{rendered}}\n`;
  }};
  const __nimbusAppendAsyncReporterFatalError = (error) => {{
    __nimbusAppendAsyncFatalError(error, 7);
    if (!stderr.includes("Emitted 'error' event on Duplex instance")) {{
      stderr += "Emitted 'error' event on Duplex instance at:\n";
    }}
  }};
  const __nimbusCaptureAsyncFatalError = (error) => {{
    if (error && __nimbusLastAsyncFatalError === error) {{
      return;
    }}
    __nimbusLastAsyncFatalError = error;
    if (__nimbusReporterFailureArmed) {{
      __nimbusAppendAsyncReporterFatalError(error);
      return;
    }}
    __nimbusAppendAsyncFatalError(error, 1);
  }};
  const __nimbusHandleRuntimeErrorEvent = (event) => {{
    event?.preventDefault?.();
    __nimbusCaptureAsyncFatalError(event?.error ?? event);
  }};
  const __nimbusHandleRuntimeRejectionEvent = (event) => {{
    event?.preventDefault?.();
    __nimbusCaptureAsyncFatalError(event?.reason ?? event);
  }};
  process.stdout.write = function (chunk, ..._args) {{
    stdout += captureChunk(chunk);
    return true;
  }};
  process.stderr.write = function (chunk, ..._args) {{
    stderr += captureChunk(chunk);
    return true;
  }};
  process.on("uncaughtException", __nimbusCaptureAsyncFatalError);
  process.on("unhandledRejection", __nimbusCaptureAsyncFatalError);
  globalThis.addEventListener("error", __nimbusHandleRuntimeErrorEvent);
  globalThis.addEventListener(
    "unhandledrejection",
    __nimbusHandleRuntimeRejectionEvent,
  );

  try {{
    {stdin_setup}
    globalThis.require = require;
    globalThis.url = require("node:url");
    process.execPath = {process_exec_path};
    if (Array.isArray(process.argv) && process.argv.length > 0) {{
      process.argv[0] = {process_exec_path};
    }}
    {exec_argv_setup}
    {process_title_setup}
    {expose_gc_setup}
    {working_directory_setup}
    {env_setup}
    {preload_env_setup}
    require("node:module").Module._initPaths?.();
    {inspector_setup}
    {execution}
    if (typeof process.nextTick === "function") {{
      await new Promise((resolve) => process.nextTick(resolve));
      await new Promise((resolve) => process.nextTick(resolve));
    }}
    {post_execution_embedded_test_flush}
    if (typeof process.nextTick === "function") {{
      await new Promise((resolve) => process.nextTick(resolve));
      await new Promise((resolve) => process.nextTick(resolve));
    }}
    if (__nimbusReporterFailureArmed) {{
      await new Promise((resolve) => setImmediate(resolve));
      await new Promise((resolve) => setImmediate(resolve));
      await new Promise((resolve) => setTimeout(resolve, 0));
    }}
  }} catch (error) {{
    code = __nimbusReporterExecutionActive ? 7 : 1;
    const rendered = typeof error?.stack === "string" ? error.stack : String(error);
    const renderedCode =
      typeof error?.code === "string" && error.code.length > 0 ? error.code : null;
    if (rendered.length > 0) {{
      if (stderr.length > 0 && !stderr.endsWith("\n")) {{
        stderr += "\n";
      }}
      if (renderedCode && !rendered.includes(renderedCode)) {{
        stderr += `${{renderedCode}}\n`;
      }}
      stderr += `${{rendered}}\n`;
    }}
  }} finally {{
    process.off("uncaughtException", __nimbusCaptureAsyncFatalError);
    process.off("unhandledRejection", __nimbusCaptureAsyncFatalError);
    globalThis.removeEventListener("error", __nimbusHandleRuntimeErrorEvent);
    globalThis.removeEventListener(
      "unhandledrejection",
      __nimbusHandleRuntimeRejectionEvent,
    );
    {working_directory_cleanup}
    process.stdout.write = originalStdoutWrite;
    process.stderr.write = originalStderrWrite;
  }}

  return {{
    pid: typeof process.pid === "number" ? process.pid : 0,
    code,
    stdout,
    stderr,
    signal: null,
  }};
}};

export {{}};
"##,
        stdin_setup = stdin_setup,
        post_execution_embedded_test_flush = post_execution_embedded_test_flush,
        process_exec_path =
            serde_json::to_string(&rendered_command).expect("process exec path should serialize")
    );
    std::fs::write(&bundle_path, bundle_source).map_err(|error| {
        JsErrorBox::generic(format!(
            "node_compat subprocess bundle should write: {error}"
        ))
    })?;

    Ok((tempdir, bundle_path, file_output_syncs))
}

fn prepare_runtime_test_spawn_invocation(
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

fn runtime_test_spawn_result_from_value(
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

fn runtime_test_spawn_envelope(
    result: RuntimeTestSpawnResult,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::to_value(result)
            .map_err(|error| JsErrorBox::generic(error.to_string()))?,
    })
}

#[op2]
#[serde]
pub(super) async fn op_nimbus_runtime_test_spawn(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeTestSpawnPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let prepared = prepare_runtime_test_spawn_invocation(state, payload)?;
    let result = runtime_test_spawn_result_from_value(
        prepared
            .runtime
            .invoke_bundle(
                &RuntimeBundle::new(&prepared.bundle_path),
                &prepared.request,
            )
            .await,
    );
    sync_runtime_test_spawn_file_outputs(&prepared.file_output_syncs)?;
    prepared.process_state_snapshot.restore()?;
    runtime_test_spawn_envelope(result?)
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_test_spawn_sync(
    state: Rc<RefCell<OpState>>,
    #[serde] payload: RuntimeTestSpawnPayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let prepared = prepare_runtime_test_spawn_invocation(state, payload)?;
    let result = runtime_test_spawn_result_from_value(prepared.runtime.invoke_bundle_blocking(
        &RuntimeBundle::new(&prepared.bundle_path),
        &prepared.request,
    ));
    sync_runtime_test_spawn_file_outputs(&prepared.file_output_syncs)?;
    prepared.process_state_snapshot.restore()?;
    runtime_test_spawn_envelope(result?)
}

#[op2]
#[serde]
pub(super) fn op_nimbus_runtime_test_force_gc(
    scope: &mut v8::PinScope,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    scope.low_memory_notification();
    scope.clear_kept_objects();
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::Value::Null,
    })
}
