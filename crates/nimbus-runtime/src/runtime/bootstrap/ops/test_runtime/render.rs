use std::path::{Path, PathBuf};

use crate::backends::v8::embedder::JsErrorBox;

use super::bundle::{rewrite_bundle_env, rewrite_bundle_path, rewrite_bundle_string};
use super::types::{RuntimeTestRunnerIsolation, RuntimeTestSpawnMode, RuntimeTestSpawnPlan};

pub(super) fn render_runtime_test_spawn_bundle_source(
    plan: &RuntimeTestSpawnPlan,
    bundle_dir: &Path,
    rendered_command: &str,
) -> std::result::Result<String, JsErrorBox> {
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

    Ok(bundle_source)
}
