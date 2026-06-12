use std::io;
use std::path::{Path, PathBuf};

use crate::node;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DevAdapter {
    Convex { source_root: PathBuf },
    CloudFunctions { source_roots: Vec<PathBuf> },
}

impl DevAdapter {
    pub(super) fn adapter(&self) -> node::Adapter {
        match self {
            Self::Convex { .. } => node::Adapter::Convex,
            Self::CloudFunctions { .. } => node::Adapter::CloudFunctions,
        }
    }

    pub(super) fn name(&self) -> &'static str {
        self.adapter().name()
    }

    pub(super) fn source_roots(&self) -> &[PathBuf] {
        match self {
            Self::Convex { source_root } => std::slice::from_ref(source_root),
            Self::CloudFunctions { source_roots } => source_roots,
        }
    }

    pub(super) fn needs_node_dependencies(&self) -> bool {
        self.adapter().needs_node_dependencies()
    }

    pub(super) fn npm_install_dirs(&self, app_dir: &Path) -> Vec<PathBuf> {
        match self {
            Self::Convex { .. } => vec![app_dir.to_path_buf()],
            Self::CloudFunctions { source_roots } => source_roots.clone(),
        }
    }
}

pub(super) fn detect_dev_adapter(app_dir: &Path) -> io::Result<Option<DevAdapter>> {
    let nimbus_root = app_dir.join("nimbus");
    if nimbus_root.is_dir() {
        return Ok(Some(DevAdapter::Convex {
            source_root: nimbus_root,
        }));
    }

    let convex_root = app_dir.join("convex");
    if convex_root.is_dir() {
        return Ok(Some(DevAdapter::Convex {
            source_root: convex_root,
        }));
    }

    if let Some(adapter) = detect_cloud_functions_adapter(app_dir)? {
        return Ok(Some(adapter));
    }

    Ok(None)
}

fn detect_cloud_functions_adapter(app_dir: &Path) -> io::Result<Option<DevAdapter>> {
    if let Some(project) = node::firebase_functions_project(app_dir)? {
        return Ok(Some(DevAdapter::CloudFunctions {
            source_roots: project.source_dirs(),
        }));
    }

    if has_functions_framework_dependency(app_dir) {
        return Ok(Some(DevAdapter::CloudFunctions {
            source_roots: vec![app_dir.to_path_buf()],
        }));
    }

    Ok(None)
}

fn has_functions_framework_dependency(app_dir: &Path) -> bool {
    has_package_dependency(app_dir, "@google-cloud/functions-framework")
}

/// True when the app's `package.json` declares a `firebase` dependency.
///
/// This is a migration hint signal, not an adapter detection: a `firebase`
/// dependency alone does not prove the app should be rewired (it may target
/// production Google Firebase, and the drop-in package covers only
/// `firebase/app` + `firebase/firestore`), so `nimbus dev` only suggests the
/// migration commands instead of provisioning automatically.
pub(super) fn has_firebase_dependency(app_dir: &Path) -> bool {
    has_package_dependency(app_dir, "firebase")
}

fn has_package_dependency(app_dir: &Path, package_name: &str) -> bool {
    let package_json_path = app_dir.join("package.json");
    let Ok(content) = std::fs::read_to_string(&package_json_path) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    for key in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if parsed[key].get(package_name).is_some() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_package_json(dir: &Path, contents: &str) {
        std::fs::write(dir.join("package.json"), contents).expect("write package.json");
    }

    #[test]
    fn has_firebase_dependency_finds_dependencies_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"dependencies": {"firebase": "^11.0.0", "react": "^18.0.0"}}"#,
        );
        assert!(has_firebase_dependency(dir.path()));
    }

    #[test]
    fn has_firebase_dependency_finds_dev_dependencies_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(dir.path(), r#"{"devDependencies": {"firebase": "*"}}"#);
        assert!(has_firebase_dependency(dir.path()));
    }

    #[test]
    fn has_firebase_dependency_ignores_other_packages_and_scoped_lookalikes() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"dependencies": {"firebase-admin": "^12.0.0", "@firebase/app": "^0.10.0"}}"#,
        );
        assert!(!has_firebase_dependency(dir.path()));
    }

    #[test]
    fn has_firebase_dependency_false_without_package_json_or_on_malformed_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!has_firebase_dependency(dir.path()));
        write_package_json(dir.path(), "{not json");
        assert!(!has_firebase_dependency(dir.path()));
    }

    #[test]
    fn has_package_dependency_matches_functions_framework_in_peer_dependencies() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"peerDependencies": {"@google-cloud/functions-framework": "^3.0.0"}}"#,
        );
        assert!(has_functions_framework_dependency(dir.path()));
        assert!(!has_firebase_dependency(dir.path()));
    }
}
