//! Maps a Firestore client app to the dev tenant it will address.
//!
//! The Firestore-compatible routes resolve the `projects/{project_id}` path
//! segment directly to the tenant of the same name, so the project id a
//! client app addresses IS the tenant dev must auto-create. Discovery order
//! (first valid candidate wins):
//!
//! 1. `.firebaserc` `projects.default` — the Firebase CLI's own record of
//!    the project the app belongs to.
//! 2. The first `projectId: "<literal>"` in scanned sources, in
//!    deterministic file/line order — `initializeApp` config commonly lives
//!    in a separate object literal, so the scan matches the key anywhere,
//!    not only inside the call.
//! 3. The standard dev `demo` tenant.
//!
//! Normalization is identity-with-validation: a candidate is used only when
//! it is already a valid Nimbus tenant id (every real Firebase project id
//! is — lowercase letters, digits, hyphens). An invalid candidate can never
//! round-trip — the serve side rejects the project segment outright — so it
//! is skipped and the next signal consulted rather than rewritten into a
//! name the app never addresses. When `.firebaserc` and a source literal
//! disagree, `.firebaserc` wins by order; the banner names the winning
//! source so the developer can see which one dev believed.

use std::io;
use std::path::{Path, PathBuf};

use nimbus::TenantId;

use super::firebase_scan::{
    SCANNED_EXTENSIONS, parse_string_literal, word_continues_at, word_starts_at,
};
use super::watch::should_skip_watch_dir;

/// The tenant dev auto-creates when discovery finds no valid project id.
pub(super) const DEMO_TENANT: &str = "demo";

/// The tenant a Firestore client app's requests will resolve to, plus the
/// signal that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProjectTenantMapping {
    pub(super) tenant: String,
    pub(super) source: ProjectTenantSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ProjectTenantSource {
    /// `.firebaserc` `projects.default`.
    FirebaseRc,
    /// A `projectId: "<literal>"` in app sources (file relative to the app
    /// dir, 1-based line).
    SourceLiteral { file: PathBuf, line: usize },
    /// No valid project id discovered.
    DemoFallback,
}

impl ProjectTenantMapping {
    /// One human phrase naming where the tenant mapping came from, for the
    /// dev banner.
    pub(super) fn describe_source(&self) -> String {
        match &self.source {
            ProjectTenantSource::FirebaseRc => ".firebaserc default project".to_string(),
            ProjectTenantSource::SourceLiteral { file, line } => {
                format!("projectId in {}:{line}", file.display())
            }
            ProjectTenantSource::DemoFallback => "no Firebase project id found".to_string(),
        }
    }
}

/// Discover the tenant a Firestore client app will address under dev.
pub(super) fn discover_project_tenant(app_dir: &Path) -> io::Result<ProjectTenantMapping> {
    if let Some(project_id) = firebaserc_default_project(app_dir)
        && is_valid_tenant(&project_id)
    {
        return Ok(ProjectTenantMapping {
            tenant: project_id,
            source: ProjectTenantSource::FirebaseRc,
        });
    }
    if let Some((project_id, file, line)) = scan_project_id_literal(app_dir, app_dir)? {
        return Ok(ProjectTenantMapping {
            tenant: project_id,
            source: ProjectTenantSource::SourceLiteral { file, line },
        });
    }
    Ok(ProjectTenantMapping {
        tenant: DEMO_TENANT.to_string(),
        source: ProjectTenantSource::DemoFallback,
    })
}

/// `.firebaserc` `projects.default`, when present and well-formed. A
/// malformed or partial file is a skipped signal, not an error — detection
/// must never wedge dev startup on a file Nimbus does not own.
fn firebaserc_default_project(app_dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(app_dir.join(".firebaserc")).ok()?;
    let parsed = serde_json::from_str::<serde_json::Value>(&content).ok()?;
    parsed["projects"]["default"]
        .as_str()
        .map(|value| value.to_string())
}

/// Walk the same file set as the import scan (same extensions, same skip
/// dirs, sorted entries, symlinks skipped) and return the first valid
/// `projectId: "<literal>"` with its file:line.
fn scan_project_id_literal(
    app_dir: &Path,
    dir: &Path,
) -> io::Result<Option<(String, PathBuf, usize)>> {
    let mut entries = std::fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if should_skip_watch_dir(&path) {
                continue;
            }
            if let Some(found) = scan_project_id_literal(app_dir, &path)? {
                return Ok(Some(found));
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if !SCANNED_EXTENSIONS.contains(&extension) {
            continue;
        }
        let bytes = std::fs::read(&path)?;
        let content = String::from_utf8_lossy(&bytes);
        for (index, line) in content.lines().enumerate() {
            if let Some(project_id) = project_id_literal_in_line(line) {
                let relative = path.strip_prefix(app_dir).unwrap_or(&path).to_path_buf();
                return Ok(Some((project_id, relative, index + 1)));
            }
        }
    }
    Ok(None)
}

/// First valid `projectId: "<literal>"` in the line. The key may be a
/// quoted object key (`"projectId":`); the value must be a plain string
/// literal — env-var or computed project ids yield nothing, so the demo
/// fallback applies and the app's own runtime value decides which tenant
/// its requests hit.
fn project_id_literal_in_line(line: &str) -> Option<String> {
    let mut search = 0;
    while let Some(offset) = line[search..].find("projectId") {
        let start = search + offset;
        let end = start + "projectId".len();
        search = end;
        if !word_starts_at(line, start) || word_continues_at(line, end) {
            continue;
        }
        let after = line[end..]
            .strip_prefix(['"', '\''])
            .unwrap_or(&line[end..])
            .trim_start();
        let Some(value_text) = after.strip_prefix(':') else {
            continue;
        };
        let Some(candidate) = parse_string_literal(value_text.trim_start()) else {
            continue;
        };
        if is_valid_tenant(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_valid_tenant(candidate: &str) -> bool {
    TenantId::new(candidate).is_ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;

    fn write_file(app_dir: &Path, relative: &str, contents: &str) {
        let path = app_dir.join(relative);
        fs::create_dir_all(path.parent().expect("path should have a parent"))
            .expect("dirs should create");
        fs::write(path, contents).expect("file should write");
    }

    #[test]
    fn firebaserc_default_project_wins_over_source_literal() {
        let dir = tempdir().expect("tempdir");
        write_file(
            dir.path(),
            ".firebaserc",
            r#"{"projects": {"default": "acme-from-rc"}}"#,
        );
        write_file(
            dir.path(),
            "src/firebase.ts",
            "const config = { projectId: \"acme-from-source\" };\n",
        );
        assert_eq!(
            discover_project_tenant(dir.path()).expect("discovery should succeed"),
            ProjectTenantMapping {
                tenant: "acme-from-rc".to_string(),
                source: ProjectTenantSource::FirebaseRc,
            }
        );
    }

    #[test]
    fn source_literal_found_with_file_line_when_no_firebaserc() {
        let dir = tempdir().expect("tempdir");
        write_file(
            dir.path(),
            "src/firebase.ts",
            "import { initializeApp } from \"firebase/app\";\n\
             const config = {\n  projectId: \"acme-dev-12345\",\n};\n\
             export const app = initializeApp(config);\n",
        );
        assert_eq!(
            discover_project_tenant(dir.path()).expect("discovery should succeed"),
            ProjectTenantMapping {
                tenant: "acme-dev-12345".to_string(),
                source: ProjectTenantSource::SourceLiteral {
                    file: Path::new("src/firebase.ts").to_path_buf(),
                    line: 3,
                },
            }
        );
    }

    #[test]
    fn demo_fallback_when_no_signals() {
        let dir = tempdir().expect("tempdir");
        write_file(dir.path(), "src/main.ts", "export const nothing = 1;\n");
        assert_eq!(
            discover_project_tenant(dir.path()).expect("discovery should succeed"),
            ProjectTenantMapping {
                tenant: DEMO_TENANT.to_string(),
                source: ProjectTenantSource::DemoFallback,
            }
        );
    }

    #[test]
    fn invalid_firebaserc_project_falls_through_to_source_literal() {
        let dir = tempdir().expect("tempdir");
        // Dots are invalid tenant id characters; the serve side would reject
        // `projects/my.project` outright, so the signal is skipped.
        write_file(
            dir.path(),
            ".firebaserc",
            r#"{"projects": {"default": "my.project"}}"#,
        );
        write_file(
            dir.path(),
            "app.js",
            "initializeApp({ projectId: 'acme-app' });\n",
        );
        let mapping = discover_project_tenant(dir.path()).expect("discovery should succeed");
        assert_eq!(mapping.tenant, "acme-app");
        assert!(matches!(
            mapping.source,
            ProjectTenantSource::SourceLiteral { .. }
        ));
    }

    #[test]
    fn malformed_firebaserc_is_skipped_not_fatal() {
        let dir = tempdir().expect("tempdir");
        write_file(dir.path(), ".firebaserc", "{not json");
        assert_eq!(
            discover_project_tenant(dir.path())
                .expect("discovery should succeed")
                .tenant,
            DEMO_TENANT
        );
    }

    #[test]
    fn quoted_key_matches_and_computed_values_yield_nothing() {
        let dir = tempdir().expect("tempdir");
        write_file(
            dir.path(),
            "a.ts",
            "const config = { projectId: process.env.FIREBASE_PROJECT };\n\
             const other = { xprojectId: \"not-the-key\" };\n\
             const more = { projectIdSuffix: \"also-not-the-key\" };\n",
        );
        write_file(
            dir.path(),
            "b.json.ts",
            "export const options = { \"projectId\": \"from-quoted-key\" };\n",
        );
        let mapping = discover_project_tenant(dir.path()).expect("discovery should succeed");
        assert_eq!(mapping.tenant, "from-quoted-key");
        assert_eq!(
            mapping.source,
            ProjectTenantSource::SourceLiteral {
                file: Path::new("b.json.ts").to_path_buf(),
                line: 1,
            }
        );
    }

    #[test]
    fn node_modules_literals_are_out_of_scan() {
        let dir = tempdir().expect("tempdir");
        write_file(
            dir.path(),
            "node_modules/firebase/index.js",
            "initializeApp({ projectId: \"vendored-project\" });\n",
        );
        assert_eq!(
            discover_project_tenant(dir.path())
                .expect("discovery should succeed")
                .tenant,
            DEMO_TENANT
        );
    }

    #[test]
    fn first_literal_in_sorted_file_order_wins() {
        let dir = tempdir().expect("tempdir");
        write_file(dir.path(), "a.ts", "const x = { projectId: \"first\" };\n");
        write_file(dir.path(), "b.ts", "const y = { projectId: \"second\" };\n");
        let mapping = discover_project_tenant(dir.path()).expect("discovery should succeed");
        assert_eq!(mapping.tenant, "first");
        assert_eq!(
            mapping.source,
            ProjectTenantSource::SourceLiteral {
                file: Path::new("a.ts").to_path_buf(),
                line: 1,
            }
        );
    }
}
