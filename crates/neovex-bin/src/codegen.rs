use std::env;
use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use clap::Args;
use neovex::{
    HostBridge, HostCallRequest, InvocationKind, InvocationRequest, NeovexRuntime, RuntimeBundle,
    RuntimeLimits, RuntimePolicy,
};
use tokio::process::Command;

use crate::node;

const CODEGEN_PACKAGE_SPECIFIER: &str = "@neovex/codegen";
const CODEGEN_WORKSPACE_ENTRY: [&str; 4] = ["packages", "codegen", "src", "main.mjs"];
const EMBEDDED_CODEGEN_PILOT_ENV: &str = "NEOVEX_EXPERIMENTAL_EMBEDDED_CODEGEN";
const EMBEDDED_CODEGEN_BUNDLE_PREFIX: &str = ".neovex-codegen-";
const EMBEDDED_CODEGEN_BUNDLE_SUFFIX: &str = ".mjs";
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

globalThis.__neovexInvoke = async function (request) {
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
    ExternalNode,
    EmbeddedPilot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodegenExecutionContext {
    app_dir: PathBuf,
    package_install_dirs: Vec<PathBuf>,
    embedded_package_install_dir: PathBuf,
    external_import_target: String,
    embedded_import_target: String,
}

/// Generate _generated files and runtime bundle from neovex/ or convex/ source.
#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = crate::cli_ux::CODEGEN_HELP_EXAMPLES
)]
pub(crate) struct CodegenCommand {
    /// App directory containing a neovex/ or convex/ source root.
    #[arg(long, default_value = ".")]
    pub(crate) app: PathBuf,
}

pub(crate) async fn run_codegen_command(
    command: CodegenCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    run_codegen_for_app_dir(&command.app).await
}

pub(crate) async fn run_codegen_for_app_dir(
    app_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let runner = resolve_codegen_runner()?;
    run_codegen_for_app_dir_with_runner(app_dir, runner).await
}

pub(crate) async fn run_codegen_for_app_dir_with_runner(
    app_dir: &Path,
    runner: CodegenRunner,
) -> Result<(), Box<dyn std::error::Error>> {
    let context = resolve_codegen_execution_context(app_dir)?;
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

fn resolve_codegen_execution_context(app_dir: &Path) -> io::Result<CodegenExecutionContext> {
    let app_dir = canonicalize_app_dir(app_dir)?;
    let package_install_dirs = node::firebase_functions_project(&app_dir)?
        .map(|project| project.source_dirs())
        .unwrap_or_else(|| vec![app_dir.clone()]);
    let embedded_package_install_dir = package_install_dirs
        .first()
        .cloned()
        .unwrap_or_else(|| app_dir.clone());
    let external_import_target = resolve_codegen_import_target(&app_dir, &package_install_dirs);
    let embedded_import_target =
        resolve_embedded_codegen_import_target(&embedded_package_install_dir);
    Ok(CodegenExecutionContext {
        app_dir,
        package_install_dirs,
        embedded_package_install_dir,
        external_import_target,
        embedded_import_target,
    })
}

fn build_codegen_process(context: &CodegenExecutionContext) -> Command {
    let mut command = Command::new("node");
    command.current_dir(&context.app_dir);
    command.arg("--input-type=module");
    command.arg("--eval");
    command.arg(CODEGEN_BOOTSTRAP);
    command.arg("--");
    command.arg(&context.external_import_target);
    command.arg("--app");
    command.arg(".");
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    command
}

fn resolve_codegen_runner() -> io::Result<CodegenRunner> {
    parse_codegen_runner_env(env::var_os(EMBEDDED_CODEGEN_PILOT_ENV))
}

fn parse_codegen_runner_env(value: Option<std::ffi::OsString>) -> io::Result<CodegenRunner> {
    let Some(value) = value else {
        return Ok(CodegenRunner::ExternalNode);
    };
    let normalized = value.to_string_lossy().trim().to_ascii_lowercase();
    match normalized.as_str() {
        "" | "0" | "false" | "no" | "off" | "node" | "external-node" => {
            Ok(CodegenRunner::ExternalNode)
        }
        "1" | "true" | "yes" | "on" | "embedded" | "pilot" => Ok(CodegenRunner::EmbeddedPilot),
        _ => Err(io::Error::other(format!(
            "{EMBEDDED_CODEGEN_PILOT_ENV} must be one of \
             1/0, true/false, on/off, yes/no, embedded, or node; got {:?}",
            value
        ))),
    }
}

async fn run_external_codegen_for_app_dir(
    context: &CodegenExecutionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    node::ensure_node22_runtime_available()?;
    let mut command = build_codegen_process(context);
    let status = command.status().await.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to start Node.js for `neovex codegen --app {}`: {error}",
                context.app_dir.display()
            ),
        )
    })?;

    if status.success() {
        return Ok(());
    }

    Err(io::Error::other(format!(
        "`neovex codegen --app {}` failed with status {status}",
        context.app_dir.display()
    ))
    .into())
}

async fn run_embedded_codegen_for_app_dir(
    context: &CodegenExecutionContext,
) -> Result<(), Box<dyn std::error::Error>> {
    ensure_embedded_codegen_package_available(&context.embedded_package_install_dir)?;
    ensure_embedded_codegen_layout_supported(context)?;
    let mut bootstrap_bundle = write_embedded_codegen_bootstrap_bundle(&context.app_dir)?;
    bootstrap_bundle.as_file_mut().flush()?;
    let bundle = RuntimeBundle::new(bootstrap_bundle.path());
    let request = InvocationRequest {
        kind: InvocationKind::Action,
        function_name: "__neovex_internal:codegen".to_string(),
        args: serde_json::json!({
            "codegenSpecifier": context.embedded_import_target.clone(),
            "cliArgs": ["--app", "."],
        }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let runtime = NeovexRuntime::with_policy(
        Arc::new(EmbeddedCodegenHost),
        Arc::new(RuntimePolicy::new(RuntimeLimits::tooling_node22())),
    );
    let result = runtime
        .invoke_bundle(&bundle, &request)
        .await
        .map_err(|error| {
            io::Error::other(format!(
                "embedded codegen pilot failed for {}: {error}. \
             Unset {EMBEDDED_CODEGEN_PILOT_ENV} to use the external Node.js runner.",
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

fn ensure_embedded_codegen_package_available(package_install_dir: &Path) -> io::Result<()> {
    let package_manifest = codegen_package_manifest_path(package_install_dir);
    if package_manifest.is_file() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "embedded codegen pilot requires a staged {} package at {}. \
         Install app dependencies first or unset {} to use the external Node.js runner.",
        CODEGEN_PACKAGE_SPECIFIER,
        package_manifest.display(),
        EMBEDDED_CODEGEN_PILOT_ENV
    )))
}

fn ensure_embedded_codegen_layout_supported(context: &CodegenExecutionContext) -> io::Result<()> {
    if context.package_install_dirs.len() == 1
        && context.embedded_package_install_dir == context.app_dir
    {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "embedded codegen pilot does not yet support Firebase Cloud Functions package layouts rooted at {}. \
         Unset {} to use the external Node.js runner.",
        context.embedded_package_install_dir.display(),
        EMBEDDED_CODEGEN_PILOT_ENV
    )))
}

fn codegen_package_manifest_path(package_install_dir: &Path) -> PathBuf {
    package_install_dir
        .join("node_modules")
        .join("@neovex")
        .join("codegen")
        .join("package.json")
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

fn resolve_codegen_import_target(app_dir: &Path, package_install_dirs: &[PathBuf]) -> String {
    resolve_codegen_import_target_with_search_roots(
        package_install_dirs,
        codegen_workspace_search_roots(app_dir),
    )
}

fn resolve_embedded_codegen_import_target(package_install_dir: &Path) -> String {
    resolve_installed_codegen_entry(package_install_dir)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| CODEGEN_PACKAGE_SPECIFIER.to_string())
}

fn resolve_codegen_import_target_with_search_roots(
    package_install_dirs: &[PathBuf],
    search_roots: Vec<PathBuf>,
) -> String {
    find_workspace_codegen_entry(search_roots)
        .or_else(|| {
            package_install_dirs
                .iter()
                .find_map(|install_dir| resolve_installed_codegen_entry(install_dir))
        })
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| CODEGEN_PACKAGE_SPECIFIER.to_string())
}

fn codegen_workspace_search_roots(app_dir: &Path) -> Vec<PathBuf> {
    let mut search_roots = vec![app_dir.to_path_buf()];

    if let Ok(current_dir) = env::current_dir() {
        search_roots.push(current_dir);
    }

    if let Ok(current_exe) = env::current_exe() {
        search_roots.push(current_exe);
    }

    search_roots
}

fn find_workspace_codegen_entry(search_roots: Vec<PathBuf>) -> Option<PathBuf> {
    search_roots
        .into_iter()
        .find_map(|root| find_workspace_codegen_entry_from(&root))
}

fn find_workspace_codegen_entry_from(start: &Path) -> Option<PathBuf> {
    let start = if start.is_dir() {
        start
    } else {
        start.parent()?
    };
    start.ancestors().find_map(|ancestor| {
        let candidate = CODEGEN_WORKSPACE_ENTRY
            .iter()
            .fold(ancestor.to_path_buf(), |path, segment| path.join(segment));
        candidate.is_file().then_some(candidate)
    })
}

fn resolve_installed_codegen_entry(package_install_dir: &Path) -> Option<PathBuf> {
    let package_manifest = codegen_package_manifest_path(package_install_dir);
    let package_root = package_manifest.parent()?.to_path_buf();
    let content = fs::read_to_string(&package_manifest).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let entry = parsed
        .get("exports")
        .and_then(|exports| {
            exports
                .as_str()
                .or_else(|| exports.get(".").and_then(serde_json::Value::as_str))
        })
        .or_else(|| parsed.get("main").and_then(serde_json::Value::as_str))
        .unwrap_or("./src/main.mjs");
    let relative_entry = entry.strip_prefix("./").unwrap_or(entry);
    let resolved = package_root.join(relative_entry);
    resolved.is_file().then_some(resolved)
}

struct EmbeddedCodegenHost;

impl HostBridge for EmbeddedCodegenHost {
    fn call(
        &self,
        request: HostCallRequest,
    ) -> Result<serde_json::Value, neovex::NeovexRuntimeError> {
        Err(neovex::NeovexRuntimeError::Contract(format!(
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
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crate manifest dir should have repo root")
            .to_path_buf()
    }

    fn workspace_embedded_codegen_dependencies_available() -> bool {
        let repo_root = repo_root();
        repo_root.join("packages/codegen/src/main.mjs").is_file()
            && repo_root.join("node_modules/esbuild").is_dir()
            && repo_root.join("node_modules/typescript").is_dir()
            && repo_root.join("node_modules/@esbuild").is_dir()
    }

    fn copy_dir_recursive(source: &Path, destination: &Path) {
        fs::create_dir_all(destination).unwrap_or_else(|error| {
            panic!(
                "destination directory {} should create: {error}",
                destination.display()
            );
        });
        for entry in fs::read_dir(source).unwrap_or_else(|error| {
            panic!("source directory {} should read: {error}", source.display());
        }) {
            let entry = entry.expect("directory entry should resolve");
            let entry_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            if entry
                .file_type()
                .expect("directory entry type should load")
                .is_dir()
            {
                copy_dir_recursive(&entry_path, &destination_path);
            } else {
                fs::copy(&entry_path, &destination_path).unwrap_or_else(|error| {
                    panic!(
                        "copy {} -> {} should succeed: {error}",
                        entry_path.display(),
                        destination_path.display()
                    );
                });
            }
        }
    }

    fn stage_workspace_codegen_package(package_install_dir: &Path) {
        let repo_root = repo_root();
        let package_root = package_install_dir
            .join("node_modules")
            .join("@neovex")
            .join("codegen");
        fs::create_dir_all(&package_root).expect("package root should create");
        fs::copy(
            repo_root.join("packages/codegen/package.json"),
            package_root.join("package.json"),
        )
        .expect("package.json should copy");
        copy_dir_recursive(
            &repo_root.join("packages/codegen/src"),
            &package_root.join("src"),
        );
        copy_dir_recursive(
            &repo_root.join("node_modules/esbuild"),
            &package_install_dir.join("node_modules/esbuild"),
        );
        copy_dir_recursive(
            &repo_root.join("node_modules/typescript"),
            &package_install_dir.join("node_modules/typescript"),
        );
        copy_dir_recursive(
            &repo_root.join("node_modules/@esbuild"),
            &package_install_dir.join("node_modules/@esbuild"),
        );
    }

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
    fn finds_workspace_codegen_entry_from_repo_target_tempdir() {
        let temp = tempdir_in_repo_target();

        let entry = find_workspace_codegen_entry_from(temp.path())
            .expect("repo-relative tempdir should resolve workspace codegen entry");

        assert_eq!(entry, repo_root().join("packages/codegen/src/main.mjs"));
    }

    #[test]
    fn returns_none_when_no_workspace_codegen_entry_is_present() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let entry = find_workspace_codegen_entry_from(temp.path());

        assert!(
            entry.is_none(),
            "non-repo tempdir should not resolve workspace codegen"
        );
    }

    #[test]
    fn resolve_codegen_import_target_uses_cloud_functions_install_root_when_workspace_entry_is_absent()
     {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let install_root = temp.path().join("functions");
        let entry_path = install_root.join("node_modules/@neovex/codegen/src/main.mjs");
        fs::create_dir_all(entry_path.parent().expect("entry parent should resolve"))
            .expect("entry parent should create");
        fs::write(
            install_root.join("node_modules/@neovex/codegen/package.json"),
            r#"{"exports":{"." :"./src/main.mjs"}}"#,
        )
        .expect("package manifest should write");
        fs::write(&entry_path, "export async function runCliFromArgs() {}")
            .expect("entry file should write");

        let import_target = resolve_codegen_import_target_with_search_roots(
            std::slice::from_ref(&install_root),
            Vec::new(),
        );

        assert_eq!(import_target, entry_path.display().to_string());
    }

    #[test]
    fn resolve_codegen_import_target_searches_all_firebase_codebase_roots() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let app_root = temp.path().join("app");
        let first_root = app_root.join("packages/app-functions");
        let second_root = app_root.join("packages/admin-functions");
        fs::create_dir_all(&first_root).expect("first root should create");
        let entry_path = second_root.join("node_modules/@neovex/codegen/src/main.mjs");
        fs::create_dir_all(entry_path.parent().expect("entry parent should resolve"))
            .expect("entry parent should create");
        fs::write(
            second_root.join("node_modules/@neovex/codegen/package.json"),
            r#"{"exports":{"." :"./src/main.mjs"}}"#,
        )
        .expect("package manifest should write");
        fs::write(&entry_path, "export async function runCliFromArgs() {}")
            .expect("entry file should write");

        let import_target =
            resolve_codegen_import_target_with_search_roots(&[first_root, second_root], Vec::new());

        assert_eq!(import_target, entry_path.display().to_string());
    }

    #[test]
    fn codegen_runner_defaults_to_external_node_when_env_is_unset() {
        assert_eq!(
            parse_codegen_runner_env(None).expect("unset env should parse"),
            CodegenRunner::ExternalNode
        );
    }

    #[test]
    fn codegen_runner_accepts_truthy_embedded_values() {
        for value in ["1", "true", "on", "yes", "embedded", "pilot"] {
            assert_eq!(
                parse_codegen_runner_env(Some(OsString::from(value)))
                    .unwrap_or_else(|error| panic!("value {value:?} should parse: {error}")),
                CodegenRunner::EmbeddedPilot
            );
        }
    }

    #[test]
    fn codegen_runner_rejects_unknown_values() {
        let error = parse_codegen_runner_env(Some(OsString::from("maybe")))
            .expect_err("unknown value should be rejected");
        assert!(
            error.to_string().contains(EMBEDDED_CODEGEN_PILOT_ENV),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn embedded_pilot_generates_convex_artifacts_from_staged_workspace_package() {
        if !workspace_embedded_codegen_dependencies_available() {
            eprintln!(
                "skipping embedded Convex codegen pilot test; workspace JS dependencies are unavailable"
            );
            return;
        }

        let temp = tempdir_in_repo_target();
        write_convex_codegen_source_fixture(temp.path());
        stage_workspace_codegen_package(temp.path());

        run_codegen_for_app_dir_with_runner(temp.path(), CodegenRunner::EmbeddedPilot)
            .await
            .expect("embedded codegen pilot should generate Convex artifacts");

        let convex_dir = temp.path().join(".neovex").join("convex");
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
        if !workspace_embedded_codegen_dependencies_available() {
            eprintln!(
                "skipping embedded Cloud Functions codegen pilot test; workspace JS dependencies are unavailable"
            );
            return;
        }

        let temp = tempdir_in_repo_target();
        write_firebase_cloud_functions_fixture(temp.path());
        stage_workspace_codegen_package(&temp.path().join("functions"));

        let error = run_codegen_for_app_dir_with_runner(temp.path(), CodegenRunner::EmbeddedPilot)
            .await
            .expect_err("embedded codegen pilot should reject Cloud Functions layouts");
        let message = error.to_string();
        assert!(
            message.contains("does not yet support Firebase Cloud Functions package layouts"),
            "unexpected embedded Cloud Functions rejection: {message}"
        );
        assert!(
            message.contains(EMBEDDED_CODEGEN_PILOT_ENV),
            "rejection should direct users back to the external Node.js runner: {message}"
        );
    }
}
