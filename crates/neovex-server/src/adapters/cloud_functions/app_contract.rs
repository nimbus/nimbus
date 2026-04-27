use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use neovex_core::{Error, Result};
use serde::{Deserialize, Serialize};

use super::CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR;

const DEFAULT_FIREBASE_FUNCTIONS_SOURCE: &str = "functions";
const DEFAULT_FIREBASE_CODEBASE: &str = "default";
const FUNCTIONS_FRAMEWORK_PACKAGE: &str = "@google-cloud/functions-framework";
const DEFAULT_FRAMEWORK_ENTRYPOINTS: &[&str] = &[
    "index.js",
    "index.mjs",
    "index.cjs",
    "index.ts",
    "index.mts",
    "index.cts",
];
const COVERED_ADMIN_APP_METHODS: &[&str] = &["initializeApp", "getApp", "getApps", "deleteApp"];
const COVERED_ADMIN_FIRESTORE_METHODS: &[&str] = &["getFirestore"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloudFunctionsAppLayout {
    FirebaseProject(CloudFunctionsFirebaseProjectLayout),
    StandaloneFrameworkPackage(CloudFunctionsFrameworkPackageLayout),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CloudFunctionsResolvedAppRoot {
    pub(crate) app_dir: PathBuf,
    pub(crate) artifact_dir: PathBuf,
    pub(crate) layout: CloudFunctionsAppLayout,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) matched_codebase: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CloudFunctionsFirebaseProjectLayout {
    pub(crate) firebase_json: PathBuf,
    pub(crate) codebases: Vec<CloudFunctionsFirebaseCodebase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CloudFunctionsFirebaseCodebase {
    pub(crate) name: String,
    pub(crate) source_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CloudFunctionsFrameworkPackageLayout {
    pub(crate) package_json: PathBuf,
    pub(crate) entrypoint: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CloudFunctionsAdminImport {
    App,
    Firestore,
}

impl CloudFunctionsAdminImport {
    pub(crate) const fn specifier(self) -> &'static str {
        match self {
            Self::App => "firebase-admin/app",
            Self::Firestore => "firebase-admin/firestore",
        }
    }

    fn covered_methods(self) -> &'static [&'static str] {
        match self {
            Self::App => COVERED_ADMIN_APP_METHODS,
            Self::Firestore => COVERED_ADMIN_FIRESTORE_METHODS,
        }
    }
}

impl CloudFunctionsResolvedAppRoot {
    fn new(
        app_dir: PathBuf,
        layout: CloudFunctionsAppLayout,
        matched_codebase: Option<String>,
    ) -> Self {
        let artifact_dir = app_dir.join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR);
        Self {
            app_dir,
            artifact_dir,
            layout,
            matched_codebase,
        }
    }

    fn matches_start_inside_codebase(&self) -> bool {
        self.matched_codebase.is_some()
    }
}

enum DiscoveryMode {
    Auto,
    Explicit,
}

#[derive(Debug)]
struct FirebaseCandidate {
    resolved: CloudFunctionsResolvedAppRoot,
}

#[derive(Debug)]
struct FrameworkCandidate {
    resolved: CloudFunctionsResolvedAppRoot,
}

pub(crate) fn resolve_cloud_functions_app_root(
    explicit_app_dir: Option<&Path>,
    cwd: &Path,
) -> Result<CloudFunctionsResolvedAppRoot> {
    if let Some(explicit_app_dir) = explicit_app_dir {
        let selected = resolve_unchecked_path(explicit_app_dir, cwd);
        return discover_cloud_functions_app_root(
            &canonicalize_dir(&selected)?,
            DiscoveryMode::Explicit,
        );
    }

    discover_cloud_functions_app_root(&canonicalize_dir(cwd)?, DiscoveryMode::Auto)
}

pub(crate) fn covered_admin_app_methods() -> Vec<String> {
    COVERED_ADMIN_APP_METHODS
        .iter()
        .map(|method| (*method).to_string())
        .collect()
}

pub(crate) fn covered_admin_firestore_methods() -> Vec<String> {
    COVERED_ADMIN_FIRESTORE_METHODS
        .iter()
        .map(|method| (*method).to_string())
        .collect()
}

pub(crate) fn validate_admin_method_support(
    specifier: &str,
    method: &str,
) -> Result<CloudFunctionsAdminImport> {
    let import = match specifier {
        "firebase-admin/app" => CloudFunctionsAdminImport::App,
        "firebase-admin/firestore" => CloudFunctionsAdminImport::Firestore,
        _ => {
            return Err(Error::InvalidInput(format!(
                "cloud functions first slice does not cover `{}`; only `firebase-admin/app` and `firebase-admin/firestore` are supported",
                specifier
            )));
        }
    };

    if import.covered_methods().contains(&method) {
        return Ok(import);
    }

    Err(Error::InvalidInput(format!(
        "cloud functions first slice does not cover `{}` from `{}`; covered methods are {}",
        method,
        import.specifier(),
        import.covered_methods().join(", ")
    )))
}

fn discover_cloud_functions_app_root(
    start: &Path,
    mode: DiscoveryMode,
) -> Result<CloudFunctionsResolvedAppRoot> {
    let firebase_candidates = discover_firebase_candidates(start)?;
    let framework_candidates = discover_framework_candidates(start)?;

    match mode {
        DiscoveryMode::Explicit => {
            select_explicit_candidate(start, &firebase_candidates, &framework_candidates)
        }
        DiscoveryMode::Auto => {
            select_auto_candidate(start, &firebase_candidates, &framework_candidates)
        }
    }
}

fn select_explicit_candidate(
    start: &Path,
    firebase_candidates: &[FirebaseCandidate],
    framework_candidates: &[FrameworkCandidate],
) -> Result<CloudFunctionsResolvedAppRoot> {
    if let Some(firebase_candidate) = firebase_candidates.first()
        && let Some(framework_candidate) = framework_candidates.first()
        && firebase_candidate.resolved.app_dir == framework_candidate.resolved.app_dir
    {
        return Ok(firebase_candidate.resolved.clone());
    }

    if let Some(framework_candidate) = framework_candidates.first() {
        return Ok(framework_candidate.resolved.clone());
    }
    if let Some(firebase_candidate) = firebase_candidates.first() {
        return Ok(firebase_candidate.resolved.clone());
    }

    Err(Error::InvalidInput(format!(
        "could not resolve `{}` to a compatible Firebase project root or standalone Functions Framework package",
        start.display()
    )))
}

fn select_auto_candidate(
    start: &Path,
    firebase_candidates: &[FirebaseCandidate],
    framework_candidates: &[FrameworkCandidate],
) -> Result<CloudFunctionsResolvedAppRoot> {
    if let Some(firebase_candidate) = firebase_candidates
        .iter()
        .find(|candidate| candidate.resolved.matches_start_inside_codebase())
    {
        return Ok(firebase_candidate.resolved.clone());
    }

    match (firebase_candidates.first(), framework_candidates.first()) {
        (Some(firebase_candidate), Some(framework_candidate))
            if firebase_candidate.resolved.app_dir == framework_candidate.resolved.app_dir =>
        {
            Ok(firebase_candidate.resolved.clone())
        }
        (Some(firebase_candidate), Some(framework_candidate)) => Err(Error::InvalidInput(format!(
            "auto-discovery is ambiguous from `{}`: found Firebase project root `{}` and standalone Functions Framework package `{}`; use `--app-dir` to choose one",
            start.display(),
            firebase_candidate.resolved.app_dir.display(),
            framework_candidate.resolved.app_dir.display(),
        ))),
        (Some(firebase_candidate), None) => Ok(firebase_candidate.resolved.clone()),
        (None, Some(framework_candidate)) => Ok(framework_candidate.resolved.clone()),
        (None, None) => Err(Error::InvalidInput(format!(
            "could not auto-discover a compatible Firebase project root or standalone Functions Framework package from `{}` or its parents",
            start.display()
        ))),
    }
}

fn discover_firebase_candidates(start: &Path) -> Result<Vec<FirebaseCandidate>> {
    let mut candidates = Vec::new();
    for candidate in start.ancestors() {
        if let Some(resolved) = load_firebase_candidate(candidate, start)? {
            candidates.push(FirebaseCandidate { resolved });
        }
    }
    Ok(candidates)
}

fn discover_framework_candidates(start: &Path) -> Result<Vec<FrameworkCandidate>> {
    let mut candidates = Vec::new();
    for candidate in start.ancestors() {
        if let Some(resolved) = load_framework_candidate(candidate)? {
            candidates.push(FrameworkCandidate { resolved });
        }
    }
    Ok(candidates)
}

fn load_firebase_candidate(
    candidate: &Path,
    start: &Path,
) -> Result<Option<CloudFunctionsResolvedAppRoot>> {
    let firebase_json = candidate.join("firebase.json");
    if !firebase_json.is_file() {
        return Ok(None);
    }

    let firebase_json = canonicalize_file(&firebase_json)?;
    let raw = read_json::<RawFirebaseJson>(&firebase_json)?;
    let mut codebases = normalize_firebase_codebases(candidate, raw.functions)?;
    if codebases.is_empty() {
        return Err(Error::InvalidInput(format!(
            "firebase project `{}` does not declare any compatible Functions codebases",
            candidate.display()
        )));
    }

    codebases.sort_by(|left, right| left.source_dir.cmp(&right.source_dir));
    let matched_codebase = codebases
        .iter()
        .filter(|codebase| start.starts_with(&codebase.source_dir))
        .max_by_key(|codebase| codebase.source_dir.components().count())
        .map(|codebase| codebase.name.clone());

    Ok(Some(CloudFunctionsResolvedAppRoot::new(
        candidate.to_path_buf(),
        CloudFunctionsAppLayout::FirebaseProject(CloudFunctionsFirebaseProjectLayout {
            firebase_json,
            codebases,
        }),
        matched_codebase,
    )))
}

fn load_framework_candidate(candidate: &Path) -> Result<Option<CloudFunctionsResolvedAppRoot>> {
    let package_json = candidate.join("package.json");
    if !package_json.is_file() {
        return Ok(None);
    }

    let package_json = canonicalize_file(&package_json)?;
    let package = read_json::<RawPackageJson>(&package_json)?;
    if !package.depends_on(FUNCTIONS_FRAMEWORK_PACKAGE) {
        return Ok(None);
    }

    let entrypoint = resolve_framework_entrypoint(candidate, package.main.as_deref())?;
    Ok(Some(CloudFunctionsResolvedAppRoot::new(
        candidate.to_path_buf(),
        CloudFunctionsAppLayout::StandaloneFrameworkPackage(CloudFunctionsFrameworkPackageLayout {
            package_json,
            entrypoint,
        }),
        None,
    )))
}

fn normalize_firebase_codebases(
    app_dir: &Path,
    functions: Option<RawFirebaseFunctionsConfig>,
) -> Result<Vec<CloudFunctionsFirebaseCodebase>> {
    let descriptors = match functions {
        None => vec![RawFirebaseFunctionsDescriptor {
            source: Some(DEFAULT_FIREBASE_FUNCTIONS_SOURCE.to_string()),
            codebase: Some(DEFAULT_FIREBASE_CODEBASE.to_string()),
        }],
        Some(RawFirebaseFunctionsConfig::Source(source)) => vec![RawFirebaseFunctionsDescriptor {
            source: Some(source),
            codebase: Some(DEFAULT_FIREBASE_CODEBASE.to_string()),
        }],
        Some(RawFirebaseFunctionsConfig::Object(descriptor)) => vec![descriptor],
        Some(RawFirebaseFunctionsConfig::Array(descriptors)) => descriptors,
    };

    let mut seen = BTreeSet::new();
    let mut codebases = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
        let source = descriptor
            .source
            .unwrap_or_else(|| DEFAULT_FIREBASE_FUNCTIONS_SOURCE.to_string())
            .trim()
            .to_string();
        if source.is_empty() {
            return Err(Error::InvalidInput(
                "firebase.json functions source must not be empty".to_string(),
            ));
        }

        let codebase = descriptor
            .codebase
            .unwrap_or_else(|| DEFAULT_FIREBASE_CODEBASE.to_string())
            .trim()
            .to_string();
        if codebase.is_empty() {
            return Err(Error::InvalidInput(
                "firebase.json codebase name must not be empty".to_string(),
            ));
        }
        if !seen.insert(codebase.clone()) {
            return Err(Error::InvalidInput(format!(
                "firebase.json reuses Functions codebase `{}`",
                codebase
            )));
        }

        let source_dir = canonicalize_dir(&app_dir.join(source))?;
        if !source_dir.starts_with(app_dir) {
            return Err(Error::InvalidInput(format!(
                "firebase.json codebase `{}` points outside the Firebase project root",
                codebase
            )));
        }

        codebases.push(CloudFunctionsFirebaseCodebase {
            name: codebase,
            source_dir,
        });
    }

    Ok(codebases)
}

fn resolve_framework_entrypoint(package_dir: &Path, package_main: Option<&str>) -> Result<PathBuf> {
    if let Some(package_main) = package_main {
        if package_main.trim().is_empty() {
            return Err(Error::InvalidInput(format!(
                "package.json in `{}` cannot use an empty `main` entrypoint",
                package_dir.display()
            )));
        }

        let entrypoint = canonicalize_file(&package_dir.join(package_main))?;
        validate_framework_entrypoint(package_dir, &entrypoint)?;
        return Ok(entrypoint);
    }

    for candidate in DEFAULT_FRAMEWORK_ENTRYPOINTS {
        let entrypoint = package_dir.join(candidate);
        if entrypoint.is_file() {
            let entrypoint = canonicalize_file(&entrypoint)?;
            validate_framework_entrypoint(package_dir, &entrypoint)?;
            return Ok(entrypoint);
        }
    }

    Err(Error::InvalidInput(format!(
        "standalone Functions Framework package `{}` needs `package.json.main` or one of the default entrypoints: {}",
        package_dir.display(),
        DEFAULT_FRAMEWORK_ENTRYPOINTS.join(", ")
    )))
}

fn validate_framework_entrypoint(package_dir: &Path, entrypoint: &Path) -> Result<()> {
    if !entrypoint.starts_with(package_dir) {
        return Err(Error::InvalidInput(format!(
            "standalone Functions Framework package `{}` cannot use an entrypoint outside the package root",
            package_dir.display()
        )));
    }
    Ok(())
}

fn resolve_unchecked_path(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn canonicalize_dir(path: &Path) -> Result<PathBuf> {
    let metadata = fs::metadata(path).map_err(|error| {
        Error::InvalidInput(format!(
            "cloud functions app path `{}` is not readable: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_dir() {
        return Err(Error::InvalidInput(format!(
            "cloud functions app path `{}` is not a directory",
            path.display()
        )));
    }
    path.canonicalize().map_err(|error| {
        Error::InvalidInput(format!(
            "failed to resolve cloud functions app path `{}`: {error}",
            path.display()
        ))
    })
}

fn canonicalize_file(path: &Path) -> Result<PathBuf> {
    let metadata = fs::metadata(path).map_err(|error| {
        Error::InvalidInput(format!(
            "cloud functions contract file `{}` is not readable: {error}",
            path.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(Error::InvalidInput(format!(
            "cloud functions contract file `{}` is not a file",
            path.display()
        )));
    }
    path.canonicalize().map_err(|error| {
        Error::InvalidInput(format!(
            "failed to resolve cloud functions contract file `{}`: {error}",
            path.display()
        ))
    })
}

fn read_json<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let contents = fs::read_to_string(path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read cloud functions contract file `{}`: {error}",
            path.display()
        ))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse JSON from `{}`: {error}",
            path.display()
        ))
    })
}

#[derive(Debug, Deserialize)]
struct RawFirebaseJson {
    #[serde(default)]
    functions: Option<RawFirebaseFunctionsConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawFirebaseFunctionsConfig {
    Source(String),
    Object(RawFirebaseFunctionsDescriptor),
    Array(Vec<RawFirebaseFunctionsDescriptor>),
}

#[derive(Debug, Deserialize)]
struct RawFirebaseFunctionsDescriptor {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    codebase: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawPackageJson {
    #[serde(default)]
    main: Option<String>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "optionalDependencies")]
    optional_dependencies: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "peerDependencies")]
    peer_dependencies: std::collections::BTreeMap<String, serde_json::Value>,
}

impl RawPackageJson {
    fn depends_on(&self, package_name: &str) -> bool {
        self.dependencies.contains_key(package_name)
            || self.dev_dependencies.contains_key(package_name)
            || self.optional_dependencies.contains_key(package_name)
            || self.peer_dependencies.contains_key(package_name)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CloudFunctionsAdminImport, CloudFunctionsAppLayout, covered_admin_app_methods,
        covered_admin_firestore_methods, resolve_cloud_functions_app_root,
        validate_admin_method_support,
    };
    use tempfile::tempdir;

    #[test]
    fn cloud_functions_auto_discovers_firebase_root_from_nested_child() {
        let temp = tempdir().expect("tempdir should build");
        let repo = temp.path().join("repo");
        let functions = repo.join("functions");
        std::fs::create_dir_all(functions.join("src")).expect("functions source dir should build");
        std::fs::write(
            repo.join("firebase.json"),
            "{\n  \"functions\": { \"source\": \"functions\" }\n}",
        )
        .expect("firebase.json should write");

        let resolved = resolve_cloud_functions_app_root(None, &functions.join("src"))
            .expect("firebase root should resolve");

        assert_eq!(
            resolved.app_dir,
            repo.canonicalize().expect("repo should canonicalize")
        );
        assert_eq!(
            resolved.artifact_dir,
            repo.canonicalize()
                .expect("repo should canonicalize")
                .join(".neovex")
                .join("firebase")
        );
        assert_eq!(resolved.matched_codebase.as_deref(), Some("default"));
        match resolved.layout {
            CloudFunctionsAppLayout::FirebaseProject(project) => {
                assert_eq!(project.codebases.len(), 1);
                assert_eq!(project.codebases[0].name, "default");
                assert_eq!(
                    project.codebases[0].source_dir,
                    functions
                        .canonicalize()
                        .expect("functions dir should canonicalize")
                );
            }
            other => panic!("expected firebase project layout, got {other:?}"),
        }
    }

    #[test]
    fn cloud_functions_firebase_discovery_defaults_to_functions_dir() {
        let temp = tempdir().expect("tempdir should build");
        let repo = temp.path().join("firebase-app");
        let functions = repo.join("functions");
        std::fs::create_dir_all(&functions).expect("default functions dir should build");
        std::fs::write(repo.join("firebase.json"), "{}").expect("firebase.json should write");

        let resolved =
            resolve_cloud_functions_app_root(None, &repo).expect("firebase root should resolve");

        match resolved.layout {
            CloudFunctionsAppLayout::FirebaseProject(project) => {
                assert_eq!(project.codebases[0].name, "default");
                assert_eq!(
                    project.codebases[0].source_dir,
                    functions
                        .canonicalize()
                        .expect("functions dir should canonicalize")
                );
            }
            other => panic!("expected firebase project layout, got {other:?}"),
        }
    }

    #[test]
    fn cloud_functions_firebase_discovery_preserves_multi_codebase_mapping() {
        let temp = tempdir().expect("tempdir should build");
        let repo = temp.path().join("monorepo");
        let primary = repo.join("packages").join("primary-functions");
        let admin = repo.join("packages").join("admin-functions");
        std::fs::create_dir_all(primary.join("src")).expect("primary source should build");
        std::fs::create_dir_all(&admin).expect("admin source should build");
        std::fs::write(
            repo.join("firebase.json"),
            r#"{
  "functions": [
    { "source": "packages/primary-functions", "codebase": "primary" },
    { "source": "packages/admin-functions", "codebase": "admin" }
  ]
}"#,
        )
        .expect("firebase.json should write");

        let resolved = resolve_cloud_functions_app_root(None, &primary.join("src"))
            .expect("firebase monorepo root should resolve");

        assert_eq!(resolved.matched_codebase.as_deref(), Some("primary"));
        match resolved.layout {
            CloudFunctionsAppLayout::FirebaseProject(project) => {
                assert_eq!(
                    project
                        .codebases
                        .iter()
                        .map(|codebase| codebase.name.as_str())
                        .collect::<Vec<_>>(),
                    vec!["admin", "primary"]
                );
            }
            other => panic!("expected firebase project layout, got {other:?}"),
        }
    }

    #[test]
    fn cloud_functions_auto_discovers_standalone_framework_package() {
        let temp = tempdir().expect("tempdir should build");
        let package = temp.path().join("framework-app");
        std::fs::create_dir_all(package.join("src")).expect("package source should build");
        std::fs::write(
            package.join("package.json"),
            r#"{
  "name": "framework-app",
  "dependencies": {
    "@google-cloud/functions-framework": "^3.4.5"
  }
}"#,
        )
        .expect("package.json should write");
        std::fs::write(package.join("index.mjs"), "export const target = 1;\n")
            .expect("entrypoint should write");

        let resolved = resolve_cloud_functions_app_root(None, &package.join("src"))
            .expect("standalone package should resolve");

        match resolved.layout {
            CloudFunctionsAppLayout::StandaloneFrameworkPackage(layout) => {
                assert_eq!(
                    layout.entrypoint,
                    package
                        .join("index.mjs")
                        .canonicalize()
                        .expect("entrypoint should canonicalize")
                );
            }
            other => panic!("expected standalone package layout, got {other:?}"),
        }
    }

    #[test]
    fn cloud_functions_auto_discovery_reports_ambiguous_nested_framework_package() {
        let temp = tempdir().expect("tempdir should build");
        let repo = temp.path().join("firebase-app");
        let functions = repo.join("functions");
        let standalone = repo.join("tools").join("framework-app");
        std::fs::create_dir_all(&functions).expect("functions dir should build");
        std::fs::create_dir_all(standalone.join("lib")).expect("standalone dir should build");
        std::fs::write(
            repo.join("firebase.json"),
            "{\n  \"functions\": { \"source\": \"functions\" }\n}",
        )
        .expect("firebase.json should write");
        std::fs::write(
            standalone.join("package.json"),
            r#"{
  "name": "framework-app",
  "dependencies": {
    "@google-cloud/functions-framework": "^3.4.5"
  },
  "main": "server.js"
}"#,
        )
        .expect("package.json should write");
        std::fs::write(standalone.join("server.js"), "export const target = 1;\n")
            .expect("entrypoint should write");

        let error = resolve_cloud_functions_app_root(None, &standalone.join("lib"))
            .expect_err("ambiguous auto discovery should fail");
        let message = error.to_string();
        assert!(message.contains("ambiguous"));
        assert!(message.contains("--app-dir"));
    }

    #[test]
    fn cloud_functions_explicit_app_dir_can_choose_nested_framework_package() {
        let temp = tempdir().expect("tempdir should build");
        let repo = temp.path().join("firebase-app");
        let functions = repo.join("functions");
        let standalone = repo.join("tools").join("framework-app");
        std::fs::create_dir_all(&functions).expect("functions dir should build");
        std::fs::create_dir_all(&standalone).expect("standalone dir should build");
        std::fs::write(
            repo.join("firebase.json"),
            "{\n  \"functions\": { \"source\": \"functions\" }\n}",
        )
        .expect("firebase.json should write");
        std::fs::write(
            standalone.join("package.json"),
            r#"{
  "name": "framework-app",
  "dependencies": {
    "@google-cloud/functions-framework": "^3.4.5"
  },
  "main": "server.js"
}"#,
        )
        .expect("package.json should write");
        std::fs::write(standalone.join("server.js"), "export const target = 1;\n")
            .expect("entrypoint should write");

        let resolved = resolve_cloud_functions_app_root(Some(&standalone), temp.path())
            .expect("explicit app dir should choose standalone package");

        assert_eq!(
            resolved.app_dir,
            standalone
                .canonicalize()
                .expect("standalone should canonicalize")
        );
        assert!(matches!(
            resolved.layout,
            CloudFunctionsAppLayout::StandaloneFrameworkPackage(_)
        ));
    }

    #[test]
    fn cloud_functions_admin_support_matrix_matches_first_slice_contract() {
        assert_eq!(
            covered_admin_app_methods(),
            vec![
                "initializeApp".to_string(),
                "getApp".to_string(),
                "getApps".to_string(),
                "deleteApp".to_string(),
            ]
        );
        assert_eq!(
            covered_admin_firestore_methods(),
            vec!["getFirestore".to_string()]
        );

        assert_eq!(
            validate_admin_method_support("firebase-admin/app", "initializeApp")
                .expect("initializeApp should be covered"),
            CloudFunctionsAdminImport::App
        );
        assert_eq!(
            validate_admin_method_support("firebase-admin/firestore", "getFirestore")
                .expect("getFirestore should be covered"),
            CloudFunctionsAdminImport::Firestore
        );
    }

    #[test]
    fn cloud_functions_admin_support_rejects_unsupported_methods() {
        let error = validate_admin_method_support("firebase-admin/firestore", "collection")
            .expect_err("unsupported firestore admin method should fail");
        assert!(error.to_string().contains("getFirestore"));

        let error = validate_admin_method_support("firebase-admin/auth", "getAuth")
            .expect_err("unsupported admin import should fail");
        assert!(error.to_string().contains("firebase-admin/app"));
        assert!(error.to_string().contains("firebase-admin/firestore"));
    }
}
