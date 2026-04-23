use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use clap::Args;
use tokio::process::Command;

const CODEGEN_PACKAGE_SPECIFIER: &str = "@neovex/codegen";
const CODEGEN_WORKSPACE_ENTRY: [&str; 4] = ["packages", "codegen", "src", "main.mjs"];
const CODEGEN_BOOTSTRAP: &str = r#"
import { pathToFileURL } from "node:url";

const [codegenEntry, ...cliArgs] = process.argv.slice(1);
const codegenSpecifier = codegenEntry.startsWith("@")
  ? codegenEntry
  : pathToFileURL(codegenEntry).href;
const { runCliFromArgs } = await import(codegenSpecifier);
await runCliFromArgs(cliArgs, {
  onInfo(message) {
    console.error(message);
  },
});
"#;

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
    let app_dir = canonicalize_app_dir(app_dir)?;
    let mut command = build_codegen_process(&app_dir);
    let status = command.status().await.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to start Node.js for `neovex codegen --app {}`: {error}",
                app_dir.display()
            ),
        )
    })?;

    if status.success() {
        return Ok(());
    }

    Err(io::Error::other(format!(
        "`neovex codegen --app {}` failed with status {status}",
        app_dir.display()
    ))
    .into())
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

fn build_codegen_process(app_dir: &Path) -> Command {
    let import_target = resolve_codegen_import_target(app_dir);
    let mut command = Command::new("node");
    command.current_dir(app_dir);
    command.arg("--input-type=module");
    command.arg("--eval");
    command.arg(CODEGEN_BOOTSTRAP);
    command.arg("--");
    command.arg(import_target);
    command.arg("--app");
    command.arg(".");
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    command
}

fn resolve_codegen_import_target(app_dir: &Path) -> String {
    find_workspace_codegen_entry(app_dir)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| CODEGEN_PACKAGE_SPECIFIER.to_string())
}

fn find_workspace_codegen_entry(app_dir: &Path) -> Option<PathBuf> {
    let mut search_roots = vec![app_dir.to_path_buf()];

    if let Ok(current_dir) = env::current_dir() {
        search_roots.push(current_dir);
    }

    if let Ok(current_exe) = env::current_exe() {
        search_roots.push(current_exe);
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crate manifest dir should have repo root")
            .to_path_buf()
    }

    #[test]
    fn finds_workspace_codegen_entry_from_repo_target_tempdir() {
        let target_dir = repo_root().join("target");
        fs::create_dir_all(&target_dir).expect("repo target dir should exist");
        let temp = tempfile::tempdir_in(&target_dir).expect("tempdir in repo target should create");

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
}
