use std::env;
use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use clap::Args;
use nimbus::{
    HostBridge, HostCallRequest, InvocationKind, InvocationRequest, NimbusRuntime, RuntimeBundle,
    RuntimeExecutionModel, RuntimeInvocationContext, RuntimeLimits, RuntimePolicy, RuntimePoolKind,
};
use tokio::process::Command;

use crate::node;
use nimbus_assets::js_packages;

/// Selects the codegen runner. Default (unset) is the in-binary V8 tooling
/// runner; set to `external-node` for the diagnostic/transition-only external
/// `node` runner (not a supported offline path — BPD4).
const CODEGEN_RUNNER_ENV: &str = "NIMBUS_CODEGEN_RUNNER";
const EMBEDDED_CODEGEN_BUNDLE_PREFIX: &str = ".nimbus-codegen-";
const EMBEDDED_CODEGEN_BUNDLE_SUFFIX: &str = ".mjs";
const EMBEDDED_CODEGEN_TENANT_LABEL: &str = "nimbus-tooling-codegen";
const CODEGEN_BOOTSTRAP: &str = r#"
import { pathToFileURL } from "node:url";

const [codegenEntry, ...cliArgs] = process.argv.slice(1);
const codegenSpecifier =
  codegenEntry.startsWith("@") || codegenEntry.startsWith("file:")
  ? codegenEntry
  : pathToFileURL(codegenEntry).href;
const { runCliFromArgs } = await import(codegenSpecifier);
await runCliFromArgs(cliArgs, {
  onInfo(message) {
    console.error(message);
  },
});
"#;
const EMBEDDED_CODEGEN_BOOTSTRAP: &str = r#"
import { pathToFileURL } from "node:url";

globalThis.__nimbusInvoke = async function (request) {
  const args = request?.args ?? {};
  const codegenSpecifier = args.codegenSpecifier;
  const cliArgs = Array.isArray(args.cliArgs) ? args.cliArgs : [];
  if (typeof codegenSpecifier !== "string" || codegenSpecifier.length === 0) {
    throw new Error("embedded codegen bootstrap requires a codegenSpecifier string");
  }
  const resolvedSpecifier =
    codegenSpecifier.startsWith("@") || codegenSpecifier.startsWith("file:")
      ? codegenSpecifier
      : pathToFileURL(codegenSpecifier).href;
  const imported = await import(resolvedSpecifier);
  if (typeof imported.runCliFromArgs !== "function") {
    throw new Error(`${resolvedSpecifier} does not export runCliFromArgs(...)`);
  }
  await imported.runCliFromArgs(cliArgs, {
    onInfo(message) {
      console.error(message);
    },
  });
  return { ok: true };
};

export {};
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodegenRunner {
    /// Runs codegen via an external `node`. This runner has two distinct roles:
    ///
    /// 1. The **supported** runner for **Cloud Functions**, the one authoring
    ///    surface deliberately out of the in-binary/offline contract (esbuild
    ///    plugin bundling + developer-supplied Firebase SDKs). CF apps select it
    ///    automatically; this is supported behavior for that surface, not a
    ///    fallback.
    /// 2. A **diagnostic/transition-only** opt-out for the **in-contract Convex**
    ///    surface, reachable only via `NIMBUS_CODEGEN_RUNNER=external-node`. The
    ///    Convex surface (schema, server, http, auth.config) is fully supported
    ///    in-binary, so this opt-out is never the supported Convex path and is
    ///    never counted as the BPD offline/in-binary proof.
    ExternalNode,
    /// Default for the whole Convex authoring surface: the in-binary V8 tooling
    /// runner, sourcing the embedded tooling closure (codegen prebundle +
    /// esbuild + platform `@esbuild` binary for the surfaces that use them).
    EmbeddedPilot,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CodegenOptions {
    pub(crate) debug_node_apis: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodegenExecutionContext {
    app_dir: PathBuf,
    package_install_dirs: Vec<PathBuf>,
    embedded_package_install_dir: PathBuf,
    options: CodegenOptions,
}

/// Generate _generated files and runtime bundle from nimbus/ or convex/ source.
#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = crate::cli_ux::CODEGEN_HELP_EXAMPLES
)]
pub(crate) struct CodegenCommand {
    /// App directory containing a nimbus/ or convex/ source root.
    #[arg(long, default_value = ".")]
    pub(crate) app: PathBuf,

    /// Diagnose Node.js builtin imports that should move behind "use node".
    #[arg(long, default_value_t = false)]
    pub(crate) debug_node_apis: bool,
}

pub(crate) async fn run_codegen_command(
    command: CodegenCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    run_codegen_for_app_dir_with_options(
        &command.app,
        CodegenOptions {
            debug_node_apis: command.debug_node_apis,
        },
    )
    .await
}

pub(crate) async fn run_codegen_for_app_dir(
    app_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    run_codegen_for_app_dir_with_options(app_dir, CodegenOptions::default()).await
}

pub(crate) async fn run_codegen_for_app_dir_with_options(
    app_dir: &Path,
    options: CodegenOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let runner = resolve_default_codegen_runner(app_dir)?;
    run_codegen_for_app_dir_with_runner_and_options(app_dir, runner, options).await
}

/// Pick the runner for the default codegen path (`nimbus codegen`/`dev`/`start`).
/// An explicit `NIMBUS_CODEGEN_RUNNER` value always wins. With no explicit
/// setting, in-binary is the supported default for the whole Convex authoring
/// surface — schema, server, http, and `auth.config` — which all run in the V8
/// tooling runtime with no external Node.
///
/// The sole exception is **Cloud Functions**, which is out of the in-binary /
/// offline contract by design (its runtime bundling needs esbuild plugins, and
/// its Firebase server SDKs are developer-supplied). Cloud Functions apps route
/// to the external Node.js runner. This is the supported behavior for that
/// detected surface, not a diagnostic fallback — see the plan's
/// `## Offline contract boundaries`.
fn resolve_default_codegen_runner(app_dir: &Path) -> io::Result<CodegenRunner> {
    let env = std::env::var_os(CODEGEN_RUNNER_ENV);
    let explicit = env
        .as_deref()
        .map(|value| !value.is_empty())
        .unwrap_or(false);
    let runner = parse_codegen_runner_env(env)?;
    if !explicit && runner == CodegenRunner::EmbeddedPilot && is_cloud_functions_app(app_dir) {
        crate::cli_ux::write_stderr_line(
            "info: Cloud Functions codegen runs on the external Node.js runner \
             (Cloud Functions is out of the in-binary/offline contract)",
        )?;
        return Ok(CodegenRunner::ExternalNode);
    }
    Ok(runner)
}

/// Whether `app_dir` is a Cloud Functions project — a `firebase.json` layout or
/// the `@google-cloud/functions-framework` framework variant. Cloud Functions is
/// the one authoring surface kept out of the in-binary/offline contract, so it
/// runs on the external Node.js runner.
fn is_cloud_functions_app(app_dir: &Path) -> bool {
    let Ok(dir) = canonicalize_app_dir(app_dir) else {
        return false;
    };
    node::firebase_functions_project(&dir)
        .ok()
        .flatten()
        .is_some()
        || crate::deploy::package_declares_functions_framework(&dir.join("package.json"))
}

#[cfg(test)]
pub(crate) async fn run_codegen_for_app_dir_with_runner(
    app_dir: &Path,
    runner: CodegenRunner,
) -> Result<(), Box<dyn std::error::Error>> {
    run_codegen_for_app_dir_with_runner_and_options(app_dir, runner, CodegenOptions::default())
        .await
}

pub(crate) async fn run_codegen_for_app_dir_with_runner_and_options(
    app_dir: &Path,
    runner: CodegenRunner,
    options: CodegenOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let context = resolve_codegen_execution_context(app_dir, options)?;
    crate::provision::ensure_known_app_packages(&context.app_dir)?;
    match runner {
        CodegenRunner::ExternalNode => run_external_codegen_for_app_dir(&context).await,
        CodegenRunner::EmbeddedPilot => run_embedded_codegen_for_app_dir(&context).await,
    }
}

fn canonicalize_app_dir(app_dir: &Path) -> io::Result<PathBuf> {
    let candidate = if app_dir.is_absolute() {
        app_dir.to_path_buf()
    } else {
        env::current_dir()?.join(app_dir)
    };
    let metadata = fs::metadata(&candidate).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "app directory {} is not readable: {error}",
                candidate.display()
            ),
        )
    })?;
    if !metadata.is_dir() {
        return Err(io::Error::other(format!(
            "app path {} is not a directory",
            candidate.display()
        )));
    }
    candidate.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to resolve app directory {}: {error}",
                candidate.display()
            ),
        )
    })
}

fn resolve_codegen_execution_context(
    app_dir: &Path,
    options: CodegenOptions,
) -> io::Result<CodegenExecutionContext> {
    let app_dir = canonicalize_app_dir(app_dir)?;
    let package_install_dirs = node::firebase_functions_project(&app_dir)?
        .map(|project| project.source_dirs())
        .unwrap_or_else(|| vec![app_dir.clone()]);
    let embedded_package_install_dir = package_install_dirs
        .first()
        .cloned()
        .unwrap_or_else(|| app_dir.clone());
    Ok(CodegenExecutionContext {
        app_dir,
        package_install_dirs,
        embedded_package_install_dir,
        options,
    })
}

fn build_codegen_process(context: &CodegenExecutionContext, codegen_bundle: &Path) -> Command {
    let mut command = Command::new("node");
    command.current_dir(&context.app_dir);
    command.arg("--input-type=module");
    command.arg("--eval");
    command.arg(CODEGEN_BOOTSTRAP);
    command.arg("--");
    command.arg(codegen_bundle);
    command.arg("--app");
    command.arg(".");
    if context.options.debug_node_apis {
        command.arg("--debug-node-apis");
    }
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    command
}

fn parse_codegen_runner_env(value: Option<std::ffi::OsString>) -> io::Result<CodegenRunner> {
    // Default (unset/empty) is the in-binary V8 tooling runner. The external
    // `node` runner is an opt-in diagnostic/transition-only escape hatch.
    let Some(value) = value else {
        return Ok(CodegenRunner::EmbeddedPilot);
    };
    let normalized = value.to_string_lossy().trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "in-binary" | "embedded" | "tooling" | "default" => Ok(CodegenRunner::EmbeddedPilot),
        "external-node" | "external" | "node" => Ok(CodegenRunner::ExternalNode),
        _ => Err(io::Error::other(format!(
            "{CODEGEN_RUNNER_ENV} must be one of in-binary (default) or external-node; got {:?}",
            value
        ))),
    }
}

async fn run_external_codegen_for_app_dir(
    context: &CodegenExecutionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    node::ensure_node22_runtime_available()?;
    // The external Node runner is a process boundary, not a package-distribution
    // boundary: it still runs the binary's embedded codegen/tooling closure so
    // developer apps never need an installed @nimbus/codegen package.
    let (_tooling_dir, codegen_bundle) =
        materialize_codegen_tooling(context, "external-node-codegen-tooling-")?;
    let mut command = build_codegen_process(context, &codegen_bundle);
    let status = command.status().await.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to start Node.js for `nimbus codegen --app {}`: {error}",
                context.app_dir.display()
            ),
        )
    })?;

    if status.success() {
        return Ok(());
    }

    Err(io::Error::other(format!(
        "`nimbus codegen --app {}` failed with status {status}",
        context.app_dir.display()
    ))
    .into())
}

async fn run_embedded_codegen_for_app_dir(
    context: &CodegenExecutionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    ensure_embedded_codegen_layout_supported(context)?;
    // Materialize the embedded tooling closure (codegen prebundle + esbuild +
    // platform @esbuild native binary) into a temp run dir. The prebundle
    // resolves esbuild module-relative from `<temp>/node_modules`; codegen reads
    // and writes the absolute app dir. The app needs no `node_modules/@nimbus/
    // codegen` — the binary owns codegen (BPD4 runner-flip).
    // Materialize inside the app's `.nimbus/tmp` — an allowed runtime read root.
    // A temp dir outside the app is capability-denied by the tooling runtime.
    let (_tooling_dir, codegen_bundle) = materialize_codegen_tooling(context, "codegen-tooling-")?;
    let mut bootstrap_bundle = write_embedded_codegen_bootstrap_bundle(&context.app_dir)?;
    bootstrap_bundle.as_file_mut().flush()?;
    let bundle = RuntimeBundle::new(bootstrap_bundle.path());
    let request = InvocationRequest {
        kind: InvocationKind::Action,
        function_name: "__nimbus_internal:codegen".to_string(),
        args: serde_json::json!({
            "codegenSpecifier": codegen_bundle.display().to_string(),
            "cliArgs": embedded_codegen_cli_args(context),
        }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let mut limits = RuntimeLimits::tooling_node22();
    limits.execution_model = RuntimeExecutionModel::RunToCompletion;
    limits.runtime_pool_kind = RuntimePoolKind::StartupSnapshotCache;
    // Codegen is a developer tool, not an isolate substrate: it intentionally
    // gets the full host filesystem to read/write the app's own source tree.
    let codegen_grants = nimbus_fs::FsCaps::new().grant("/", nimbus_fs::FsMountCaps::read_write());
    let runtime_policy = RuntimePolicy::new(limits)
        .clone_with_file_system(nimbus_fs::file_system_for_grants(&codegen_grants)?);
    let runtime =
        NimbusRuntime::with_policy(Arc::new(EmbeddedCodegenHost), Arc::new(runtime_policy));
    let invocation_context =
        RuntimeInvocationContext::top_level_for_tenant(&request, EMBEDDED_CODEGEN_TENANT_LABEL);
    let result = runtime
        .executor()
        .invoke_on_worker(runtime.clone(), bundle, request, invocation_context, None)
        .await
        .map_err(|error| {
            io::Error::other(format!(
                "in-binary codegen failed for {}: {error}. \
             Set {CODEGEN_RUNNER_ENV}=external-node for the diagnostic external Node.js runner.",
                context.app_dir.display()
            ))
        })?;
    if result.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "embedded codegen pilot for {} returned an unexpected result: {}",
        context.app_dir.display(),
        result
    ))
    .into())
}

fn materialize_codegen_tooling(
    context: &CodegenExecutionContext,
    prefix: &str,
) -> io::Result<(tempfile::TempDir, PathBuf)> {
    // Materialize inside the app's `.nimbus/tmp` — an allowed runtime read root
    // for the embedded path and an app-owned location for the external path. A
    // temp dir outside the app is capability-denied by the tooling runtime.
    let tooling_parent = context.app_dir.join(".nimbus").join("tmp");
    fs::create_dir_all(&tooling_parent)?;
    let tooling_dir = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(&tooling_parent)?;
    let codegen_bundle = js_packages::materialize_tooling(tooling_dir.path())?;
    Ok((tooling_dir, codegen_bundle))
}

fn embedded_codegen_cli_args(context: &CodegenExecutionContext) -> Vec<String> {
    // Absolute app dir: the codegen bundle runs from a temp tooling dir, so a
    // relative `.` would point at the wrong place. codegen reads/writes this
    // absolute path via the runtime's node-compatible fs.
    let mut args = vec!["--app".to_string(), context.app_dir.display().to_string()];
    if context.options.debug_node_apis {
        args.push("--debug-node-apis".to_string());
    }
    args
}

// (Obsolete) The embedded runner no longer requires a staged
// `node_modules/@nimbus/codegen` in the app: it materializes the codegen
// prebundle + esbuild tooling closure from the embedded payload
// (`js_packages::materialize_tooling`). Removed in the BPD4 runner-flip.

fn ensure_embedded_codegen_layout_supported(context: &CodegenExecutionContext) -> io::Result<()> {
    if context.package_install_dirs.len() == 1
        && context.embedded_package_install_dir == context.app_dir
    {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "in-binary codegen does not yet support Firebase Cloud Functions package layouts rooted at {}. \
         Set {}=external-node to use the supported Cloud Functions external Node.js runner.",
        context.embedded_package_install_dir.display(),
        CODEGEN_RUNNER_ENV
    )))
}

fn write_embedded_codegen_bootstrap_bundle(app_dir: &Path) -> io::Result<tempfile::NamedTempFile> {
    let mut temp_file = tempfile::Builder::new()
        .prefix(EMBEDDED_CODEGEN_BUNDLE_PREFIX)
        .suffix(EMBEDDED_CODEGEN_BUNDLE_SUFFIX)
        .tempfile_in(app_dir)
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "failed to prepare embedded codegen bootstrap in {}: {error}",
                    app_dir.display()
                ),
            )
        })?;
    temp_file
        .write_all(EMBEDDED_CODEGEN_BOOTSTRAP.as_bytes())
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "failed to write embedded codegen bootstrap in {}: {error}",
                    app_dir.display()
                ),
            )
        })?;
    Ok(temp_file)
}

struct EmbeddedCodegenHost;

impl HostBridge for EmbeddedCodegenHost {
    fn call(
        &self,
        request: HostCallRequest,
    ) -> Result<serde_json::Value, nimbus::NimbusRuntimeError> {
        Err(nimbus::NimbusRuntimeError::Contract(format!(
            "embedded codegen should not issue host bridge calls (received {})",
            request.operation
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;

    fn tempdir_in_repo_target() -> tempfile::TempDir {
        let target_dir = repo_root().join("target");
        fs::create_dir_all(&target_dir).expect("repo target dir should exist");
        tempfile::tempdir_in(&target_dir).expect("tempdir in repo target should create")
    }

    fn repo_root() -> PathBuf {
        let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-cli tests");
        manifest_dir
            .parent()
            .and_then(Path::parent)
            .expect("crate manifest dir should have repo root")
            .to_path_buf()
    }

    fn embedded_codegen_tooling_available() -> bool {
        // The embedded runner sources its tooling closure (codegen prebundle +
        // esbuild + platform @esbuild binary) from the embedded payload, staged
        // by `make build-packages`. Skip if the binary was built without it.
        js_packages::tooling_available()
    }

    // (Removed `copy_dir_recursive` + `stage_workspace_codegen_package`: the
    // embedded codegen runner now materializes its tooling closure from the
    // embedded payload, so tests no longer stage @nimbus/codegen + esbuild into
    // an app's node_modules. BPD4 runner-flip.)

    fn write_convex_codegen_source_fixture(app_dir: &Path) {
        let convex_dir = app_dir.join("convex");
        fs::create_dir_all(&convex_dir).expect("convex source dir should create");
        fs::write(
            convex_dir.join("messages.ts"),
            r#"
import { query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async () => [],
});
"#,
        )
        .expect("convex source fixture should write");
    }

    fn write_firebase_cloud_functions_fixture(app_dir: &Path) {
        let functions_dir = app_dir.join("functions");
        let source_dir = functions_dir.join("src");
        fs::create_dir_all(&source_dir).expect("firebase functions source dir should create");
        fs::write(
            app_dir.join("firebase.json"),
            r#"{
  "functions": { "source": "functions" }
}
"#,
        )
        .expect("firebase.json should write");
        fs::write(
            functions_dir.join("package.json"),
            r#"{
  "main": "lib/index.js"
}
"#,
        )
        .expect("functions package.json should write");
        fs::write(
            source_dir.join("index.ts"),
            r#"
import { onDocumentCreated } from "firebase-functions/v2/firestore";

export const syncUser = onDocumentCreated("users/{userId}", async (event) => event);
"#,
        )
        .expect("firebase source fixture should write");
    }

    #[test]
    fn codegen_runner_defaults_to_in_binary_when_env_is_unset() {
        // BPD4: in-binary V8 codegen is the default; external Node is opt-in.
        assert_eq!(
            parse_codegen_runner_env(None).expect("unset env should parse"),
            CodegenRunner::EmbeddedPilot
        );
    }

    #[test]
    fn codegen_runner_selects_in_binary_or_external_node() {
        for value in ["", "in-binary", "embedded", "tooling", "default"] {
            assert_eq!(
                parse_codegen_runner_env(Some(OsString::from(value)))
                    .unwrap_or_else(|error| panic!("value {value:?} should parse: {error}")),
                CodegenRunner::EmbeddedPilot
            );
        }
        for value in ["external-node", "external", "node"] {
            assert_eq!(
                parse_codegen_runner_env(Some(OsString::from(value)))
                    .unwrap_or_else(|error| panic!("value {value:?} should parse: {error}")),
                CodegenRunner::ExternalNode
            );
        }
    }

    #[test]
    fn codegen_runner_rejects_unknown_values() {
        let error = parse_codegen_runner_env(Some(OsString::from("maybe")))
            .expect_err("unknown value should be rejected");
        assert!(
            error.to_string().contains(CODEGEN_RUNNER_ENV),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn external_node_codegen_uses_embedded_tooling_without_app_codegen_package() {
        if !embedded_codegen_tooling_available() {
            eprintln!(
                "skipping external Node codegen test; embedded tooling closure is not staged \
                 (run `make build-packages`)"
            );
            return;
        }
        if let Err(error) = crate::node::ensure_node22_runtime_available() {
            eprintln!("skipping external Node codegen test; Node.js baseline unavailable: {error}");
            return;
        }

        let temp = tempdir_in_repo_target();
        write_firebase_cloud_functions_fixture(temp.path());

        run_codegen_for_app_dir_with_runner(temp.path(), CodegenRunner::ExternalNode)
            .await
            .expect("external Node CF codegen should use the embedded tooling closure");

        assert!(
            !temp
                .path()
                .join("functions/node_modules/@nimbus/codegen")
                .exists(),
            "external Node runner must not require app-installed @nimbus/codegen"
        );
        let firebase_dir = temp.path().join(".nimbus").join("firebase");
        assert!(
            firebase_dir.join("artifact.json").is_file(),
            "cloud functions artifact manifest should be generated"
        );
        assert!(
            firebase_dir.join("targets.json").is_file(),
            "cloud functions targets manifest should be generated"
        );
        assert!(
            firebase_dir.join("bundle.mjs").is_file(),
            "cloud functions runtime bundle should be generated"
        );
    }

    #[tokio::test]
    async fn embedded_codegen_generates_convex_artifacts_without_app_node_modules() {
        if !embedded_codegen_tooling_available() {
            eprintln!(
                "skipping embedded Convex codegen test; embedded tooling closure is not staged \
                 (run `make build-packages`)"
            );
            return;
        }

        let temp = tempdir_in_repo_target();
        write_convex_codegen_source_fixture(temp.path());
        // NB: no stage_workspace_codegen_package — the in-binary runner
        // materializes the embedded tooling closure itself. The app has no
        // node_modules at all.

        run_codegen_for_app_dir_with_runner(temp.path(), CodegenRunner::EmbeddedPilot)
            .await
            .expect("embedded codegen should generate Convex artifacts from the embedded tooling");

        // cond 14: codegen ran with no app-provided @nimbus/codegen.
        assert!(
            !temp.path().join("node_modules/@nimbus/codegen").exists(),
            "embedded codegen must not require an app-provided @nimbus/codegen"
        );
        let convex_dir = temp.path().join(".nimbus").join("convex");
        assert!(
            convex_dir.join("functions.json").is_file(),
            "functions manifest should be generated"
        );
        assert!(
            convex_dir.join("bundle.mjs").is_file(),
            "runtime bundle should be generated"
        );
        assert!(
            temp.path()
                .join("convex")
                .join("_generated")
                .join("api.ts")
                .is_file(),
            "_generated api file should be generated"
        );
    }

    #[tokio::test]
    async fn embedded_pilot_rejects_cloud_functions_layout_with_clear_message() {
        if !embedded_codegen_tooling_available() {
            eprintln!(
                "skipping embedded Cloud Functions codegen test; embedded tooling closure is not staged"
            );
            return;
        }

        let temp = tempdir_in_repo_target();
        write_firebase_cloud_functions_fixture(temp.path());

        let error = run_codegen_for_app_dir_with_runner(temp.path(), CodegenRunner::EmbeddedPilot)
            .await
            .expect_err("embedded codegen pilot should reject Cloud Functions layouts");
        let message = error.to_string();
        assert!(
            message.contains("does not yet support Firebase Cloud Functions package layouts"),
            "unexpected embedded Cloud Functions rejection: {message}"
        );
        assert!(
            message.contains(CODEGEN_RUNNER_ENV),
            "rejection should direct users back to the external Node.js runner: {message}"
        );
    }
}
