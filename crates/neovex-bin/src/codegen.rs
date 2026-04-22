use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use clap::Args;
use tokio::process::Command;

const CODEGEN_BOOTSTRAP: &str = r#"
const { runCliFromArgs } = await import("@neovex/codegen");
await runCliFromArgs(process.argv.slice(1), {
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
    let mut command = Command::new("node");
    command.current_dir(app_dir);
    command.arg("--input-type=module");
    command.arg("--eval");
    command.arg(CODEGEN_BOOTSTRAP);
    command.arg("--");
    command.arg("--app");
    command.arg(".");
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());
    command
}
