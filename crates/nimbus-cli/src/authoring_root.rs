//! Which directory an app authors server functions in, and which embedded
//! package those functions import.
//!
//! Codegen decides both: it picks the source root, writes `_generated` inside
//! it, and bakes the matching package namespace into every generated import.
//! Everything that wires dependencies or reads sources afterwards has to agree
//! with that choice, or the app imports one package and declares another.
//!
//! The precedence therefore mirrors `resolveSourceRoot` in
//! `packages/codegen/src/app.mjs`, which is the authority:
//!
//! 1. `convex.json`'s `"functions"` override. It is a Convex-specific setting,
//!    so it always resolves to the `convex` package — never to `@nimbus/nimbus`
//!    — whatever the relocated directory happens to be named.
//! 2. `nimbus/` — the Nimbus-native root, authored against `@nimbus/nimbus`.
//! 3. `convex/` — the compatibility root, authored against `convex`.
//!
//! `nimbus/` beats `convex/` when an app has both, which is why this order is
//! worth centralizing: reversing it in one caller wires the app to a package
//! its generated code never imports.

use std::io;
use std::path::{Path, PathBuf};

/// The provisioning target for the Convex compatibility package.
pub(crate) const CONVEX_TARGET: &str = "convex";
/// The provisioning target for the Nimbus-native SDK. `provision::Selection`
/// resolves it through the embedded manifest's `source_dir` to the
/// `@nimbus/nimbus` package.
pub(crate) const NIMBUS_TARGET: &str = "nimbus";

/// An app's authoring root and the package its functions import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthoringRoot {
    /// The directory holding the app's server functions, below `app_dir`.
    pub(crate) source_root: PathBuf,
    /// The `provision::Selection` target for the package those functions
    /// import — the dependency the app's `package.json` must declare.
    pub(crate) package_target: &'static str,
}

/// Resolve an app's authoring root, or `None` when the app has no server
/// functions directory.
///
/// Fails when `convex.json` declares a `"functions"` directory that does not
/// exist: an explicit override is an unambiguous signal, so a missing target is
/// a real misconfiguration to surface rather than something to fall through
/// past to a directory the developer did not name.
pub(crate) fn resolve(app_dir: &Path) -> io::Result<Option<AuthoringRoot>> {
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
        return Ok(Some(AuthoringRoot {
            source_root: functions_root,
            package_target: CONVEX_TARGET,
        }));
    }

    let nimbus_root = app_dir.join("nimbus");
    if nimbus_root.is_dir() {
        return Ok(Some(AuthoringRoot {
            source_root: nimbus_root,
            package_target: NIMBUS_TARGET,
        }));
    }

    let convex_root = app_dir.join("convex");
    if convex_root.is_dir() {
        return Ok(Some(AuthoringRoot {
            source_root: convex_root,
            package_target: CONVEX_TARGET,
        }));
    }

    Ok(None)
}

fn read_convex_json_functions_field(app_dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(app_dir.join("convex.json")).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    parsed.get("functions")?.as_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(files: &[(&str, &str)], dirs: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for sub in dirs {
            std::fs::create_dir_all(dir.path().join(sub)).expect("create dir");
        }
        for (path, contents) in files {
            std::fs::write(dir.path().join(path), contents).expect("write file");
        }
        dir
    }

    #[test]
    fn an_app_with_no_functions_directory_has_no_authoring_root() {
        let dir = app(&[], &[]);
        assert_eq!(resolve(dir.path()).expect("resolve"), None);
    }

    #[test]
    fn a_convex_only_app_authors_against_the_compatibility_package() {
        let dir = app(&[], &["convex"]);
        let root = resolve(dir.path()).expect("resolve").expect("root");
        assert_eq!(root.source_root, dir.path().join("convex"));
        assert_eq!(root.package_target, CONVEX_TARGET);
    }

    #[test]
    fn a_nimbus_only_app_authors_against_the_native_sdk() {
        let dir = app(&[], &["nimbus"]);
        let root = resolve(dir.path()).expect("resolve").expect("root");
        assert_eq!(root.source_root, dir.path().join("nimbus"));
        assert_eq!(root.package_target, NIMBUS_TARGET);
    }

    /// Codegen announces `using nimbus/` for a dual-root app and bakes
    /// `@nimbus/nimbus` into its generated imports, so provisioning has to
    /// declare that package rather than `convex`.
    #[test]
    fn a_dual_root_app_resolves_the_way_codegen_does() {
        let dir = app(&[], &["convex", "nimbus"]);
        let root = resolve(dir.path()).expect("resolve").expect("root");
        assert_eq!(root.source_root, dir.path().join("nimbus"));
        assert_eq!(root.package_target, NIMBUS_TARGET);
    }

    /// `"functions"` is a Convex setting. Even pointed at a directory named
    /// `nimbus`, it selects the compatibility package — matching
    /// `resolveSourceRoot`, which returns `packageNamespace: "convex"` for
    /// every override without re-running the dual-root heuristic.
    #[test]
    fn a_functions_override_always_selects_the_compatibility_package() {
        let dir = app(
            &[("convex.json", r#"{"functions": "nimbus"}"#)],
            &["nimbus"],
        );
        let root = resolve(dir.path()).expect("resolve").expect("root");
        assert_eq!(root.source_root, dir.path().join("nimbus"));
        assert_eq!(root.package_target, CONVEX_TARGET);
    }

    #[test]
    fn a_functions_override_relocates_the_source_root() {
        let dir = app(
            &[("convex.json", r#"{"functions": "src/backend"}"#)],
            &["src/backend"],
        );
        let root = resolve(dir.path()).expect("resolve").expect("root");
        assert_eq!(root.source_root, dir.path().join("src/backend"));
        assert_eq!(root.package_target, CONVEX_TARGET);
    }

    #[test]
    fn a_functions_override_naming_a_missing_directory_is_reported() {
        let dir = app(&[("convex.json", r#"{"functions": "src/backend"}"#)], &[]);
        let error = resolve(dir.path()).expect_err("missing override dir should fail");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            error.to_string().contains("src/backend"),
            "error should name the missing directory, got: {error}"
        );
    }

    /// A `convex.json` without `"functions"` — or one that is not valid JSON —
    /// leaves the directory heuristic in charge rather than failing the app.
    #[test]
    fn a_convex_json_without_a_functions_field_falls_through_to_the_directories() {
        let dir = app(
            &[("convex.json", r#"{"origin": "https://example.test"}"#)],
            &["convex"],
        );
        let root = resolve(dir.path()).expect("resolve").expect("root");
        assert_eq!(root.source_root, dir.path().join("convex"));
        assert_eq!(root.package_target, CONVEX_TARGET);
    }
}
