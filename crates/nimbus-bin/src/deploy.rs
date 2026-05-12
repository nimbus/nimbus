use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli_ux;
use crate::codegen::run_codegen_for_app_dir;

const DEPLOY_URL_ENV: &str = "NIMBUS_DEPLOY_URL";
const DEPLOY_TOKEN_ENV: &str = "NIMBUS_DEPLOY_TOKEN";

/// Push app artifacts to an explicit self-hosted Nimbus instance.
#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = crate::cli_ux::DEPLOY_HELP_EXAMPLES
)]
pub(crate) struct DeployCommand {
    /// Target Nimbus server URL. Defaults to NIMBUS_DEPLOY_URL.
    #[arg(long)]
    pub(crate) url: Option<String>,

    /// Deploy admin bearer token. Defaults to NIMBUS_DEPLOY_TOKEN.
    #[arg(long)]
    pub(crate) token: Option<String>,

    /// App directory containing a nimbus/ or convex/ source root.
    #[arg(long)]
    pub(crate) app_dir: Option<PathBuf>,

    /// Validate and diff without activating the new generation.
    #[arg(long, default_value_t = false)]
    pub(crate) dry_run: bool,

    /// Skip codegen and package already-generated artifacts.
    #[arg(long, default_value_t = false)]
    pub(crate) skip_codegen: bool,

    /// Show packaging and deploy phase detail.
    #[arg(long, default_value_t = false)]
    pub(crate) verbose: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct DeployRequest {
    pub(crate) dry_run: bool,
    pub(crate) artifacts: DeployArtifacts,
}

#[derive(Debug, Serialize)]
pub(crate) struct DeployArtifacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) convex: Option<ConvexDeployArtifacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cloud_functions: Option<CloudFunctionsDeployArtifacts>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ConvexDeployArtifacts {
    pub(crate) functions_json: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) http_routes_json: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) schema_json: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) auth_config_json: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bundle_mjs: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bundle_sha256: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct CloudFunctionsDeployArtifacts {
    pub(crate) artifact_json: Value,
    pub(crate) targets_json: Value,
    pub(crate) bundle_mjs: String,
    pub(crate) bundle_sha256: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeployResponse {
    pub(crate) dry_run: bool,
    pub(crate) activated: bool,
    pub(crate) generation: u64,
    pub(crate) previous_generation: u64,
    pub(crate) diff: DeployDiff,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeployDiff {
    pub(crate) functions: DeployFunctionDiff,
    pub(crate) http_routes: DeployHttpRouteDiff,
    pub(crate) schema_changed: bool,
    pub(crate) indexes_changed: bool,
    pub(crate) runtime_bundle_changed: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeployFunctionDiff {
    pub(crate) added: Vec<DeployFunctionChange>,
    pub(crate) changed: Vec<DeployFunctionChange>,
    pub(crate) removed: Vec<DeployFunctionChange>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeployHttpRouteDiff {
    pub(crate) added: Vec<DeployHttpRouteChange>,
    pub(crate) changed: Vec<DeployHttpRouteChange>,
    pub(crate) removed: Vec<DeployHttpRouteChange>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeployFunctionChange {
    pub(crate) name: String,
    pub(crate) kind: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DeployHttpRouteChange {
    pub(crate) key: String,
}

pub(crate) async fn run_deploy_command(
    command: DeployCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = env::current_dir()?;
    let target_url = resolve_deploy_url(command.url.as_deref(), |name| env::var(name).ok())?;
    let token = resolve_deploy_token(command.token.as_deref(), |name| env::var(name).ok())?;
    let app_dir = resolve_deploy_app_dir(command.app_dir.as_deref(), &cwd)?;

    emit_deploy_phase(format!("Preparing Nimbus app from {}", app_dir.display()));
    if command.skip_codegen {
        emit_deploy_phase("Using existing generated artifacts because --skip-codegen is set");
    } else {
        emit_deploy_phase("Running codegen");
        run_codegen_for_app_dir(&app_dir).await?;
    }

    if command.verbose {
        emit_deploy_phase("Packaging generated app artifacts");
    }
    let request = DeployRequest::from_app_dir(&app_dir, command.dry_run)?;

    emit_deploy_phase(format!("Uploading app artifacts to {target_url}"));
    let response = post_deploy_request(&target_url, &token, &request).await?;
    print_deploy_result(&target_url, &response);
    Ok(())
}

impl DeployDiff {
    pub(crate) fn human_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        append_function_lines(&mut lines, "+", &self.functions.added);
        append_function_lines(&mut lines, "~", &self.functions.changed);
        append_function_lines(&mut lines, "-", &self.functions.removed);
        append_route_lines(&mut lines, "+", &self.http_routes.added);
        append_route_lines(&mut lines, "~", &self.http_routes.changed);
        append_route_lines(&mut lines, "-", &self.http_routes.removed);
        if self.schema_changed {
            lines.push("~ schema".to_string());
        }
        if self.indexes_changed {
            lines.push("~ indexes".to_string());
        }
        if self.runtime_bundle_changed {
            lines.push("~ runtime bundle".to_string());
        }
        lines
    }
}

fn resolve_deploy_url(
    explicit_url: Option<&str>,
    env_lookup: impl Fn(&str) -> Option<String>,
) -> Result<String, Box<dyn std::error::Error>> {
    let url = explicit_url
        .map(str::to_owned)
        .or_else(|| env_lookup(DEPLOY_URL_ENV))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "nimbus deploy requires --url or NIMBUS_DEPLOY_URL",
            )
        })?;
    let trimmed = url.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "deploy target URL must not be empty",
        )
        .into());
    }
    Ok(trimmed)
}

fn resolve_deploy_token(
    explicit_token: Option<&str>,
    env_lookup: impl Fn(&str) -> Option<String>,
) -> Result<String, Box<dyn std::error::Error>> {
    let token = explicit_token
        .map(str::to_owned)
        .or_else(|| env_lookup(DEPLOY_TOKEN_ENV))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "nimbus deploy requires --token or NIMBUS_DEPLOY_TOKEN",
            )
        })?;
    if token.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "deploy admin token must not be empty",
        )
        .into());
    }
    Ok(token)
}

pub(crate) fn resolve_deploy_app_dir(
    explicit_app_dir: Option<&Path>,
    cwd: &Path,
) -> io::Result<PathBuf> {
    let selected = explicit_app_dir
        .map(|path| resolve_unchecked_path(path, cwd))
        .unwrap_or_else(|| detect_app_dir(cwd));
    canonicalize_dir(&selected)
}

fn detect_app_dir(cwd: &Path) -> PathBuf {
    for candidate in cwd.ancestors() {
        if candidate.join("nimbus").is_dir()
            || candidate.join("convex").is_dir()
            || candidate.join("firebase.json").is_file()
            || package_declares_functions_framework(&candidate.join("package.json"))
            || candidate
                .join(".nimbus")
                .join("convex")
                .join("functions.json")
                .is_file()
            || candidate
                .join(".nimbus")
                .join("firebase")
                .join("artifact.json")
                .is_file()
        {
            return candidate.to_path_buf();
        }
    }
    cwd.to_path_buf()
}

fn package_declares_functions_framework(package_json_path: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(package_json_path) else {
        return false;
    };
    let Ok(package_json) = serde_json::from_str::<Value>(&contents) else {
        return false;
    };
    [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ]
    .into_iter()
    .any(|field| {
        package_json
            .get(field)
            .and_then(Value::as_object)
            .is_some_and(|deps| deps.contains_key("@google-cloud/functions-framework"))
    })
}

fn resolve_unchecked_path(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn canonicalize_dir(path: &Path) -> io::Result<PathBuf> {
    let metadata = fs::metadata(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("app directory {} is not readable: {error}", path.display()),
        )
    })?;
    if !metadata.is_dir() {
        return Err(io::Error::other(format!(
            "app path {} is not a directory",
            path.display()
        )));
    }
    path.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to resolve app directory {}: {error}",
                path.display()
            ),
        )
    })
}

fn emit_deploy_phase(message: impl AsRef<str>) {
    let _ = cli_ux::write_stderr_line(message.as_ref());
}

fn print_deploy_result(target_url: &str, response: &DeployResponse) {
    print!("{}", render_deploy_result(target_url, response));
}

fn render_deploy_result(target_url: &str, response: &DeployResponse) -> String {
    let mut output = String::new();
    if response.dry_run {
        output.push_str(&format!(
            "Validated Nimbus app for {target_url} (generation {})\n",
            response.generation
        ));
    } else {
        output.push_str(&format!(
            "Deployed Nimbus app to {target_url} (generation {} from {})\n",
            response.generation, response.previous_generation
        ));
    }

    let change_lines = response.diff.human_lines();
    if change_lines.is_empty() {
        output.push_str("\nNo app surface changes reported.\n");
    } else {
        output.push_str("\nChanges:\n");
        for line in change_lines {
            output.push_str("  ");
            output.push_str(&line);
            output.push('\n');
        }
    }
    output
}

impl DeployRequest {
    pub(crate) fn from_app_dir(
        app_dir: &Path,
        dry_run: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            dry_run,
            artifacts: DeployArtifacts::from_app_dir(app_dir)?,
        })
    }
}

fn append_function_lines(
    lines: &mut Vec<String>,
    marker: &str,
    functions: &[DeployFunctionChange],
) {
    lines.extend(
        functions
            .iter()
            .map(|function| format!("{marker} {} {}", function.name, function.kind)),
    );
}

fn append_route_lines(lines: &mut Vec<String>, marker: &str, routes: &[DeployHttpRouteChange]) {
    lines.extend(
        routes
            .iter()
            .map(|route| format!("{marker} route {}", route.key)),
    );
}

impl DeployArtifacts {
    pub(crate) fn from_app_dir(app_dir: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let convex = package_convex_artifacts(app_dir)?;
        let cloud_functions = package_cloud_functions_artifacts(app_dir)?;
        if convex.is_none() && cloud_functions.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "no generated deploy artifacts found under {} or {}",
                    generated_convex_dir(app_dir).display(),
                    generated_cloud_functions_dir(app_dir).display(),
                ),
            )
            .into());
        }

        Ok(Self {
            convex,
            cloud_functions,
        })
    }
}

pub(crate) async fn post_deploy_request(
    base_url: &str,
    token: &str,
    request: &DeployRequest,
) -> Result<DeployResponse, Box<dyn std::error::Error>> {
    let response = reqwest::Client::new()
        .post(deploy_endpoint_url(base_url))
        .bearer_auth(token)
        .json(request)
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(
            io::Error::other(format!("deploy request failed with {status}: {body}")).into(),
        );
    }
    Ok(response.json::<DeployResponse>().await?)
}

pub(crate) fn deploy_endpoint_url(base_url: &str) -> String {
    format!("{}/api/admin/deploy", base_url.trim_end_matches('/'))
}

fn generated_convex_dir(app_dir: &Path) -> PathBuf {
    app_dir.join(".nimbus").join("convex")
}

fn generated_cloud_functions_dir(app_dir: &Path) -> PathBuf {
    app_dir.join(".nimbus").join("firebase")
}

fn package_convex_artifacts(
    app_dir: &Path,
) -> Result<Option<ConvexDeployArtifacts>, Box<dyn std::error::Error>> {
    let generated_dir = generated_convex_dir(app_dir);
    let Some(functions_json) = read_optional_json(&generated_dir.join("functions.json"))? else {
        return Ok(None);
    };
    let http_routes_json = read_optional_json(&generated_dir.join("http_routes.json"))?;
    let schema_json = read_optional_json(&generated_dir.join("schema.json"))?;
    let auth_config_json = read_optional_json(&generated_dir.join("auth.config.json"))?;
    let bundle_mjs = read_optional_text(&generated_dir.join("bundle.mjs"))?;
    let bundle_sha256 = read_optional_text(&generated_dir.join("bundle.sha256"))?;
    if bundle_mjs.is_some() != bundle_sha256.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "runtime bundle artifacts in {} must include both bundle.mjs and bundle.sha256",
                generated_dir.display()
            ),
        )
        .into());
    }

    Ok(Some(ConvexDeployArtifacts {
        functions_json,
        http_routes_json,
        schema_json,
        auth_config_json,
        bundle_mjs,
        bundle_sha256,
    }))
}

fn package_cloud_functions_artifacts(
    app_dir: &Path,
) -> Result<Option<CloudFunctionsDeployArtifacts>, Box<dyn std::error::Error>> {
    let generated_dir = generated_cloud_functions_dir(app_dir);
    let Some(artifact_json) = read_optional_json(&generated_dir.join("artifact.json"))? else {
        return Ok(None);
    };
    let targets_json = read_required_json(&generated_dir.join("targets.json"))?;
    let bundle_mjs = read_required_text(&generated_dir.join("bundle.mjs"))?;
    let bundle_sha256 = read_required_text(&generated_dir.join("bundle.sha256"))?;
    Ok(Some(CloudFunctionsDeployArtifacts {
        artifact_json,
        targets_json,
        bundle_mjs,
        bundle_sha256,
    }))
}

fn read_required_json(path: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    let contents = fs::read_to_string(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to read required deploy artifact {}: {error}",
                path.display()
            ),
        )
    })?;
    parse_json_file(path, &contents)
}

fn read_optional_json(path: &Path) -> Result<Option<Value>, Box<dyn std::error::Error>> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("failed to read deploy artifact {}: {error}", path.display()),
            )
            .into());
        }
    };
    Ok(Some(parse_json_file(path, &contents)?))
}

fn parse_json_file(path: &Path, contents: &str) -> Result<Value, Box<dyn std::error::Error>> {
    serde_json::from_str(contents).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "failed to parse deploy artifact {}: {error}",
                path.display()
            ),
        )
        .into()
    })
}

fn read_required_text(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    fs::read_to_string(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to read required deploy artifact {}: {error}",
                path.display()
            ),
        )
        .into()
    })
}

fn read_optional_text(path: &Path) -> Result<Option<String>, Box<dyn std::error::Error>> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!("failed to read deploy artifact {}: {error}", path.display()),
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use clap::{Parser, error::ErrorKind};
    use serde_json::json;

    use super::*;
    use crate::{Cli, Command};

    fn parse_deploy<I, T>(args: I) -> DeployCommand
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let cli = Cli::parse_from(args);
        let Command::Deploy(command) = cli.command else {
            panic!("deploy subcommand should parse");
        };
        command
    }

    #[test]
    fn cli_parses_deploy_defaults() {
        let command = parse_deploy(["nimbus", "deploy"]);

        assert_eq!(command.url, None);
        assert_eq!(command.token, None);
        assert_eq!(command.app_dir, None);
        assert!(!command.dry_run);
        assert!(!command.skip_codegen);
        assert!(!command.verbose);
    }

    #[test]
    fn cli_parses_deploy_overrides() {
        let command = parse_deploy([
            "nimbus",
            "deploy",
            "--url",
            "http://localhost:3210/",
            "--token",
            "secret",
            "--app-dir",
            "./app",
            "--dry-run",
            "--skip-codegen",
            "--verbose",
        ]);

        assert_eq!(command.url.as_deref(), Some("http://localhost:3210/"));
        assert_eq!(command.token.as_deref(), Some("secret"));
        assert_eq!(command.app_dir, Some(PathBuf::from("./app")));
        assert!(command.dry_run);
        assert!(command.skip_codegen);
        assert!(command.verbose);
    }

    #[test]
    fn deploy_help_describes_explicit_target() {
        let error =
            Cli::try_parse_from(["nimbus", "deploy", "--help"]).expect_err("help should render");
        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
        let rendered = error.to_string();

        assert!(rendered.contains("--url"));
        assert!(rendered.contains("NIMBUS_DEPLOY_URL"));
        assert!(rendered.contains("--token"));
        assert!(rendered.contains("NIMBUS_DEPLOY_TOKEN"));
        assert!(rendered.contains("--dry-run"));
    }

    #[test]
    fn deploy_target_resolution_requires_url_and_token() {
        let missing_url = resolve_deploy_url(None, |_| None).expect_err("url should be required");
        assert!(missing_url.to_string().contains("requires --url"));

        let url = resolve_deploy_url(None, |name| {
            (name == DEPLOY_URL_ENV).then(|| "http://localhost:3210/".to_string())
        })
        .expect("url should resolve from env");
        assert_eq!(url, "http://localhost:3210");

        let missing_token =
            resolve_deploy_token(None, |_| None).expect_err("token should be required");
        assert!(missing_token.to_string().contains("requires --token"));

        let token =
            resolve_deploy_token(Some("secret"), |_| None).expect("explicit token should resolve");
        assert_eq!(token, "secret");
    }

    #[test]
    fn deploy_app_dir_detection_walks_to_source_root_parent() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let source_root = temp.path().join("convex");
        fs::create_dir_all(&source_root).expect("source root should create");

        let app_dir =
            resolve_deploy_app_dir(None, &source_root).expect("app dir should auto-detect");

        assert_eq!(
            app_dir,
            temp.path()
                .canonicalize()
                .expect("tempdir should canonicalize")
        );
    }

    #[test]
    fn deploy_app_dir_detection_walks_to_firebase_project_root() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let functions_src = temp.path().join("functions").join("src");
        fs::create_dir_all(&functions_src).expect("firebase functions source should create");
        fs::write(
            temp.path().join("firebase.json"),
            r#"{"functions":{"source":"functions"}}"#,
        )
        .expect("firebase.json should write");

        let app_dir =
            resolve_deploy_app_dir(None, &functions_src).expect("firebase app dir should resolve");

        assert_eq!(
            app_dir,
            temp.path()
                .canonicalize()
                .expect("tempdir should canonicalize")
        );
    }

    #[test]
    fn deploy_app_dir_detection_walks_to_framework_package_root() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let source_dir = temp.path().join("src");
        fs::create_dir_all(&source_dir).expect("framework source should create");
        fs::write(
            temp.path().join("package.json"),
            r#"{
  "main": "dist/index.js",
  "dependencies": {
    "@google-cloud/functions-framework": "^3.4.5"
  }
}
"#,
        )
        .expect("package.json should write");

        let app_dir =
            resolve_deploy_app_dir(None, &source_dir).expect("framework app dir should resolve");

        assert_eq!(
            app_dir,
            temp.path()
                .canonicalize()
                .expect("tempdir should canonicalize")
        );
    }

    #[test]
    fn deploy_endpoint_url_appends_admin_route() {
        assert_eq!(
            deploy_endpoint_url("http://localhost:3210/"),
            "http://localhost:3210/api/admin/deploy"
        );
    }

    #[test]
    fn deploy_artifacts_package_generated_files() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let generated = temp.path().join(".nimbus").join("convex");
        fs::create_dir_all(&generated).expect("generated dir should create");
        fs::write(
            generated.join("functions.json"),
            r#"{"functions":[{"name":"messages:list","kind":"query","plan":{"table":"messages","filters":[],"order":null,"limit":10}}]}"#,
        )
        .expect("functions should write");
        fs::write(generated.join("http_routes.json"), r#"{"routes":[]}"#)
            .expect("routes should write");
        fs::write(generated.join("schema.json"), r#"{"tables":{}}"#).expect("schema should write");
        fs::write(generated.join("auth.config.json"), r#"{"providers":[]}"#)
            .expect("auth config should write");
        fs::write(generated.join("bundle.mjs"), "export const value = 1;\n")
            .expect("bundle should write");
        fs::write(generated.join("bundle.sha256"), "a".repeat(64))
            .expect("bundle hash should write");

        let artifacts =
            DeployArtifacts::from_app_dir(temp.path()).expect("artifacts should package");
        let convex = artifacts.convex.expect("convex artifacts should package");

        assert_eq!(
            convex.functions_json["functions"][0]["name"],
            json!("messages:list")
        );
        assert_eq!(convex.http_routes_json, Some(json!({ "routes": [] })));
        assert_eq!(convex.schema_json, Some(json!({ "tables": {} })));
        assert_eq!(convex.auth_config_json, Some(json!({ "providers": [] })));
        assert_eq!(
            convex.bundle_mjs.as_deref(),
            Some("export const value = 1;\n")
        );
        assert_eq!(
            convex.bundle_sha256.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    #[test]
    fn deploy_artifacts_require_bundle_pair() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let generated = temp.path().join(".nimbus").join("convex");
        fs::create_dir_all(&generated).expect("generated dir should create");
        fs::write(generated.join("functions.json"), r#"{"functions":[]}"#)
            .expect("functions should write");
        fs::write(generated.join("bundle.mjs"), "export const value = 1;\n")
            .expect("bundle should write");

        let error = DeployArtifacts::from_app_dir(temp.path())
            .expect_err("missing bundle hash should fail");

        assert!(
            error
                .to_string()
                .contains("must include both bundle.mjs and bundle.sha256")
        );
    }

    #[test]
    fn deploy_artifacts_package_cloud_functions_family() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let generated = temp.path().join(".nimbus").join("firebase");
        fs::create_dir_all(&generated).expect("generated dir should create");
        fs::write(generated.join("artifact.json"), r#"{"version":1,"family":"cloud_functions","runtime_bundle":{"entry_file":"bundle.mjs","sha256_file":"bundle.sha256"},"targets_manifest":"targets.json","import_resolution":{"strategy":"deploy_alias_layer","covered_specifiers":["@google-cloud/functions-framework","firebase-admin/app","firebase-admin/firestore","firebase-functions/v2","firebase-functions/v2/firestore","firebase-functions/v2/https"]}}"#)
            .expect("artifact manifest should write");
        fs::write(
            generated.join("targets.json"),
            r#"{"version":1,"targets":[]}"#,
        )
        .expect("targets should write");
        fs::write(generated.join("bundle.mjs"), "export const value = 1;\n")
            .expect("bundle should write");
        fs::write(generated.join("bundle.sha256"), "b".repeat(64))
            .expect("bundle hash should write");

        let artifacts =
            DeployArtifacts::from_app_dir(temp.path()).expect("artifacts should package");
        let cloud_functions = artifacts
            .cloud_functions
            .expect("cloud functions artifacts should package");

        assert_eq!(
            cloud_functions.targets_json,
            json!({ "version": 1, "targets": [] })
        );
        assert_eq!(
            cloud_functions.bundle_sha256,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
    }

    #[test]
    fn render_deploy_result_prints_human_diff() {
        let response: DeployResponse = serde_json::from_value(json!({
            "dry_run": false,
            "activated": true,
            "generation": 2,
            "previous_generation": 1,
            "diff": {
                "functions": {
                    "added": [{ "name": "messages:list", "kind": "query" }],
                    "changed": [],
                    "removed": []
                },
                "http_routes": {
                    "added": [{ "key": "GET /messages" }],
                    "changed": [],
                    "removed": []
                },
                "schema_changed": true,
                "indexes_changed": true,
                "runtime_bundle_changed": true
            }
        }))
        .expect("deploy response should parse");

        let rendered = render_deploy_result("http://localhost:3210", &response);

        assert!(rendered.contains("Deployed Nimbus app to http://localhost:3210"));
        assert!(rendered.contains("+ messages:list query"));
        assert!(rendered.contains("+ route GET /messages"));
        assert!(rendered.contains("~ schema"));
        assert!(rendered.contains("~ indexes"));
        assert!(rendered.contains("~ runtime bundle"));
    }
}
