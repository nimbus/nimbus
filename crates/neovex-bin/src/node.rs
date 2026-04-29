use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cli_ux;

const NODE_DEPENDENCY_STATE_PATH: [&str; 4] = [".neovex", "cache", "node", "dependency-state.json"];
const NPM_LOCKFILE_CANDIDATES: [&str; 2] = ["package-lock.json", "npm-shrinkwrap.json"];
const REQUIRED_NODE_MAJOR_VERSION: u64 = 22;
const DEFAULT_FIREBASE_FUNCTIONS_SOURCE: &str = "functions";
const DEFAULT_FIREBASE_FUNCTIONS_CODEBASE: &str = "default";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Adapter {
    Convex,
    CloudFunctions,
}

impl Adapter {
    pub(crate) fn from_cli_arg(s: &str) -> Option<Adapter> {
        match s {
            "convex" => Some(Self::Convex),
            "cloud-functions" => Some(Self::CloudFunctions),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Convex => "convex",
            Self::CloudFunctions => "cloud-functions",
        }
    }

    pub(crate) fn needs_node_dependencies(self) -> bool {
        match self {
            Self::Convex | Self::CloudFunctions => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FirebaseFunctionsCodebase {
    pub(crate) codebase: String,
    pub(crate) source_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FirebaseFunctionsProject {
    codebases: Vec<FirebaseFunctionsCodebase>,
}

impl FirebaseFunctionsProject {
    pub(crate) fn source_dirs(&self) -> Vec<PathBuf> {
        self.codebases
            .iter()
            .map(|codebase| codebase.source_dir.clone())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct FirebaseProjectConfig {
    #[serde(default)]
    functions: Option<FirebaseFunctionsConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
enum FirebaseFunctionsConfig {
    Source(String),
    Descriptor(FirebaseFunctionsDescriptor),
    Descriptors(Vec<FirebaseFunctionsDescriptor>),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct FirebaseFunctionsDescriptor {
    source: Option<String>,
    codebase: Option<String>,
}

pub(crate) fn firebase_functions_project(
    app_dir: &Path,
) -> io::Result<Option<FirebaseFunctionsProject>> {
    let firebase_json_path = app_dir.join("firebase.json");
    if !firebase_json_path.is_file() {
        return Ok(None);
    }
    let config = read_firebase_project_config(&firebase_json_path)?;
    let codebases = normalize_firebase_functions_codebases(config.functions)?;
    let canonical_app_dir = app_dir.canonicalize().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to resolve Firebase project root {}: {error}",
                app_dir.display()
            ),
        )
    })?;
    let mut resolved = Vec::with_capacity(codebases.len());
    for descriptor in codebases {
        let source_dir = canonical_app_dir.join(&descriptor.source);
        let metadata = std::fs::metadata(&source_dir).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "Firebase Functions source directory {} does not exist or is not readable: {error}",
                    source_dir.display()
                ),
            )
        })?;
        if !metadata.is_dir() {
            return Err(io::Error::other(format!(
                "Firebase Functions source directory {} is not a directory",
                source_dir.display()
            )));
        }
        let canonical_source_dir = source_dir.canonicalize().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "failed to resolve Firebase Functions source directory {}: {error}",
                    source_dir.display()
                ),
            )
        })?;
        if !canonical_source_dir.starts_with(&canonical_app_dir) {
            return Err(io::Error::other(format!(
                "Firebase Functions source directory {} must stay inside the Firebase project root {}",
                canonical_source_dir.display(),
                canonical_app_dir.display()
            )));
        }
        resolved.push(FirebaseFunctionsCodebase {
            codebase: descriptor.codebase,
            source_dir: canonical_source_dir,
        });
    }
    Ok(Some(FirebaseFunctionsProject {
        codebases: resolved,
    }))
}

/// Install Node.js dependencies when declared authoring packages are missing.
pub(crate) async fn auto_install_node_dependencies(
    app_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    match node_dependency_install_action(app_dir)? {
        NodeDependencyInstallAction::Skip => Ok(()),
        NodeDependencyInstallAction::RecordState(state) => {
            persist_node_dependency_state(app_dir, &state)?;
            Ok(())
        }
        NodeDependencyInstallAction::Install(state) => {
            ensure_node22_runtime_available()?;
            ensure_npm_available()?;
            cli_ux::write_stderr_prefixed_line("info:", "running npm install")?;
            let status = tokio::process::Command::new("npm")
                .arg("install")
                .current_dir(app_dir)
                .status()
                .await
                .map_err(|e| {
                    io::Error::other(format!(
                        "failed to run npm install in {}: {e}. Install Node.js with npm to use Neovex authoring flows.",
                        app_dir.display()
                    ))
                })?;

            if !status.success() {
                return Err(io::Error::other(format!(
                    "npm install failed in {}. Resolve the npm error above, then rerun the Neovex command or run `npm install` manually.",
                    app_dir.display()
                ))
                .into());
            }

            persist_node_dependency_state(app_dir, &state)?;
            Ok(())
        }
    }
}

pub(crate) fn ensure_node22_runtime_available() -> io::Result<()> {
    let version = read_node_runtime_version()?;
    match validate_node_runtime_version(&version)? {
        NodeRuntimeVersionValidation::VerifiedBaseline => Ok(()),
        NodeRuntimeVersionValidation::NewerThanVerified => {
            let message = format!(
                "Neovex verifies Node.js {}.x for authoring flows; found Node.js {}. Proceeding with best-effort compatibility.",
                REQUIRED_NODE_MAJOR_VERSION, version
            );
            cli_ux::write_stderr_prefixed_line("warning:", &message)?;
            Ok(())
        }
    }
}

pub(crate) fn ensure_npm_available() -> io::Result<()> {
    run_version_command("npm", "--version")
        .map(|_| ())
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "Neovex authoring flows require npm on PATH when dependencies must be installed: {error}"
                ),
            )
        })
}

#[cfg(test)]
fn missing_required_node_packages(app_dir: &Path) -> io::Result<Vec<String>> {
    let Some(requirements) = collect_node_dependency_requirements(app_dir)? else {
        return Ok(Vec::new());
    };

    Ok(requirements
        .packages
        .into_iter()
        .filter(|package_name| !node_package_manifest_path(app_dir, package_name).is_file())
        .collect())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct NodeDependencyState {
    fingerprint: String,
    packages: Vec<String>,
    lockfile: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NodeDependencyInstallAction {
    Skip,
    RecordState(NodeDependencyState),
    Install(NodeDependencyState),
}

fn node_dependency_install_action(app_dir: &Path) -> io::Result<NodeDependencyInstallAction> {
    let Some(state) = collect_node_dependency_requirements(app_dir)? else {
        return Ok(NodeDependencyInstallAction::Skip);
    };
    let missing_packages: Vec<String> = state
        .packages
        .iter()
        .filter(|package_name| !node_package_manifest_path(app_dir, package_name).is_file())
        .cloned()
        .collect();
    if !missing_packages.is_empty() {
        return Ok(NodeDependencyInstallAction::Install(state));
    }

    match load_node_dependency_state(app_dir)? {
        Some(saved) if saved == state => Ok(NodeDependencyInstallAction::Skip),
        Some(_) => Ok(NodeDependencyInstallAction::Install(state)),
        None => Ok(NodeDependencyInstallAction::RecordState(state)),
    }
}

fn collect_node_dependency_requirements(app_dir: &Path) -> io::Result<Option<NodeDependencyState>> {
    let package_json_path = app_dir.join("package.json");
    if !package_json_path.is_file() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&package_json_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to read package.json at {}: {error}",
                package_json_path.display()
            ),
        )
    })?;
    let parsed: serde_json::Value = serde_json::from_str(&content).map_err(|error| {
        io::Error::other(format!(
            "package.json at {} is not valid JSON: {error}",
            package_json_path.display()
        ))
    })?;

    let mut packages = BTreeSet::new();
    collect_dependency_names(&parsed, "dependencies", &package_json_path, &mut packages)?;
    collect_dependency_names(
        &parsed,
        "devDependencies",
        &package_json_path,
        &mut packages,
    )?;
    if packages.is_empty() {
        return Ok(None);
    }

    let (lockfile_name, lockfile_content) = read_preferred_lockfile(app_dir)?;
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    if let Some(lockfile_name) = &lockfile_name {
        hasher.update(b"\n--lockfile-name--\n");
        hasher.update(lockfile_name.as_bytes());
    }
    if let Some(lockfile_content) = &lockfile_content {
        hasher.update(b"\n--lockfile-content--\n");
        hasher.update(lockfile_content.as_bytes());
    }
    let fingerprint = format!("{:x}", hasher.finalize());

    Ok(Some(NodeDependencyState {
        fingerprint,
        packages: packages.into_iter().collect(),
        lockfile: lockfile_name,
    }))
}

fn read_preferred_lockfile(app_dir: &Path) -> io::Result<(Option<String>, Option<String>)> {
    for candidate in NPM_LOCKFILE_CANDIDATES {
        let path = app_dir.join(candidate);
        if path.is_file() {
            let content = std::fs::read_to_string(&path).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("failed to read lockfile {}: {error}", path.display()),
                )
            })?;
            return Ok((Some(candidate.to_string()), Some(content)));
        }
    }
    Ok((None, None))
}

fn node_dependency_state_path(app_dir: &Path) -> std::path::PathBuf {
    NODE_DEPENDENCY_STATE_PATH
        .iter()
        .fold(app_dir.to_path_buf(), |path, segment| path.join(segment))
}

fn load_node_dependency_state(app_dir: &Path) -> io::Result<Option<NodeDependencyState>> {
    let path = node_dependency_state_path(app_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to read node dependency state {}: {error}",
                path.display()
            ),
        )
    })?;
    match serde_json::from_str(&content) {
        Ok(state) => Ok(Some(state)),
        Err(_) => Ok(None),
    }
}

fn persist_node_dependency_state(app_dir: &Path, state: &NodeDependencyState) -> io::Result<()> {
    let path = node_dependency_state_path(app_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "failed to prepare node dependency cache directory {}: {error}",
                    parent.display()
                ),
            )
        })?;
    }
    let content = serde_json::to_string_pretty(state).map_err(|error| {
        io::Error::other(format!(
            "failed to serialize node dependency state for {}: {error}",
            app_dir.display()
        ))
    })?;
    std::fs::write(&path, content).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to write node dependency state {}: {error}",
                path.display()
            ),
        )
    })
}

fn collect_dependency_names(
    parsed: &serde_json::Value,
    field_name: &str,
    package_json_path: &Path,
    packages: &mut BTreeSet<String>,
) -> io::Result<()> {
    let Some(value) = parsed.get(field_name) else {
        return Ok(());
    };
    if value.is_null() {
        return Ok(());
    }
    let Some(object) = value.as_object() else {
        return Err(io::Error::other(format!(
            "package.json field `{field_name}` at {} must be an object",
            package_json_path.display()
        )));
    };
    packages.extend(object.keys().cloned());
    Ok(())
}

fn node_package_manifest_path(app_dir: &Path, package_name: &str) -> std::path::PathBuf {
    app_dir
        .join("node_modules")
        .join(package_name)
        .join("package.json")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedFirebaseFunctionsCodebase {
    source: String,
    codebase: String,
}

fn read_firebase_project_config(firebase_json_path: &Path) -> io::Result<FirebaseProjectConfig> {
    let content = std::fs::read_to_string(firebase_json_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to read firebase.json at {}: {error}",
                firebase_json_path.display()
            ),
        )
    })?;
    serde_json::from_str(&content).map_err(|error| {
        io::Error::other(format!(
            "firebase.json at {} is not valid JSON: {error}",
            firebase_json_path.display()
        ))
    })
}

fn normalize_firebase_functions_codebases(
    functions: Option<FirebaseFunctionsConfig>,
) -> io::Result<Vec<NormalizedFirebaseFunctionsCodebase>> {
    let descriptors = match functions {
        None => vec![NormalizedFirebaseFunctionsCodebase {
            source: DEFAULT_FIREBASE_FUNCTIONS_SOURCE.to_string(),
            codebase: DEFAULT_FIREBASE_FUNCTIONS_CODEBASE.to_string(),
        }],
        Some(FirebaseFunctionsConfig::Source(source)) => {
            vec![NormalizedFirebaseFunctionsCodebase {
                source: normalize_non_empty_string(source, "firebase.json functions source")?,
                codebase: DEFAULT_FIREBASE_FUNCTIONS_CODEBASE.to_string(),
            }]
        }
        Some(FirebaseFunctionsConfig::Descriptor(descriptor)) => {
            vec![normalize_firebase_functions_descriptor(descriptor)?]
        }
        Some(FirebaseFunctionsConfig::Descriptors(descriptors)) => descriptors
            .into_iter()
            .map(normalize_firebase_functions_descriptor)
            .collect::<io::Result<Vec<_>>>()?,
    };
    if descriptors.is_empty() {
        return Err(io::Error::other(
            "firebase.json functions array must contain at least one descriptor",
        ));
    }

    let mut seen = BTreeSet::new();
    for descriptor in &descriptors {
        if !seen.insert(descriptor.codebase.clone()) {
            return Err(io::Error::other(format!(
                "firebase.json reuses Functions codebase `{}`",
                descriptor.codebase
            )));
        }
    }
    Ok(descriptors)
}

fn normalize_firebase_functions_descriptor(
    descriptor: FirebaseFunctionsDescriptor,
) -> io::Result<NormalizedFirebaseFunctionsCodebase> {
    Ok(NormalizedFirebaseFunctionsCodebase {
        source: normalize_non_empty_string(
            descriptor
                .source
                .unwrap_or_else(|| DEFAULT_FIREBASE_FUNCTIONS_SOURCE.to_string()),
            "firebase.json functions source",
        )?,
        codebase: normalize_non_empty_string(
            descriptor
                .codebase
                .unwrap_or_else(|| DEFAULT_FIREBASE_FUNCTIONS_CODEBASE.to_string()),
            "firebase.json functions codebase",
        )?,
    })
}

fn normalize_non_empty_string(value: String, label: &str) -> io::Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(io::Error::other(format!(
            "{label} must be a non-empty string"
        )));
    }
    Ok(trimmed.to_string())
}

fn read_node_runtime_version() -> io::Result<Version> {
    parse_node_runtime_version(&run_version_command("node", "--version")?)
}

fn run_version_command(program: &str, arg: &str) -> io::Result<String> {
    let output = std::process::Command::new(program)
        .arg(arg)
        .output()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("failed to run `{program} {arg}`: {error}"),
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let suffix = if stderr.is_empty() {
            String::new()
        } else {
            format!(": {stderr}")
        };
        return Err(io::Error::other(format!(
            "`{program} {arg}` exited with status {}{}",
            output.status, suffix
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_node_runtime_version(output: &str) -> io::Result<Version> {
    let trimmed = output.trim();
    let normalized = trimmed.strip_prefix('v').unwrap_or(trimmed);
    Version::parse(normalized).map_err(|error| {
        io::Error::other(format!(
            "failed to parse Node.js version from `node --version` output `{trimmed}`: {error}"
        ))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeRuntimeVersionValidation {
    VerifiedBaseline,
    NewerThanVerified,
}

fn validate_node_runtime_version(version: &Version) -> io::Result<NodeRuntimeVersionValidation> {
    if version.major < REQUIRED_NODE_MAJOR_VERSION {
        return Err(io::Error::other(format!(
            "Neovex authoring flows require Node.js {}.x or newer; found Node.js {}.",
            REQUIRED_NODE_MAJOR_VERSION, version
        )));
    }
    if version.major == REQUIRED_NODE_MAJOR_VERSION {
        return Ok(NodeRuntimeVersionValidation::VerifiedBaseline);
    }
    Ok(NodeRuntimeVersionValidation::NewerThanVerified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_from_cli_arg_convex() {
        assert_eq!(Adapter::from_cli_arg("convex"), Some(Adapter::Convex));
    }

    #[test]
    fn adapter_from_cli_arg_cloud_functions() {
        assert_eq!(
            Adapter::from_cli_arg("cloud-functions"),
            Some(Adapter::CloudFunctions)
        );
    }

    #[test]
    fn adapter_from_cli_arg_unknown() {
        assert_eq!(Adapter::from_cli_arg("unknown"), None);
    }

    #[test]
    fn adapter_round_trips_through_name() {
        for adapter in [Adapter::Convex, Adapter::CloudFunctions] {
            assert_eq!(Adapter::from_cli_arg(adapter.name()), Some(adapter));
        }
    }

    #[test]
    fn all_adapters_need_node_dependencies() {
        assert!(Adapter::Convex.needs_node_dependencies());
        assert!(Adapter::CloudFunctions.needs_node_dependencies());
    }

    #[tokio::test]
    async fn auto_install_skips_when_no_package_json() {
        let temp = tempfile::tempdir().unwrap();
        auto_install_node_dependencies(temp.path())
            .await
            .expect("should be a no-op without package.json");
        assert!(
            !temp.path().join("node_modules").exists(),
            "should not create node_modules"
        );
    }

    #[tokio::test]
    async fn auto_install_skips_when_no_packages_are_declared() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        auto_install_node_dependencies(temp.path())
            .await
            .expect("should be a no-op when no packages are declared");
    }

    #[test]
    fn missing_required_node_packages_reports_declared_packages_without_install() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{
  "dependencies": {
    "convex": "^1.0.0"
  },
  "devDependencies": {
    "@neovex/codegen": "^1.0.0"
  }
}"#,
        )
        .unwrap();

        let missing = missing_required_node_packages(temp.path()).unwrap();

        assert_eq!(
            missing,
            vec!["@neovex/codegen".to_string(), "convex".to_string()]
        );
    }

    #[test]
    fn missing_required_node_packages_ignores_installed_packages() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{
  "dependencies": {
    "convex": "^1.0.0"
  },
  "devDependencies": {
    "@neovex/codegen": "^1.0.0"
  }
}"#,
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("node_modules/convex")).unwrap();
        std::fs::write(temp.path().join("node_modules/convex/package.json"), "{}").unwrap();
        std::fs::create_dir_all(temp.path().join("node_modules/@neovex/codegen")).unwrap();
        std::fs::write(
            temp.path()
                .join("node_modules/@neovex/codegen/package.json"),
            "{}",
        )
        .unwrap();

        let missing = missing_required_node_packages(temp.path()).unwrap();

        assert!(
            missing.is_empty(),
            "all declared packages should be present"
        );
    }

    #[test]
    fn missing_required_node_packages_does_not_trust_node_modules_directory_alone() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{
  "dependencies": {
    "convex": "^1.0.0"
  }
}"#,
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("node_modules")).unwrap();

        let missing = missing_required_node_packages(temp.path()).unwrap();

        assert_eq!(missing, vec!["convex".to_string()]);
    }

    #[test]
    fn missing_required_node_packages_errors_on_invalid_package_json() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("package.json"), "{not-json").unwrap();

        let error = missing_required_node_packages(temp.path()).unwrap_err();

        assert!(
            error.to_string().contains("not valid JSON"),
            "error should mention invalid JSON, got: {error}"
        );
    }

    #[test]
    fn dependency_install_action_records_state_when_packages_are_present() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{
  "dependencies": {
    "convex": "^1.0.0"
  }
}"#,
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("node_modules/convex")).unwrap();
        std::fs::write(temp.path().join("node_modules/convex/package.json"), "{}").unwrap();

        let action = node_dependency_install_action(temp.path()).unwrap();

        assert!(matches!(
            action,
            NodeDependencyInstallAction::RecordState(_)
        ));
    }

    #[test]
    fn dependency_install_action_requires_install_when_lockfile_fingerprint_changes() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{
  "dependencies": {
    "convex": "^1.0.0"
  }
}"#,
        )
        .unwrap();
        std::fs::write(
            temp.path().join("package-lock.json"),
            r#"{"lockfileVersion":3}"#,
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("node_modules/convex")).unwrap();
        std::fs::write(temp.path().join("node_modules/convex/package.json"), "{}").unwrap();

        let state = collect_node_dependency_requirements(temp.path())
            .unwrap()
            .expect("dependency state should exist");
        persist_node_dependency_state(temp.path(), &state).unwrap();
        std::fs::write(
            temp.path().join("package-lock.json"),
            r#"{"lockfileVersion":3,"packages":{"":{}}}"#,
        )
        .unwrap();

        let action = node_dependency_install_action(temp.path()).unwrap();

        assert!(matches!(action, NodeDependencyInstallAction::Install(_)));
    }

    #[test]
    fn dependency_install_action_skips_when_state_matches() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{
  "dependencies": {
    "convex": "^1.0.0"
  }
}"#,
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("node_modules/convex")).unwrap();
        std::fs::write(temp.path().join("node_modules/convex/package.json"), "{}").unwrap();

        let state = collect_node_dependency_requirements(temp.path())
            .unwrap()
            .expect("dependency state should exist");
        persist_node_dependency_state(temp.path(), &state).unwrap();

        let action = node_dependency_install_action(temp.path()).unwrap();

        assert_eq!(action, NodeDependencyInstallAction::Skip);
    }

    #[test]
    fn firebase_functions_project_defaults_to_functions() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("firebase.json"), "{}").unwrap();
        std::fs::create_dir_all(temp.path().join("functions")).unwrap();

        assert_eq!(
            firebase_functions_project(temp.path()).unwrap(),
            Some(FirebaseFunctionsProject {
                codebases: vec![FirebaseFunctionsCodebase {
                    codebase: "default".to_string(),
                    source_dir: temp.path().join("functions").canonicalize().unwrap(),
                }],
            })
        );
    }

    #[test]
    fn firebase_functions_project_uses_configured_source_directory() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("backend/functions")).unwrap();
        std::fs::write(
            temp.path().join("firebase.json"),
            r#"{"functions":{"source":"backend/functions"}}"#,
        )
        .unwrap();

        assert_eq!(
            firebase_functions_project(temp.path()).unwrap(),
            Some(FirebaseFunctionsProject {
                codebases: vec![FirebaseFunctionsCodebase {
                    codebase: "default".to_string(),
                    source_dir: temp
                        .path()
                        .join("backend/functions")
                        .canonicalize()
                        .unwrap(),
                }],
            })
        );
    }

    #[test]
    fn firebase_functions_project_preserves_multiple_codebases() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("packages/app-functions")).unwrap();
        std::fs::create_dir_all(temp.path().join("packages/admin-functions")).unwrap();
        std::fs::write(
            temp.path().join("firebase.json"),
            r#"{
  "functions": [
    { "source": "packages/app-functions", "codebase": "app" },
    { "source": "packages/admin-functions", "codebase": "admin" }
  ]
}"#,
        )
        .unwrap();

        assert_eq!(
            firebase_functions_project(temp.path()).unwrap(),
            Some(FirebaseFunctionsProject {
                codebases: vec![
                    FirebaseFunctionsCodebase {
                        codebase: "app".to_string(),
                        source_dir: temp
                            .path()
                            .join("packages/app-functions")
                            .canonicalize()
                            .unwrap(),
                    },
                    FirebaseFunctionsCodebase {
                        codebase: "admin".to_string(),
                        source_dir: temp
                            .path()
                            .join("packages/admin-functions")
                            .canonicalize()
                            .unwrap(),
                    },
                ],
            })
        );
    }

    #[test]
    fn firebase_functions_project_rejects_duplicate_codebases() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("packages/app-functions")).unwrap();
        std::fs::create_dir_all(temp.path().join("packages/admin-functions")).unwrap();
        std::fs::write(
            temp.path().join("firebase.json"),
            r#"{
  "functions": [
    { "source": "packages/app-functions", "codebase": "shared" },
    { "source": "packages/admin-functions", "codebase": "shared" }
  ]
}"#,
        )
        .unwrap();

        let error = firebase_functions_project(temp.path()).unwrap_err();
        assert!(
            error.to_string().contains("reuses Functions codebase"),
            "unexpected duplicate-codebase error: {error}"
        );
    }

    #[test]
    fn parse_node_runtime_version_accepts_v_prefixed_semver() {
        assert_eq!(
            parse_node_runtime_version("v22.3.1").unwrap(),
            Version::parse("22.3.1").unwrap()
        );
    }

    #[test]
    fn parse_node_runtime_version_rejects_non_semver_output() {
        let error = parse_node_runtime_version("node-22").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed to parse Node.js version from `node --version` output"),
            "unexpected parse error: {error}"
        );
    }

    #[test]
    fn validate_node_runtime_version_rejects_older_than_node22_baseline() {
        let error = validate_node_runtime_version(&Version::parse("20.18.1").unwrap()).unwrap_err();
        assert!(
            error.to_string().contains("require Node.js 22.x or newer"),
            "unexpected version validation error: {error}"
        );
    }

    #[test]
    fn validate_node_runtime_version_allows_newer_runtime_with_warning_status() {
        assert_eq!(
            validate_node_runtime_version(&Version::parse("25.9.0").unwrap()).unwrap(),
            NodeRuntimeVersionValidation::NewerThanVerified
        );
    }
}
