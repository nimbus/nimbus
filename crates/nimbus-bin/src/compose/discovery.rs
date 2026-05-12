use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use super::file::DEFAULT_COMPOSE_FILE;

const DEFAULT_COMPOSE_OVERRIDE_FILE: &str = "compose.override.yaml";
const MODERN_FALLBACK_COMPOSE_FILE: &str = "compose.yml";
const LEGACY_COMPOSE_FILE_YAML: &str = "docker-compose.yaml";
const LEGACY_COMPOSE_FILE_YML: &str = "docker-compose.yml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ComposeSelectionOrigin {
    ExplicitFlag,
    ExplicitEnvironment,
    AutoDiscovered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedComposeSelection {
    pub(crate) origin: ComposeSelectionOrigin,
    pub(crate) project_root: PathBuf,
    pub(crate) files: Vec<PathBuf>,
    pub(crate) display_files: Vec<PathBuf>,
}

impl ResolvedComposeSelection {
    pub(crate) fn explicit(path: PathBuf) -> Self {
        let project_root = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            origin: ComposeSelectionOrigin::ExplicitFlag,
            project_root,
            files: vec![path.clone()],
            display_files: vec![path],
        }
    }

    fn auto_discovered(project_root: PathBuf, files: Vec<PathBuf>) -> Self {
        Self {
            origin: ComposeSelectionOrigin::AutoDiscovered,
            project_root,
            display_files: files.clone(),
            files,
        }
    }

    pub(crate) fn primary_file(&self) -> &Path {
        self.files
            .first()
            .map(PathBuf::as_path)
            .expect("resolved compose selection should include a primary file")
    }

    pub(crate) fn display_primary_file(&self) -> &Path {
        self.display_files
            .first()
            .map(PathBuf::as_path)
            .expect("resolved compose selection should include a display primary file")
    }

    pub(crate) fn includes_default_override(&self) -> bool {
        self.origin == ComposeSelectionOrigin::AutoDiscovered
            && self.files.len() > 1
            && self.files.iter().skip(1).any(|path| {
                path.file_name()
                    .is_some_and(|name| name == DEFAULT_COMPOSE_OVERRIDE_FILE)
            })
    }

    fn from_explicit_files(
        requested_files: &[PathBuf],
        cwd: &Path,
        origin: ComposeSelectionOrigin,
    ) -> Result<Self, ComposeDiscoveryError> {
        if requested_files.is_empty() {
            return Err(ComposeDiscoveryError::new(
                "explicit compose selection did not include any files",
            ));
        }
        let files = requested_files
            .iter()
            .map(|path| resolve_path_from_cwd(path, cwd))
            .collect::<Vec<_>>();
        let project_root = files
            .first()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| cwd.to_path_buf());
        Ok(Self {
            origin,
            project_root,
            files,
            display_files: requested_files.to_vec(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComposeDiscoveryError {
    message: String,
}

impl ComposeDiscoveryError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ComposeDiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ComposeDiscoveryError {}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ComposeDiscoveryEnvironment {
    pub(crate) compose_file: Option<String>,
    pub(crate) compose_path_separator: Option<String>,
}

impl ComposeDiscoveryEnvironment {
    pub(crate) fn current() -> Self {
        Self {
            compose_file: env::var("COMPOSE_FILE").ok(),
            compose_path_separator: env::var("COMPOSE_PATH_SEPARATOR").ok(),
        }
    }
}

pub(crate) fn compose_selection_summary(selection: &ResolvedComposeSelection) -> String {
    match selection.origin {
        ComposeSelectionOrigin::ExplicitFlag => {
            explicit_selection_summary(selection.display_primary_file(), selection.files.len())
        }
        ComposeSelectionOrigin::ExplicitEnvironment => {
            environment_selection_summary(selection.display_primary_file(), selection.files.len())
        }
        ComposeSelectionOrigin::AutoDiscovered if selection.includes_default_override() => format!(
            "auto-discovered {} (+ compose.override.yaml)",
            selection.primary_file().display()
        ),
        ComposeSelectionOrigin::AutoDiscovered if selection.files.len() > 1 => format!(
            "auto-discovered {} (+ {} extra Compose files)",
            selection.primary_file().display(),
            selection.files.len() - 1
        ),
        ComposeSelectionOrigin::AutoDiscovered => {
            format!("auto-discovered {}", selection.primary_file().display())
        }
    }
}

pub(crate) fn resolve_compose_selection(
    explicit_files: &[PathBuf],
    cwd: &Path,
) -> Result<Option<ResolvedComposeSelection>, ComposeDiscoveryError> {
    resolve_compose_selection_with_environment(
        explicit_files,
        cwd,
        &ComposeDiscoveryEnvironment::current(),
    )
}

pub(crate) fn resolve_compose_selection_with_environment(
    explicit_files: &[PathBuf],
    cwd: &Path,
    environment: &ComposeDiscoveryEnvironment,
) -> Result<Option<ResolvedComposeSelection>, ComposeDiscoveryError> {
    if !explicit_files.is_empty() {
        return Ok(Some(ResolvedComposeSelection::from_explicit_files(
            explicit_files,
            cwd,
            ComposeSelectionOrigin::ExplicitFlag,
        )?));
    }

    if let Some(environment_files) = compose_files_from_environment(environment)? {
        return Ok(Some(ResolvedComposeSelection::from_explicit_files(
            &environment_files,
            cwd,
            ComposeSelectionOrigin::ExplicitEnvironment,
        )?));
    }

    resolve_auto_discovered_compose_selection(cwd)
}

fn resolve_auto_discovered_compose_selection(
    cwd: &Path,
) -> Result<Option<ResolvedComposeSelection>, ComposeDiscoveryError> {
    for directory in cwd.ancestors() {
        let Some(primary_file) = discover_primary_compose_file(directory)? else {
            continue;
        };
        let mut files = vec![primary_file.clone()];
        if primary_file
            .file_name()
            .is_some_and(|name| name == DEFAULT_COMPOSE_FILE)
        {
            let override_file = directory.join(DEFAULT_COMPOSE_OVERRIDE_FILE);
            if is_file(&override_file) {
                files.push(override_file);
            }
        }
        return Ok(Some(ResolvedComposeSelection::auto_discovered(
            directory.to_path_buf(),
            files,
        )));
    }

    Ok(None)
}

fn discover_primary_compose_file(
    directory: &Path,
) -> Result<Option<PathBuf>, ComposeDiscoveryError> {
    let canonical = directory.join(DEFAULT_COMPOSE_FILE);
    if is_file(&canonical) {
        return Ok(Some(canonical));
    }

    let modern_fallback = directory.join(MODERN_FALLBACK_COMPOSE_FILE);
    if is_file(&modern_fallback) {
        return Ok(Some(modern_fallback));
    }

    let legacy_yaml = directory.join(LEGACY_COMPOSE_FILE_YAML);
    let legacy_yml = directory.join(LEGACY_COMPOSE_FILE_YML);
    let legacy_yaml_exists = is_file(&legacy_yaml);
    let legacy_yml_exists = is_file(&legacy_yml);

    match (legacy_yaml_exists, legacy_yml_exists) {
        (true, true) => Err(ComposeDiscoveryError::new(format!(
            "multiple Compose files found in {}: {}, {}. Remove one or pass an explicit compose path with --compose-file or --file.",
            directory.display(),
            LEGACY_COMPOSE_FILE_YAML,
            LEGACY_COMPOSE_FILE_YML
        ))),
        (true, false) => Ok(Some(legacy_yaml)),
        (false, true) => Ok(Some(legacy_yml)),
        (false, false) => Ok(None),
    }
}

fn resolve_path_from_cwd(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn is_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn explicit_selection_summary(primary_file: &Path, file_count: usize) -> String {
    if file_count > 1 {
        format!(
            "{} (+ {} extra Compose files)",
            primary_file.display(),
            file_count - 1
        )
    } else {
        primary_file.display().to_string()
    }
}

fn environment_selection_summary(primary_file: &Path, file_count: usize) -> String {
    if file_count > 1 {
        format!(
            "COMPOSE_FILE={} (+ {} extra Compose files)",
            primary_file.display(),
            file_count - 1
        )
    } else {
        format!("COMPOSE_FILE={}", primary_file.display())
    }
}

fn compose_files_from_environment(
    environment: &ComposeDiscoveryEnvironment,
) -> Result<Option<Vec<PathBuf>>, ComposeDiscoveryError> {
    let Some(compose_file) = environment.compose_file.as_ref() else {
        return Ok(None);
    };
    let separator = compose_path_separator(environment)?;
    let files = compose_file
        .split(&separator)
        .filter(|segment| !segment.trim().is_empty())
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if files.is_empty() {
        return Err(ComposeDiscoveryError::new(
            "COMPOSE_FILE is set but does not contain any compose paths. Unset COMPOSE_FILE, or provide one or more paths.",
        ));
    }
    Ok(Some(files))
}

fn compose_path_separator(
    environment: &ComposeDiscoveryEnvironment,
) -> Result<String, ComposeDiscoveryError> {
    if let Some(separator) = environment.compose_path_separator.as_ref() {
        if separator.is_empty() {
            return Err(ComposeDiscoveryError::new(
                "COMPOSE_PATH_SEPARATOR must not be empty.",
            ));
        }
        return Ok(separator.clone());
    }
    if cfg!(windows) {
        Ok(";".to_owned())
    } else {
        Ok(":".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should build");
        }
        fs::write(
            path,
            "name: demo\nservices:\n  api:\n    image: busybox:latest\n",
        )
        .expect("fixture should write");
    }

    #[test]
    fn explicit_selection_resolves_relative_path_from_cwd() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let cwd = tempdir.path().join("apps").join("chat");
        fs::create_dir_all(&cwd).expect("cwd should build");

        let selection =
            resolve_compose_selection(&[PathBuf::from("./infra/dev-compose.yaml")], &cwd)
                .expect("explicit selection should resolve")
                .expect("selection should exist");

        assert_eq!(selection.origin, ComposeSelectionOrigin::ExplicitFlag);
        assert_eq!(selection.files, vec![cwd.join("./infra/dev-compose.yaml")]);
        assert_eq!(
            selection.display_files,
            vec![PathBuf::from("./infra/dev-compose.yaml")]
        );
        assert_eq!(selection.project_root, cwd.join("./infra"));
        assert!(!selection.includes_default_override());
    }

    #[test]
    fn explicit_selection_preserves_ordered_file_lists() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let cwd = tempdir.path().join("apps").join("chat");
        fs::create_dir_all(&cwd).expect("cwd should build");

        let selection = resolve_compose_selection(
            &[
                PathBuf::from("./compose.yaml"),
                PathBuf::from("./compose.local.yaml"),
            ],
            &cwd,
        )
        .expect("explicit selection should resolve")
        .expect("selection should exist");

        assert_eq!(
            selection.files,
            vec![cwd.join("./compose.yaml"), cwd.join("./compose.local.yaml")]
        );
        assert_eq!(
            selection.display_files,
            vec![
                PathBuf::from("./compose.yaml"),
                PathBuf::from("./compose.local.yaml")
            ]
        );
        assert_eq!(selection.project_root, cwd);
    }

    #[test]
    fn compose_file_environment_selects_ordered_file_lists() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let cwd = tempdir.path().join("workspace");
        fs::create_dir_all(&cwd).expect("cwd should build");

        let selection = resolve_compose_selection_with_environment(
            &[],
            &cwd,
            &ComposeDiscoveryEnvironment {
                compose_file: Some("./compose.yaml:./compose.prod.yaml".to_owned()),
                compose_path_separator: None,
            },
        )
        .expect("environment selection should resolve")
        .expect("selection should exist");

        assert_eq!(
            selection.origin,
            ComposeSelectionOrigin::ExplicitEnvironment
        );
        assert_eq!(
            selection.files,
            vec![cwd.join("./compose.yaml"), cwd.join("./compose.prod.yaml")]
        );
        assert_eq!(
            selection.display_files,
            vec![
                PathBuf::from("./compose.yaml"),
                PathBuf::from("./compose.prod.yaml")
            ]
        );
        assert_eq!(selection.project_root, cwd);
    }

    #[test]
    fn explicit_flags_override_compose_file_environment() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let cwd = tempdir.path().join("workspace");
        fs::create_dir_all(&cwd).expect("cwd should build");

        let selection = resolve_compose_selection_with_environment(
            &[PathBuf::from("./compose.dev.yaml")],
            &cwd,
            &ComposeDiscoveryEnvironment {
                compose_file: Some("./compose.yaml:./compose.prod.yaml".to_owned()),
                compose_path_separator: None,
            },
        )
        .expect("explicit flags should resolve")
        .expect("selection should exist");

        assert_eq!(selection.origin, ComposeSelectionOrigin::ExplicitFlag);
        assert_eq!(selection.files, vec![cwd.join("./compose.dev.yaml")]);
    }

    #[test]
    fn compose_file_environment_honors_custom_separator() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let cwd = tempdir.path().join("workspace");
        fs::create_dir_all(&cwd).expect("cwd should build");

        let selection = resolve_compose_selection_with_environment(
            &[],
            &cwd,
            &ComposeDiscoveryEnvironment {
                compose_file: Some("./compose.yaml|./compose.prod.yaml".to_owned()),
                compose_path_separator: Some("|".to_owned()),
            },
        )
        .expect("environment selection should resolve")
        .expect("selection should exist");

        assert_eq!(
            selection.files,
            vec![cwd.join("./compose.yaml"), cwd.join("./compose.prod.yaml")]
        );
    }

    #[test]
    fn empty_compose_path_separator_is_rejected() {
        let error = resolve_compose_selection_with_environment(
            &[],
            Path::new("/workspace"),
            &ComposeDiscoveryEnvironment {
                compose_file: Some("./compose.yaml".to_owned()),
                compose_path_separator: Some(String::new()),
            },
        )
        .expect_err("empty separator should fail");

        assert_eq!(
            error.to_string(),
            "COMPOSE_PATH_SEPARATOR must not be empty."
        );
    }

    #[test]
    fn empty_compose_file_environment_is_rejected() {
        let error = resolve_compose_selection_with_environment(
            &[],
            Path::new("/workspace"),
            &ComposeDiscoveryEnvironment {
                compose_file: Some("::".to_owned()),
                compose_path_separator: None,
            },
        )
        .expect_err("empty compose file list should fail");

        assert!(
            error
                .to_string()
                .contains("COMPOSE_FILE is set but does not contain any compose paths")
        );
    }

    #[test]
    fn auto_discovery_prefers_compose_yaml_and_override() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let root = tempdir.path();
        write_file(&root.join(DEFAULT_COMPOSE_FILE));
        write_file(&root.join(DEFAULT_COMPOSE_OVERRIDE_FILE));
        write_file(&root.join(MODERN_FALLBACK_COMPOSE_FILE));

        let selection = resolve_compose_selection(&[], root)
            .expect("auto discovery should succeed")
            .expect("selection should exist");

        assert_eq!(selection.origin, ComposeSelectionOrigin::AutoDiscovered);
        assert_eq!(
            selection.files,
            vec![
                root.join(DEFAULT_COMPOSE_FILE),
                root.join(DEFAULT_COMPOSE_OVERRIDE_FILE)
            ]
        );
        assert_eq!(selection.project_root, root);
        assert_eq!(selection.primary_file(), root.join(DEFAULT_COMPOSE_FILE));
        assert!(selection.includes_default_override());
    }

    #[test]
    fn auto_discovery_prefers_modern_fallback_over_legacy_filenames() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let root = tempdir.path();
        write_file(&root.join(MODERN_FALLBACK_COMPOSE_FILE));
        write_file(&root.join(LEGACY_COMPOSE_FILE_YAML));
        write_file(&root.join(LEGACY_COMPOSE_FILE_YML));

        let selection = resolve_compose_selection(&[], root)
            .expect("modern discovery should succeed")
            .expect("selection should exist");

        assert_eq!(
            selection.files,
            vec![root.join(MODERN_FALLBACK_COMPOSE_FILE)]
        );
    }

    #[test]
    fn auto_discovery_walks_parent_directories() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let root = tempdir.path();
        let nested = root.join("packages").join("web").join("src");
        fs::create_dir_all(&nested).expect("nested path should build");
        write_file(&root.join(DEFAULT_COMPOSE_FILE));

        let selection = resolve_compose_selection(&[], &nested)
            .expect("parent discovery should succeed")
            .expect("selection should exist");

        assert_eq!(selection.project_root, root);
        assert_eq!(selection.files, vec![root.join(DEFAULT_COMPOSE_FILE)]);
    }

    #[test]
    fn auto_discovery_errors_on_ambiguous_legacy_candidates() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let root = tempdir.path();
        write_file(&root.join(LEGACY_COMPOSE_FILE_YAML));
        write_file(&root.join(LEGACY_COMPOSE_FILE_YML));

        let error = resolve_compose_selection(&[], root)
            .expect_err("ambiguous legacy candidates should fail");

        assert!(error.to_string().contains("multiple Compose files found"));
        assert!(error.to_string().contains("--compose-file"));
        assert!(error.to_string().contains("--file"));
    }

    #[test]
    fn auto_discovery_returns_none_when_no_compose_file_exists() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");

        let selection =
            resolve_compose_selection(&[], tempdir.path()).expect("missing files should not error");

        assert_eq!(selection, None);
    }

    #[test]
    fn compose_selection_summary_reports_origin_and_override_loading() {
        let explicit = ResolvedComposeSelection::explicit(PathBuf::from("./compose.custom.yaml"));
        assert_eq!(
            compose_selection_summary(&explicit),
            "./compose.custom.yaml"
        );

        let explicit_multi = ResolvedComposeSelection::from_explicit_files(
            &[
                PathBuf::from("./compose.yaml"),
                PathBuf::from("./compose.local.yaml"),
            ],
            Path::new("/workspace"),
            ComposeSelectionOrigin::ExplicitFlag,
        )
        .expect("explicit multi-file summary should build");
        assert_eq!(
            compose_selection_summary(&explicit_multi),
            "./compose.yaml (+ 1 extra Compose files)"
        );

        let environment_multi = ResolvedComposeSelection::from_explicit_files(
            &[
                PathBuf::from("./compose.yaml"),
                PathBuf::from("./compose.prod.yaml"),
            ],
            Path::new("/workspace"),
            ComposeSelectionOrigin::ExplicitEnvironment,
        )
        .expect("environment summary should build");
        assert_eq!(
            compose_selection_summary(&environment_multi),
            "COMPOSE_FILE=./compose.yaml (+ 1 extra Compose files)"
        );

        let auto_single = ResolvedComposeSelection::auto_discovered(
            PathBuf::from("/workspace"),
            vec![PathBuf::from("/workspace/compose.yml")],
        );
        assert_eq!(
            compose_selection_summary(&auto_single),
            "auto-discovered /workspace/compose.yml"
        );

        let auto_override = ResolvedComposeSelection::auto_discovered(
            PathBuf::from("/workspace"),
            vec![
                PathBuf::from("/workspace/compose.yaml"),
                PathBuf::from("/workspace/compose.override.yaml"),
            ],
        );
        assert_eq!(
            compose_selection_summary(&auto_override),
            "auto-discovered /workspace/compose.yaml (+ compose.override.yaml)"
        );
    }
}
