//! Pure-lexical (zero I/O) path checks shared across the workspace.
//!
//! These operate on `Path`/`Component` structure only: no filesystem access,
//! no symlink resolution, no environment lookups. They exist so crates that
//! each hand-rolled a small lexical traversal check (a manifest-relative-path
//! guard, a `/../` reject on a trusted string, an absolute-path normalizer)
//! share one implementation instead of drifting copies. Crates that need to
//! reason about *real* filesystem traversal (symlinks, mount points) still
//! have to canonicalize and check `starts_with(root)` themselves; nothing
//! here can see the filesystem, so it cannot make that guarantee alone.
//!
//! `nimbus-runtime` is intentionally NOT a consumer: it has a hard
//! zero-workspace-dependencies invariant, so its own lexical fold
//! (`runtime_capabilities/paths.rs`) stays a private, textually-similar copy
//! rather than a dependency on this module.

use std::path::{Component, Path, PathBuf};

/// Why [`reject_relative_traversal`] refused a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexicalPathError {
    /// The path was empty (or all whitespace).
    Empty,
    /// The path was rooted (started with a root or a platform prefix).
    Absolute,
    /// The path carried a platform prefix (e.g. a Windows drive letter).
    Prefix,
    /// The path contained a `..` (parent-directory) component.
    ParentDirTraversal,
}

impl std::fmt::Display for LexicalPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LexicalPathError::Empty => write!(f, "path must not be empty"),
            LexicalPathError::Absolute => write!(f, "path must not be absolute"),
            LexicalPathError::Prefix => write!(f, "path must not contain a platform prefix"),
            LexicalPathError::ParentDirTraversal => {
                write!(f, "path must not contain parent-directory traversal")
            }
        }
    }
}

impl std::error::Error for LexicalPathError {}

/// Lexically normalizes `path`: a root component clears any parts
/// accumulated so far, `.` components are dropped, and a `..` component pops
/// the last accumulated part -- or is silently absorbed once there is
/// nothing left to pop. This is NOT an escape check: a `..` that would climb
/// above `path`'s own root is a no-op, not an error. Any platform prefix
/// (e.g. a Windows drive letter) is preserved verbatim.
///
/// This is a pure `Component` fold: it does not touch the filesystem and
/// does not resolve symlinks. Callers that need real traversal protection
/// must canonicalize and check `starts_with(root)` themselves; this helper
/// only removes redundant `.`/`..` bookkeeping ahead of that check.
pub fn normalize_absolute_lexical(path: &Path) -> PathBuf {
    let mut prefix = None::<std::ffi::OsString>;
    let mut has_root = false;
    let mut parts = Vec::<std::ffi::OsString>::new();

    for component in path.components() {
        match component {
            Component::Prefix(value) => {
                prefix = Some(value.as_os_str().to_os_string());
            }
            Component::RootDir => {
                has_root = true;
                parts.clear();
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !parts.is_empty() {
                    parts.pop();
                }
            }
            Component::Normal(part) => parts.push(part.to_os_string()),
        }
    }

    let mut normalized = PathBuf::new();
    if let Some(prefix) = prefix {
        normalized.push(prefix);
    }
    if has_root {
        normalized.push(std::path::MAIN_SEPARATOR.to_string());
    }
    for part in parts {
        normalized.push(part);
    }
    normalized
}

/// Rejects `value` unless it is a plain relative path with no traversal:
/// non-empty, not absolute, no platform prefix, no root component, and no
/// `..` component. This is the check for paths that must stay lexically
/// confined under a caller-controlled relative root (a manifest-declared
/// package path, an importer file, ...) where the caller does not want to
/// canonicalize because the target need not exist on disk yet.
pub fn reject_relative_traversal(value: &str) -> Result<(), LexicalPathError> {
    if value.trim().is_empty() {
        return Err(LexicalPathError::Empty);
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(LexicalPathError::Absolute);
    }
    for component in path.components() {
        match component {
            Component::Prefix(_) => return Err(LexicalPathError::Prefix),
            Component::RootDir => return Err(LexicalPathError::Absolute),
            Component::ParentDir => return Err(LexicalPathError::ParentDirTraversal),
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Ok(())
}

/// True if `path` contains any `..` (parent-directory) component. A coarser,
/// boolean-only primitive for callers that already validated the path's
/// shape by their own rules (e.g. "must be absolute", "must have no control
/// characters") and only need the traversal test out of this module.
pub fn has_parent_dir_component(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- normalize_absolute_lexical: locks in runtime paths.rs's current
    // `normalize_absolute_path_lexically` semantics (a pure fold with no
    // escape check -- a `..` past the root is absorbed, not rejected; the
    // caller's later canonicalize + starts_with(root) check is what actually
    // enforces containment).

    #[test]
    fn normalizes_dot_and_dot_dot_segments() {
        assert_eq!(
            normalize_absolute_lexical(Path::new("/a/b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            normalize_absolute_lexical(Path::new("/./a/./b")),
            PathBuf::from("/a/b")
        );
    }

    #[test]
    fn absorbs_parent_dir_past_root_without_erroring() {
        assert_eq!(
            normalize_absolute_lexical(Path::new("/a/../../b")),
            PathBuf::from("/b")
        );
        assert_eq!(
            normalize_absolute_lexical(Path::new("/../../../etc")),
            PathBuf::from("/etc")
        );
    }

    #[test]
    fn root_component_clears_accumulated_parts() {
        assert_eq!(
            normalize_absolute_lexical(Path::new("/a/b//c")),
            PathBuf::from("/a/b/c")
        );
    }

    // --- reject_relative_traversal: locks in convex loading.rs's current
    // `validate_relative_manifest_path` semantics exactly (empty/absolute/
    // Prefix/RootDir/ParentDir reject; CurDir and Normal allowed).

    #[test]
    fn relative_traversal_accepts_plain_relative_paths() {
        assert!(reject_relative_traversal("a/b").is_ok());
        assert!(reject_relative_traversal("./a/b").is_ok());
        assert!(reject_relative_traversal("a").is_ok());
    }

    #[test]
    fn relative_traversal_rejects_empty_and_blank() {
        assert_eq!(reject_relative_traversal(""), Err(LexicalPathError::Empty));
        assert_eq!(
            reject_relative_traversal("   "),
            Err(LexicalPathError::Empty)
        );
    }

    #[test]
    fn relative_traversal_rejects_absolute_paths() {
        assert_eq!(
            reject_relative_traversal("/a/b"),
            Err(LexicalPathError::Absolute)
        );
    }

    #[test]
    fn relative_traversal_rejects_parent_dir_anywhere() {
        assert_eq!(
            reject_relative_traversal("a/../b"),
            Err(LexicalPathError::ParentDirTraversal)
        );
        assert_eq!(
            reject_relative_traversal(".."),
            Err(LexicalPathError::ParentDirTraversal)
        );
    }

    // --- has_parent_dir_component: locks in node host_lifecycle.rs's
    // current `contains("/../") || ends_with("/..")` semantics for realistic
    // trusted-runner-bundle-path inputs (always absolute, already validated
    // for control characters by the caller before this check runs).

    #[test]
    fn parent_dir_component_detects_mid_path_traversal() {
        assert!(has_parent_dir_component(Path::new("/run/nimbus/../escape")));
    }

    #[test]
    fn parent_dir_component_detects_trailing_traversal() {
        assert!(has_parent_dir_component(Path::new("/a/..")));
        assert!(has_parent_dir_component(Path::new("/..")));
    }

    #[test]
    fn parent_dir_component_ignores_dotdot_prefixed_names() {
        assert!(!has_parent_dir_component(Path::new("/a/..b/c")));
        assert!(!has_parent_dir_component(Path::new("/a/foo..")));
    }

    #[test]
    fn parent_dir_component_absent_on_plain_paths() {
        assert!(!has_parent_dir_component(Path::new("/run/nimbus/bundle")));
    }
}
