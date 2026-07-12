use std::io;
use std::path::{Path, PathBuf};

use crate::node_runtime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DevAdapter {
    Convex {
        source_root: PathBuf,
    },
    CloudFunctions {
        source_roots: Vec<PathBuf>,
    },
    /// A Firestore client app: a `firebase` dependency without any
    /// higher-precedence marker. Client apps have no server-side authoring
    /// sources, so this variant carries no source roots — wiring is owned
    /// by the scan-gated Firebase path in `dev.rs`, never done here.
    FirestoreClient,
}

impl DevAdapter {
    pub(super) fn name(&self) -> &'static str {
        match self {
            Self::Convex { .. } => node_runtime::Adapter::Convex.name(),
            Self::CloudFunctions { .. } => node_runtime::Adapter::CloudFunctions.name(),
            Self::FirestoreClient => "firestore-client",
        }
    }

    pub(super) fn source_roots(&self) -> &[PathBuf] {
        match self {
            Self::Convex { source_root } => std::slice::from_ref(source_root),
            Self::CloudFunctions { source_roots } => source_roots,
            Self::FirestoreClient => &[],
        }
    }

    pub(super) fn needs_node_dependencies(&self) -> bool {
        match self {
            Self::Convex { .. } => node_runtime::Adapter::Convex.needs_node_dependencies(),
            Self::CloudFunctions { .. } => {
                node_runtime::Adapter::CloudFunctions.needs_node_dependencies()
            }
            // The scan-gated wiring rewires `firebase` to the provisioned
            // drop-in copy, which must then be installed.
            Self::FirestoreClient => true,
        }
    }

    /// The embedded-package target provisioned unconditionally before the
    /// install loop. FirestoreClient answers `None` here on purpose: its
    /// `firebase` drop-in provision mutates the app and therefore happens
    /// only behind the fail-closed import scan in `dev.rs`.
    pub(super) fn provision_target(&self) -> Option<&'static str> {
        match self {
            Self::Convex { .. } => node_runtime::Adapter::Convex.provision_target(),
            Self::CloudFunctions { .. } => node_runtime::Adapter::CloudFunctions.provision_target(),
            Self::FirestoreClient => None,
        }
    }

    pub(super) fn npm_install_dirs(&self, app_dir: &Path) -> Vec<PathBuf> {
        match self {
            Self::Convex { .. } | Self::FirestoreClient => vec![app_dir.to_path_buf()],
            Self::CloudFunctions { source_roots } => source_roots.clone(),
        }
    }
}

pub(super) fn detect_dev_adapter(app_dir: &Path) -> io::Result<Option<DevAdapter>> {
    // convex.json's "functions" setting relocates the source directory (e.g.
    // Create React App projects that cannot import from outside src/) and
    // takes precedence over the nimbus/convex directory heuristic below — an
    // explicit override is an unambiguous signal, so a declared-but-missing
    // directory is a real misconfiguration to surface, not something to
    // silently fall through past.
    if let Some(functions_override) = read_convex_json_functions_field(app_dir) {
        let functions_root = app_dir.join(&functions_override);
        if !functions_root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "convex.json declares \"functions\": {functions_override:?}, but {} is not a directory in {}. Create that directory with your Convex functions inside it, or remove \"functions\" from convex.json.",
                    functions_root.display(),
                    app_dir.display(),
                ),
            ));
        }
        return Ok(Some(DevAdapter::Convex {
            source_root: functions_root,
        }));
    }

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

    // Precedence 3: a `firebase` dependency without any higher-precedence
    // marker is a Firestore client app. This includes a `firebase.json`
    // declaring only firestore/hosting/emulators — such configs resolve to
    // no Functions project above.
    if has_firebase_dependency(app_dir) {
        return Ok(Some(DevAdapter::FirestoreClient));
    }

    Ok(None)
}

/// Best-effort read of convex.json's top-level "functions" string field.
/// Malformed JSON, a missing file, or a wrong-typed value all fall through
/// to `None` (the default nimbus/convex directory heuristic in the caller)
/// — codegen's own convex.json loader
/// (packages/codegen/src/project_config.mjs) is the strict validator that
/// runs right after detection; this detection-time read only needs the
/// override signal, not full validation.
fn read_convex_json_functions_field(app_dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(app_dir.join("convex.json")).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    parsed.get("functions")?.as_str().map(str::to_owned)
}

fn detect_cloud_functions_adapter(app_dir: &Path) -> io::Result<Option<DevAdapter>> {
    if let Some(project) = node_runtime::firebase_functions_project(app_dir)? {
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

/// True when the app's `package.json` declares a `firebase` dependency —
/// the FirestoreClient detection signal (precedence 3).
///
/// Detection alone never rewires the app: the dependency may target
/// production Google Firebase with imports the drop-in package cannot
/// serve, so wiring stays behind the fail-closed import-coverage scan
/// (`super::firebase_scan`).
pub(super) fn has_firebase_dependency(app_dir: &Path) -> bool {
    has_package_dependency(app_dir, "firebase")
}

pub(super) fn has_package_dependency(app_dir: &Path, package_name: &str) -> bool {
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

    #[test]
    fn firebase_dep_alone_detects_firestore_client() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(dir.path(), r#"{"dependencies": {"firebase": "^11.0.0"}}"#);
        assert_eq!(
            detect_dev_adapter(dir.path()).expect("detection should succeed"),
            Some(DevAdapter::FirestoreClient)
        );
    }

    #[test]
    fn firestore_only_firebase_json_with_firebase_dep_detects_firestore_client() {
        let dir = tempfile::tempdir().expect("tempdir");
        // The functions-config disambiguation, client direction: a
        // firebase.json declaring only firestore/hosting/emulators must not
        // resolve to Cloud Functions.
        std::fs::write(
            dir.path().join("firebase.json"),
            r#"{
  "firestore": {"rules": "firestore.rules"},
  "hosting": {"public": "dist"},
  "emulators": {"firestore": {"port": 8080}}
}"#,
        )
        .expect("write firebase.json");
        write_package_json(dir.path(), r#"{"dependencies": {"firebase": "^11.0.0"}}"#);
        assert_eq!(
            detect_dev_adapter(dir.path()).expect("detection should succeed"),
            Some(DevAdapter::FirestoreClient)
        );
    }

    #[test]
    fn firebase_json_functions_config_outranks_firestore_client() {
        let dir = tempfile::tempdir().expect("tempdir");
        // The functions-config disambiguation, Functions direction: the same
        // app with a functions config stays Cloud Functions even though the
        // firebase dependency would otherwise detect FirestoreClient.
        std::fs::create_dir_all(dir.path().join("functions")).expect("create functions dir");
        std::fs::write(
            dir.path().join("firebase.json"),
            r#"{"functions": {"source": "functions"}, "firestore": {"rules": "firestore.rules"}}"#,
        )
        .expect("write firebase.json");
        write_package_json(dir.path(), r#"{"dependencies": {"firebase": "^11.0.0"}}"#);
        assert_eq!(
            detect_dev_adapter(dir.path()).expect("detection should succeed"),
            Some(DevAdapter::CloudFunctions {
                source_roots: vec![dir.path().join("functions").canonicalize().unwrap()],
            })
        );
    }

    #[test]
    fn functions_framework_dep_outranks_firestore_client() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_package_json(
            dir.path(),
            r#"{"dependencies": {"@google-cloud/functions-framework": "^3.0.0", "firebase": "^11.0.0"}}"#,
        );
        assert_eq!(
            detect_dev_adapter(dir.path()).expect("detection should succeed"),
            Some(DevAdapter::CloudFunctions {
                source_roots: vec![dir.path().to_path_buf()],
            })
        );
    }

    #[test]
    fn convex_dir_outranks_firestore_client() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("convex")).expect("create convex dir");
        write_package_json(dir.path(), r#"{"dependencies": {"firebase": "^11.0.0"}}"#);
        assert_eq!(
            detect_dev_adapter(dir.path()).expect("detection should succeed"),
            Some(DevAdapter::Convex {
                source_root: dir.path().join("convex"),
            })
        );
    }

    #[test]
    fn firestore_only_firebase_json_without_firebase_dep_detects_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("firebase.json"),
            r#"{"hosting": {"public": "dist"}}"#,
        )
        .expect("write firebase.json");
        assert_eq!(
            detect_dev_adapter(dir.path()).expect("detection should succeed"),
            None
        );
    }

    #[test]
    fn firestore_client_has_no_source_roots_and_installs_in_app_dir() {
        let adapter = DevAdapter::FirestoreClient;
        assert_eq!(adapter.name(), "firestore-client");
        assert!(adapter.source_roots().is_empty());
        assert!(adapter.needs_node_dependencies());
        assert_eq!(adapter.provision_target(), None);
        assert_eq!(
            adapter.npm_install_dirs(Path::new("/project")),
            vec![PathBuf::from("/project")]
        );
    }
}
