use std::io;
use std::path::{Path, PathBuf};

use clap::Args;

use crate::cli_ux;

pub(crate) const CONVEX_VERSION: &str = env!("NEOVEX_CONVEX_VERSION");
pub(crate) const CODEGEN_VERSION: &str = env!("NEOVEX_CODEGEN_VERSION");

/// Scaffold a new Neovex project.
#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct InitCommand {
    /// Adapter to scaffold (e.g. convex, cloud-functions).
    #[arg(value_parser = ["convex", "cloud-functions"])]
    pub(crate) adapter: String,

    /// Target directory (created if it does not exist).
    #[arg(default_value = ".")]
    pub(crate) directory: PathBuf,

    /// Source root directory name (convex adapter only).
    #[arg(long, default_value = "convex")]
    pub(crate) source_root: String,

    /// Install adapter dependencies after scaffolding.
    #[arg(long, default_value_t = false)]
    pub(crate) install: bool,
}

pub(crate) async fn run_init_command(
    command: InitCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let adapter = crate::node::Adapter::from_cli_arg(&command.adapter)
        .ok_or_else(|| format!("unknown adapter: {}", command.adapter))?;

    if adapter == crate::node::Adapter::Convex {
        check_source_root_flag(&command.source_root)?;
    }

    let target = if command.directory.is_absolute() {
        command.directory.clone()
    } else {
        std::env::current_dir()?.join(&command.directory)
    };

    if !target.exists() {
        std::fs::create_dir_all(&target)
            .map_err(|e| format!("failed to create {}: {e}", target.display()))?;
    }

    let canonical = target
        .canonicalize()
        .map_err(|e| format!("failed to resolve {}: {e}", target.display()))?;

    check_adapter_already_exists(adapter, &canonical)?;

    let templates = adapter_templates(adapter);
    let result = scaffold_project(&canonical, templates)?;

    cli_ux::write_stderr_line("")?;
    cli_ux::write_stderr_line("Created starter project:")?;
    for action in &result.actions {
        match action {
            ScaffoldAction::Created(path) => {
                cli_ux::write_stderr_line(&format!("  {path}"))?;
            }
            ScaffoldAction::Skipped(path) => {
                cli_ux::write_stderr_line(&format!("  skipped: {path} (already exists)"))?;
            }
        }
    }

    if command.install && adapter.needs_node_dependencies() {
        let npm_dir = adapter_npm_install_dir(adapter, &canonical);
        cli_ux::write_stderr_line("")?;
        crate::node::auto_install_node_dependencies(&npm_dir)
            .await
            .map_err(|error| io::Error::other(format_init_install_failure(&canonical, &*error)))?;
    }

    cli_ux::write_stderr_line("")?;
    cli_ux::write_stderr_line("Next steps:")?;
    if command.directory != Path::new(".") {
        cli_ux::write_stderr_line(&format!("  cd {}", command.directory.display()))?;
    }
    cli_ux::write_stderr_line("  neovex dev")?;

    Ok(())
}

const CONVEX_SCHEMA_TS: &str = include_str!("../templates/convex/convex/schema.ts");
const CONVEX_MESSAGES_TS: &str = include_str!("../templates/convex/convex/messages.ts");
const CONVEX_GITIGNORE: &str = include_str!("../templates/convex/gitignore");
const CONVEX_TSCONFIG_JSON: &str = include_str!("../templates/convex/tsconfig.json");
const CONVEX_PACKAGE_JSON_TMPL: &str = include_str!("../templates/convex/package.json.tmpl");

const CF_FIREBASE_JSON: &str = include_str!("../templates/cloud-functions/firebase.json");
const CF_FUNCTIONS_PACKAGE_JSON_TMPL: &str =
    include_str!("../templates/cloud-functions/functions/package.json.tmpl");
const CF_FUNCTIONS_TSCONFIG_JSON: &str =
    include_str!("../templates/cloud-functions/functions/tsconfig.json");
const CF_FUNCTIONS_INDEX_TS: &str =
    include_str!("../templates/cloud-functions/functions/src/index.ts");
const CF_GITIGNORE: &str = include_str!("../templates/cloud-functions/gitignore");

pub(crate) fn render_template(template: &str, project_name: &str) -> String {
    template
        .replace("{{PROJECT_NAME}}", project_name)
        .replace("{{CONVEX_VERSION}}", CONVEX_VERSION)
        .replace("{{CODEGEN_VERSION}}", CODEGEN_VERSION)
}

struct TemplateFile {
    relative_path: &'static str,
    content: TemplateContent,
}

enum TemplateContent {
    Static(&'static str),
    Template(&'static str),
}

const CONVEX_TEMPLATE: &[TemplateFile] = &[
    TemplateFile {
        relative_path: "convex/schema.ts",
        content: TemplateContent::Static(CONVEX_SCHEMA_TS),
    },
    TemplateFile {
        relative_path: "convex/messages.ts",
        content: TemplateContent::Static(CONVEX_MESSAGES_TS),
    },
    TemplateFile {
        relative_path: ".gitignore",
        content: TemplateContent::Static(CONVEX_GITIGNORE),
    },
    TemplateFile {
        relative_path: "tsconfig.json",
        content: TemplateContent::Static(CONVEX_TSCONFIG_JSON),
    },
    TemplateFile {
        relative_path: "package.json",
        content: TemplateContent::Template(CONVEX_PACKAGE_JSON_TMPL),
    },
];

const CLOUD_FUNCTIONS_TEMPLATE: &[TemplateFile] = &[
    TemplateFile {
        relative_path: "firebase.json",
        content: TemplateContent::Static(CF_FIREBASE_JSON),
    },
    TemplateFile {
        relative_path: "functions/package.json",
        content: TemplateContent::Template(CF_FUNCTIONS_PACKAGE_JSON_TMPL),
    },
    TemplateFile {
        relative_path: "functions/tsconfig.json",
        content: TemplateContent::Static(CF_FUNCTIONS_TSCONFIG_JSON),
    },
    TemplateFile {
        relative_path: "functions/src/index.ts",
        content: TemplateContent::Static(CF_FUNCTIONS_INDEX_TS),
    },
    TemplateFile {
        relative_path: ".gitignore",
        content: TemplateContent::Static(CF_GITIGNORE),
    },
];

#[derive(Debug)]
enum ScaffoldAction {
    Created(String),
    Skipped(String),
}

#[derive(Debug)]
struct ScaffoldResult {
    actions: Vec<ScaffoldAction>,
}

fn is_unsafe_directory(dir: &Path) -> Option<&'static str> {
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());

    if let Ok(home) = std::env::var("HOME")
        && canonical
            == Path::new(&home)
                .canonicalize()
                .unwrap_or_else(|_| home.into())
    {
        return Some(
            "Refusing to scaffold into your home directory. \
             Create a project directory first: `mkdir my-app && cd my-app`",
        );
    }

    let path_str = canonical.to_string_lossy();
    if path_str == "/" {
        return Some(
            "Refusing to scaffold into the root directory. \
             Create a project directory first: `mkdir my-app && cd my-app`",
        );
    }
    if path_str == "/tmp" || path_str == "/private/tmp" {
        return Some(
            "Refusing to scaffold into /tmp. \
             Create a project directory first: `mkdir my-app && cd my-app`",
        );
    }
    None
}

fn adapter_templates(adapter: crate::node::Adapter) -> &'static [TemplateFile] {
    match adapter {
        crate::node::Adapter::Convex => CONVEX_TEMPLATE,
        crate::node::Adapter::CloudFunctions => CLOUD_FUNCTIONS_TEMPLATE,
    }
}

fn check_adapter_already_exists(
    adapter: crate::node::Adapter,
    target_dir: &Path,
) -> Result<(), String> {
    match adapter {
        crate::node::Adapter::Convex => {
            if target_dir.join("convex").is_dir() || target_dir.join("neovex").is_dir() {
                return Err(
                    "Source root already exists. Run `neovex dev` to start the development server."
                        .to_string(),
                );
            }
        }
        crate::node::Adapter::CloudFunctions => {
            if target_dir.join("firebase.json").is_file() {
                return Err(
                    "firebase.json already exists. Run `neovex dev` to start the development server."
                        .to_string(),
                );
            }
        }
    }
    Ok(())
}

fn adapter_npm_install_dir(adapter: crate::node::Adapter, target_dir: &Path) -> PathBuf {
    match adapter {
        crate::node::Adapter::Convex => target_dir.to_path_buf(),
        crate::node::Adapter::CloudFunctions => target_dir.join("functions"),
    }
}

fn format_init_install_failure(target_dir: &Path, error: &dyn std::error::Error) -> String {
    format!(
        "Starter project was created at {} but dependency installation failed. \
Resolve the npm error above, then rerun `neovex dev` from that directory or run `npm install` manually. \
Details: {error}",
        target_dir.display()
    )
}

fn scaffold_project(
    target_dir: &Path,
    templates: &[TemplateFile],
) -> Result<ScaffoldResult, String> {
    if let Some(msg) = is_unsafe_directory(target_dir) {
        return Err(msg.to_string());
    }

    let project_name = target_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my-app");

    let mut actions = Vec::new();

    for template in templates {
        let dest = target_dir.join(template.relative_path);

        if dest.exists() {
            actions.push(ScaffoldAction::Skipped(template.relative_path.to_string()));
            continue;
        }

        if let Some(parent) = dest.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }

        let content = match &template.content {
            TemplateContent::Static(s) => (*s).to_string(),
            TemplateContent::Template(tmpl) => render_template(tmpl, project_name),
        };

        std::fs::write(&dest, content)
            .map_err(|e| format!("failed to write {}: {e}", dest.display()))?;

        actions.push(ScaffoldAction::Created(template.relative_path.to_string()));
    }

    Ok(ScaffoldResult { actions })
}

pub(crate) fn check_source_root_flag(source_root: &str) -> Result<(), String> {
    if source_root == "neovex" {
        return Err(
            "The neovex/ source root is experimental and not yet supported \
             by the scaffold templates."
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::{Cli, Command};

    fn parse_init<I, T>(args: I) -> InitCommand
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let cli = Cli::parse_from(args);
        let Command::Init(command) = cli.command else {
            panic!("init subcommand should parse");
        };
        command
    }

    #[test]
    fn compile_time_versions_are_populated() {
        assert!(
            !CONVEX_VERSION.is_empty(),
            "NEOVEX_CONVEX_VERSION should be set by build.rs"
        );
        assert!(
            !CODEGEN_VERSION.is_empty(),
            "NEOVEX_CODEGEN_VERSION should be set by build.rs"
        );
        assert!(
            CONVEX_VERSION.contains('.'),
            "NEOVEX_CONVEX_VERSION should be a semver string, got: {CONVEX_VERSION}"
        );
        assert!(
            CODEGEN_VERSION.contains('.'),
            "NEOVEX_CODEGEN_VERSION should be a semver string, got: {CODEGEN_VERSION}"
        );
    }

    #[test]
    fn convex_package_json_template_substitution() {
        let rendered = render_template(CONVEX_PACKAGE_JSON_TMPL, "my-app");
        assert!(
            rendered.contains(&format!("\"convex\": \"^{CONVEX_VERSION}\"")),
            "rendered package.json should contain convex version"
        );
        assert!(
            rendered.contains(&format!("\"@neovex/codegen\": \"^{CODEGEN_VERSION}\"")),
            "rendered package.json should contain codegen version"
        );
        assert!(
            rendered.contains("\"name\": \"my-app\""),
            "rendered package.json should contain the project name"
        );
        assert!(
            !rendered.contains("{{"),
            "rendered package.json should not contain unresolved placeholders"
        );
    }

    #[test]
    fn cloud_functions_package_json_template_substitution() {
        let rendered = render_template(CF_FUNCTIONS_PACKAGE_JSON_TMPL, "my-app");
        assert!(
            rendered.contains("\"name\": \"my-app-functions\""),
            "rendered package.json should contain the project name"
        );
        assert!(
            rendered.contains(&format!("\"@neovex/codegen\": \"^{CODEGEN_VERSION}\"")),
            "rendered package.json should contain codegen version"
        );
        assert!(
            rendered.contains("\"firebase-functions\""),
            "rendered package.json should contain firebase-functions"
        );
        assert!(
            rendered.contains("\"firebase-admin\""),
            "rendered package.json should contain firebase-admin"
        );
        assert!(
            !rendered.contains("{{"),
            "rendered package.json should not contain unresolved placeholders"
        );
    }

    #[test]
    fn scaffold_convex_writes_all_files_to_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = scaffold_project(tmp.path(), CONVEX_TEMPLATE).unwrap();

        assert_eq!(result.actions.len(), 5);
        for action in &result.actions {
            assert!(
                matches!(action, ScaffoldAction::Created(_)),
                "all files should be created in empty dir"
            );
        }

        assert!(tmp.path().join("convex/schema.ts").exists());
        assert!(tmp.path().join("convex/messages.ts").exists());
        assert!(tmp.path().join(".gitignore").exists());
        assert!(tmp.path().join("tsconfig.json").exists());
        assert!(tmp.path().join("package.json").exists());

        let pkg = std::fs::read_to_string(tmp.path().join("package.json")).unwrap();
        assert!(
            pkg.contains(&format!("\"convex\": \"^{CONVEX_VERSION}\"")),
            "package.json should have substituted convex version"
        );
        assert!(
            !pkg.contains("{{"),
            "package.json should not have unresolved placeholders"
        );
    }

    #[test]
    fn scaffold_cloud_functions_writes_all_files_to_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = scaffold_project(tmp.path(), CLOUD_FUNCTIONS_TEMPLATE).unwrap();

        assert_eq!(result.actions.len(), 5);
        for action in &result.actions {
            assert!(
                matches!(action, ScaffoldAction::Created(_)),
                "all files should be created in empty dir"
            );
        }

        assert!(tmp.path().join("firebase.json").exists());
        assert!(tmp.path().join("functions/package.json").exists());
        assert!(tmp.path().join("functions/tsconfig.json").exists());
        assert!(tmp.path().join("functions/src/index.ts").exists());
        assert!(tmp.path().join(".gitignore").exists());

        let pkg = std::fs::read_to_string(tmp.path().join("functions/package.json")).unwrap();
        assert!(
            pkg.contains("\"firebase-functions\""),
            "functions/package.json should contain firebase-functions"
        );
        assert!(
            !pkg.contains("{{"),
            "functions/package.json should not have unresolved placeholders"
        );
    }

    #[test]
    fn scaffold_convex_skips_existing_files() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(tmp.path().join("package.json"), "{}").unwrap();
        std::fs::write(tmp.path().join("tsconfig.json"), "{}").unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "node_modules/\n").unwrap();

        let result = scaffold_project(tmp.path(), CONVEX_TEMPLATE).unwrap();

        let created: Vec<_> = result
            .actions
            .iter()
            .filter_map(|a| match a {
                ScaffoldAction::Created(p) => Some(p.as_str()),
                _ => None,
            })
            .collect();
        let skipped: Vec<_> = result
            .actions
            .iter()
            .filter_map(|a| match a {
                ScaffoldAction::Skipped(p) => Some(p.as_str()),
                _ => None,
            })
            .collect();

        assert!(created.contains(&"convex/schema.ts"));
        assert!(created.contains(&"convex/messages.ts"));
        assert!(skipped.contains(&"package.json"));
        assert!(skipped.contains(&"tsconfig.json"));
        assert!(skipped.contains(&".gitignore"));

        assert_eq!(
            std::fs::read_to_string(tmp.path().join("package.json")).unwrap(),
            "{}",
            "existing package.json should not be overwritten"
        );
    }

    #[test]
    fn scaffold_cloud_functions_skips_existing_firebase_json() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(tmp.path().join("firebase.json"), "{}").unwrap();

        let result = scaffold_project(tmp.path(), CLOUD_FUNCTIONS_TEMPLATE).unwrap();

        let skipped: Vec<_> = result
            .actions
            .iter()
            .filter_map(|a| match a {
                ScaffoldAction::Skipped(p) => Some(p.as_str()),
                _ => None,
            })
            .collect();

        assert!(skipped.contains(&"firebase.json"));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("firebase.json")).unwrap(),
            "{}",
            "existing firebase.json should not be overwritten"
        );
    }

    #[test]
    fn scaffold_refuses_home_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let original_home = std::env::var("HOME").ok();
        // SAFETY: this test runs single-threaded for env mutation; restored below.
        unsafe { std::env::set_var("HOME", tmp.path()) };
        let result = scaffold_project(tmp.path(), CONVEX_TEMPLATE);
        match original_home {
            Some(h) => unsafe { std::env::set_var("HOME", h) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("home directory"),
            "should mention home directory"
        );
    }

    #[test]
    fn scaffold_refuses_root_directory() {
        let result = scaffold_project(Path::new("/"), CONVEX_TEMPLATE);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("root directory"),
            "should mention root directory"
        );
    }

    #[test]
    fn scaffold_refuses_tmp_directory() {
        let result = scaffold_project(Path::new("/tmp"), CONVEX_TEMPLATE);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("/tmp"), "should mention /tmp");
    }

    #[test]
    fn source_root_neovex_returns_advisory() {
        let result = check_source_root_flag("neovex");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("experimental"));
    }

    #[test]
    fn source_root_convex_is_accepted() {
        assert!(check_source_root_flag("convex").is_ok());
    }

    #[test]
    fn cli_parses_init_defaults() {
        let command = parse_init(["neovex", "init", "convex"]);

        assert_eq!(command.adapter, "convex");
        assert_eq!(command.directory, PathBuf::from("."));
        assert_eq!(command.source_root, "convex");
        assert!(!command.install);
    }

    #[test]
    fn cli_parses_init_install_flag() {
        let command = parse_init(["neovex", "init", "cloud-functions", "./my-app", "--install"]);

        assert_eq!(command.adapter, "cloud-functions");
        assert_eq!(command.directory, PathBuf::from("./my-app"));
        assert!(command.install);
    }

    #[test]
    fn init_install_failure_message_preserves_recovery_steps() {
        let message = format_init_install_failure(
            Path::new("/tmp/my-app"),
            &io::Error::other("npm install failed"),
        );

        assert!(message.contains("Starter project was created at /tmp/my-app"));
        assert!(message.contains("rerun `neovex dev`"));
        assert!(message.contains("run `npm install` manually"));
        assert!(message.contains("npm install failed"));
    }

    #[tokio::test]
    async fn init_command_scaffolds_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let command = InitCommand {
            directory: tmp.path().to_path_buf(),
            adapter: "convex".to_string(),
            source_root: "convex".to_string(),
            install: false,
        };
        run_init_command(command).await.unwrap();
        assert!(tmp.path().join("convex/schema.ts").exists());
        assert!(tmp.path().join("package.json").exists());
        assert!(
            !tmp.path().join("node_modules").exists(),
            "init should not install dependencies unless requested"
        );
        assert!(
            std::fs::read_to_string(tmp.path().join("convex/schema.ts"))
                .unwrap()
                .contains(".index(\"by_author\", [\"author\"])"),
            "starter schema should model indexed author queries canonically"
        );
    }

    #[tokio::test]
    async fn init_command_creates_target_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("my-app");
        let command = InitCommand {
            directory: target.clone(),
            adapter: "convex".to_string(),
            source_root: "convex".to_string(),
            install: false,
        };
        run_init_command(command).await.unwrap();
        assert!(target.join("convex/schema.ts").exists());
        assert!(target.join("package.json").exists());
        assert!(
            !target.join("node_modules").exists(),
            "init should leave dependency bootstrap to dev by default"
        );
        let pkg = std::fs::read_to_string(target.join("package.json")).unwrap();
        assert!(
            pkg.contains("\"name\": \"my-app\""),
            "project name should come from the directory name"
        );
    }

    #[tokio::test]
    async fn init_command_errors_when_source_root_exists() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("convex")).unwrap();
        let command = InitCommand {
            directory: tmp.path().to_path_buf(),
            adapter: "convex".to_string(),
            source_root: "convex".to_string(),
            install: false,
        };
        let err = run_init_command(command).await.unwrap_err();
        assert!(
            err.to_string().contains("already exists"),
            "should mention source root already exists"
        );
    }

    #[tokio::test]
    async fn init_command_rejects_neovex_source_root() {
        let tmp = tempfile::tempdir().unwrap();
        let command = InitCommand {
            directory: tmp.path().to_path_buf(),
            adapter: "convex".to_string(),
            source_root: "neovex".to_string(),
            install: false,
        };
        let err = run_init_command(command).await.unwrap_err();
        assert!(
            err.to_string().contains("experimental"),
            "should mention neovex source root is experimental"
        );
    }

    #[tokio::test]
    async fn init_cloud_functions_scaffolds_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let command = InitCommand {
            directory: tmp.path().to_path_buf(),
            adapter: "cloud-functions".to_string(),
            source_root: "convex".to_string(),
            install: false,
        };
        run_init_command(command).await.unwrap();
        assert!(tmp.path().join("firebase.json").exists());
        assert!(tmp.path().join("functions/src/index.ts").exists());
        assert!(tmp.path().join("functions/package.json").exists());
        assert!(
            !tmp.path().join("functions/node_modules").exists(),
            "cloud-functions init should not install dependencies unless requested"
        );
    }

    #[tokio::test]
    async fn init_cloud_functions_errors_when_firebase_json_exists() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("firebase.json"), "{}").unwrap();
        let command = InitCommand {
            directory: tmp.path().to_path_buf(),
            adapter: "cloud-functions".to_string(),
            source_root: "convex".to_string(),
            install: false,
        };
        let err = run_init_command(command).await.unwrap_err();
        assert!(
            err.to_string().contains("firebase.json already exists"),
            "should mention firebase.json already exists"
        );
    }

    #[test]
    fn adapter_npm_install_dir_convex_is_project_root() {
        let dir = Path::new("/project");
        assert_eq!(
            adapter_npm_install_dir(crate::node::Adapter::Convex, dir),
            PathBuf::from("/project")
        );
    }

    #[test]
    fn adapter_npm_install_dir_cloud_functions_is_functions_subdir() {
        let dir = Path::new("/project");
        assert_eq!(
            adapter_npm_install_dir(crate::node::Adapter::CloudFunctions, dir),
            PathBuf::from("/project/functions")
        );
    }

    #[test]
    fn check_adapter_already_exists_convex_ok_for_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(check_adapter_already_exists(crate::node::Adapter::Convex, tmp.path()).is_ok());
    }

    #[test]
    fn check_adapter_already_exists_cloud_functions_ok_for_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(
            check_adapter_already_exists(crate::node::Adapter::CloudFunctions, tmp.path()).is_ok()
        );
    }
}
