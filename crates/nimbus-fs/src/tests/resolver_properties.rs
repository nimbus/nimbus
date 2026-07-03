//! Property corpus for `MountResolver` path normalization and mount matching.
//!
//! These properties hold for arbitrary path inputs, not just the handful of
//! examples in `tests/mount.rs`: resolution never yields a backend path that
//! escapes the matched mount root, the chosen mount is always the longest
//! matching prefix, masked overlays stay opaque regardless of unrelated
//! surrounding grants, and re-resolving an already-normalized path is a
//! no-op.

use std::path::{Component, Path, PathBuf};

use proptest::prelude::*;

use super::memfs_rc;
use crate::{MountResolver, MountTable, ResolvedAccess};

/// One path segment drawn from the classes called out in the FCW3 property
/// requirements: ordinary names, `.`/`..`, an empty segment (collapses like a
/// repeated path separator), unicode names, and a very long name.
fn segment_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => "[a-zA-Z0-9_-]{1,8}",
        1 => Just(".".to_string()),
        1 => Just("..".to_string()),
        1 => Just(String::new()),
        1 => Just("héllo-wörld".to_string()),
        1 => Just("文件夹".to_string()),
        1 => Just("🙂link".to_string()),
        1 => "[a-z]{120,240}",
    ]
}

fn path_segments_strategy() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(segment_strategy(), 0..8)
}

fn joined_absolute_path(prefix: &str, segments: &[String]) -> PathBuf {
    let joined = segments.join("/");
    PathBuf::from(format!("{prefix}/{joined}"))
}

/// Mount table used by the escape / longest-prefix properties: a root
/// backend plus two nested backends so the resolver has real prefix
/// ambiguity to resolve.
fn mount_table_for_prefix_property() -> MountTable {
    let mut table = MountTable::new(memfs_rc());
    table.mount("/app", memfs_rc()).unwrap();
    table.mount("/app/cache", memfs_rc()).unwrap();
    table.mount_readonly("/data", memfs_rc()).unwrap();
    table
}

/// Independent (non-resolver) longest-prefix oracle used to check the
/// resolver's choice without reusing its own implementation.
fn reference_longest_prefix(virtual_path: &Path, prefixes: &[&str]) -> PathBuf {
    prefixes
        .iter()
        .map(PathBuf::from)
        .filter(|prefix| prefix.as_path() == Path::new("/") || virtual_path.starts_with(prefix))
        .max_by_key(|prefix| prefix.components().count())
        .expect("root mount always matches an absolute virtual path")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Resolution of an arbitrary path never yields a backend path outside
    /// the matched mount root, and it never picks a shorter mount prefix
    /// when a longer one applies.
    #[test]
    fn resolver_property_corpus_never_escapes_and_picks_longest_prefix(
        segments in path_segments_strategy(),
    ) {
        let resolver = MountResolver::new(mount_table_for_prefix_property());
        let input = joined_absolute_path("", &segments);

        match resolver.resolve(Path::new("/"), &input) {
            Ok(resolved) => {
                prop_assert!(
                    !resolved
                        .backend_path
                        .components()
                        .any(|component| matches!(component, Component::ParentDir)),
                    "backend path escaped via .. oscillation: {:?}",
                    resolved.backend_path
                );
                let expected_prefix =
                    reference_longest_prefix(&resolved.virtual_path, &["/", "/app", "/app/cache", "/data"]);
                prop_assert_eq!(
                    resolved.mount_prefix.clone(),
                    expected_prefix,
                    "resolver did not pick the longest matching mount prefix"
                );
            }
            Err(error) => {
                let message = error.to_string();
                prop_assert!(
                    message.contains("escapes NimbusFS"),
                    "unexpected resolver error for a non-escaping input: {message}"
                );
            }
        }
    }

    /// A masked overlay is opaque for every path underneath it, regardless of
    /// what other grants (readonly, read-write) exist elsewhere in the
    /// table.
    #[test]
    fn resolver_property_corpus_masked_overlay_always_wins(
        segments in path_segments_strategy(),
        extra_readonly_sibling in any::<bool>(),
    ) {
        let mut table = MountTable::new(memfs_rc());
        table.mount("/app", memfs_rc()).unwrap();
        table.mount("/other-grant", memfs_rc()).unwrap();
        if extra_readonly_sibling {
            table.mount_readonly("/app/ro-sibling", memfs_rc()).unwrap();
        }
        table.mount_masked("/app/secret").unwrap();
        let resolver = MountResolver::new(table);

        let input = joined_absolute_path("/app/secret", &segments);

        match resolver.resolve(Path::new("/"), &input) {
            Ok(resolved) => {
                prop_assert_eq!(resolved.access(), ResolvedAccess::Masked);
                let error = resolved
                    .ensure_readable()
                    .expect_err("masked overlay must stay opaque regardless of surrounding grants");
                prop_assert!(error.to_string().contains("masked"));
            }
            Err(error) => {
                // The only other legal outcome is an explicit escape past
                // the masked mount root itself (`..` back out of it).
                prop_assert!(error.to_string().contains("escapes NimbusFS"));
            }
        }
    }

    /// Resolving an already-normalized (resolved) virtual path again yields
    /// the same mount and backend path -- resolution is idempotent.
    #[test]
    fn resolver_property_corpus_resolution_is_idempotent(segments in path_segments_strategy()) {
        let resolver = MountResolver::new(mount_table_for_prefix_property());
        let input = joined_absolute_path("", &segments);

        if let Ok(first) = resolver.resolve(Path::new("/"), &input) {
            let second = resolver
                .resolve(Path::new("/"), &first.virtual_path)
                .expect("re-resolving an already normalized path must not fail");
            prop_assert_eq!(first.mount_prefix, second.mount_prefix);
            prop_assert_eq!(first.backend_path, second.backend_path);
        }
    }
}
