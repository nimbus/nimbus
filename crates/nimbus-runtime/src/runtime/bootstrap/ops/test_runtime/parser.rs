use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

use crate::backends::v8::embedder::JsErrorBox;

use super::types::{
    RuntimeTestInspectorOpen, RuntimeTestRunnerIsolation, RuntimeTestSpawnMode,
    RuntimeTestSpawnPayload, RuntimeTestSpawnPlan,
};

fn resolve_runtime_test_spawn_path(path: &Path, cwd: Option<&Path>) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else if let Some(cwd) = cwd {
        cwd.join(path)
    } else {
        path.to_path_buf()
    }
}

pub(super) fn runtime_test_spawn_mode(
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
