//! Live re-detection for the dev loop (DXL1). A poll loop watches the
//! app's manifests — `package.json`, `firebase.json`, `.firebaserc`, and
//! the adapter source dirs — and re-runs detection when any of them
//! change. Adoption of a driver dependency mid-session is
//! presentation-only by construction (D6): every wire listener has been
//! serving since boot on the ports the boot-time [`WirePlan`] resolved,
//! so a rescan refreshes the Nimbus-owned `.env.local` keys and prints a
//! notice pointing at endpoints that never moved. The listener set, the
//! main listener, and any open subscriptions are untouched because a
//! rescan writes one file and emits stderr lines — nothing else.

use std::path::Path;
use std::time::{Duration, SystemTime};

use crate::cli_ux;

use super::env_file::write_env_local_nimbus_keys;
use super::surfaces::{WireSurfaces, detect_wire_surfaces};
use super::wire::WirePlan;

const MANIFEST_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Manifest files whose edits re-run detection.
const MANIFEST_FILES: &[&str] = &["package.json", "firebase.json", ".firebaserc"];

/// Adapter source roots whose creation or removal re-runs detection.
const ADAPTER_DIRS: &[&str] = &["convex", "nimbus"];

/// Fingerprint of the watched manifest set. Two equal snapshots mean no
/// detection input changed; everything else in the app dir (sources,
/// build output, `node_modules/`) is deliberately outside this set — the
/// codegen watch loop owns source changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ManifestSnapshot {
    files: Vec<FileStamp>,
    adapter_dirs: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileStamp {
    Absent,
    Present {
        len: u64,
        modified: Option<SystemTime>,
    },
}

/// Stamp the watched manifests. Unreadable entries count as absent so a
/// transient I/O error degrades to a retry on the next poll instead of
/// killing the loop.
pub(super) fn manifest_snapshot(app_dir: &Path) -> ManifestSnapshot {
    let files = MANIFEST_FILES
        .iter()
        .map(|name| match std::fs::metadata(app_dir.join(name)) {
            Ok(metadata) if metadata.is_file() => FileStamp::Present {
                len: metadata.len(),
                modified: metadata.modified().ok(),
            },
            _ => FileStamp::Absent,
        })
        .collect();
    let adapter_dirs = ADAPTER_DIRS
        .iter()
        .map(|name| app_dir.join(name).is_dir())
        .collect();
    ManifestSnapshot {
        files,
        adapter_dirs,
    }
}

/// Outcome of one manifest rescan: the re-detected surface set plus the
/// notices a newly detected signal earns.
pub(super) struct WireRescan {
    pub(super) surfaces: WireSurfaces,
    pub(super) notices: Vec<String>,
}

/// Re-detect wire surfaces and converge `.env.local` on the boot-time
/// wire plan. The entries always come from the same [`WirePlan`] the
/// listeners were bound from, so the advertised endpoints are the ones
/// already serving — never a re-probe that could drift from reality.
/// Newly detected surfaces earn a notice naming the env key and the
/// live endpoint; an unchanged surface set writes nothing (the env
/// writer no-ops on identical content) and says nothing. A surface
/// whose dependency was removed keeps its keys: the listener behind
/// them is still serving for the rest of the session (D6), so the keys
/// remain true.
pub(super) fn rescan_wire_presentation(
    app_dir: &Path,
    wire: &WirePlan,
    previous: WireSurfaces,
) -> std::io::Result<WireRescan> {
    let surfaces = detect_wire_surfaces(app_dir);
    write_env_local_nimbus_keys(app_dir, &wire.env_local_entries(surfaces))?;
    let mut notices = Vec::new();
    if surfaces.mongodb && !previous.mongodb {
        notices.push(format!(
            "detected mongodb dependency: NIMBUS_MONGODB_URL added to .env.local \
             (listener already serving on 127.0.0.1:{})",
            wire.mongodb_port.port
        ));
    }
    if surfaces.dynamodb && !previous.dynamodb {
        notices.push(format!(
            "detected DynamoDB SDK dependency: NIMBUS_DYNAMODB_ENDPOINT and access keys \
             added to .env.local (listener already serving on 127.0.0.1:{})",
            wire.dynamodb_port.port
        ));
    }
    if surfaces.aws_sdk_v2_hint && !previous.aws_sdk_v2_hint {
        notices.push(
            "aws-sdk v2 detected; @aws-sdk/client-dynamodb (v3) enables automatic \
             DynamoDB endpoint + credentials in .env.local"
                .to_string(),
        );
    }
    Ok(WireRescan { surfaces, notices })
}

/// Long-running manifest watch arm of the dev loop. Polls the manifest
/// snapshot and rescans on change. Runs for the life of the session.
pub(super) async fn run_manifest_watch_loop(
    app_dir: &Path,
    wire: &WirePlan,
    initial_surfaces: WireSurfaces,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut snapshot = manifest_snapshot(app_dir);
    let mut surfaces = initial_surfaces;
    loop {
        tokio::time::sleep(MANIFEST_POLL_INTERVAL).await;
        let next = manifest_snapshot(app_dir);
        if next == snapshot {
            continue;
        }
        snapshot = next;
        match rescan_wire_presentation(app_dir, wire, surfaces) {
            Ok(rescan) => {
                for notice in &rescan.notices {
                    let _ = cli_ux::write_stderr_prefixed_line("info:", notice);
                }
                surfaces = rescan.surfaces;
            }
            Err(error) => {
                let _ = cli_ux::write_stderr_prefixed_line(
                    "warning:",
                    &format!("manifest change rescan failed: {error}"),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::*;

    fn sorted_dir_entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("app dir should list")
            .map(|entry| {
                entry
                    .expect("dir entry should read")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn repeated_manifest_rescans_are_convergent() {
        // DXL1: a rescan converges — repeated manifest touches with an
        // unchanged dependency set keep `.env.local` byte-stable, announce
        // nothing new, and create nothing in the app dir (no codegen, no
        // installs, no provisioning).
        let temp = tempdir().expect("tempdir should build");
        fs::write(
            temp.path().join(".env.local"),
            "MONGODB_URI=mongodb://prod.example.com/\n",
        )
        .expect("user env should seed");
        let manifest =
            r#"{"dependencies": {"mongodb": "^6.0.0", "@aws-sdk/client-dynamodb": "^3.600.0"}}"#;
        fs::write(temp.path().join("package.json"), manifest).expect("package.json should write");

        let wire = WirePlan::fixture();
        let first = rescan_wire_presentation(temp.path(), &wire, WireSurfaces::default())
            .expect("first rescan should succeed");
        assert!(first.surfaces.mongodb && first.surfaces.dynamodb);
        assert_eq!(
            first.notices.len(),
            2,
            "both newly detected surfaces should announce once: {:?}",
            first.notices
        );

        let env_path = temp.path().join(".env.local");
        let after_first = fs::read_to_string(&env_path).expect("env file should read");
        assert!(
            after_first.starts_with("MONGODB_URI=mongodb://prod.example.com/"),
            "the user-owned key must stay untouched: {after_first}"
        );
        assert!(after_first.contains("NIMBUS_MONGODB_URL="));
        assert!(after_first.contains("NIMBUS_DYNAMODB_ENDPOINT="));
        let baseline_entries = sorted_dir_entries(temp.path());

        for _ in 0..3 {
            // Touch the manifest without changing the dependency set; the
            // mtime moves, the detection outcome does not.
            fs::write(temp.path().join("package.json"), manifest)
                .expect("package.json should rewrite");
            let rescan = rescan_wire_presentation(temp.path(), &wire, first.surfaces)
                .expect("repeat rescan should succeed");
            assert_eq!(rescan.surfaces, first.surfaces);
            assert!(
                rescan.notices.is_empty(),
                "an unchanged surface set must not re-announce: {:?}",
                rescan.notices
            );
            assert_eq!(
                fs::read_to_string(&env_path).expect("env file should read"),
                after_first,
                ".env.local must stay byte-stable across repeated rescans"
            );
            assert_eq!(
                sorted_dir_entries(temp.path()),
                baseline_entries,
                "a rescan must spawn nothing into the app dir"
            );
        }
    }

    #[test]
    fn manifest_snapshot_tracks_manifests_and_adapter_dirs_only() {
        let temp = tempdir().expect("tempdir should build");
        let empty = manifest_snapshot(temp.path());

        fs::write(temp.path().join("package.json"), "{}").expect("package.json should write");
        let with_manifest = manifest_snapshot(temp.path());
        assert_ne!(empty, with_manifest, "a manifest write must trip the watch");

        // Source edits belong to the codegen watch loop, not this one.
        fs::create_dir_all(temp.path().join("src")).expect("src dir should build");
        fs::write(temp.path().join("src").join("app.ts"), "export {};\n")
            .expect("source file should write");
        assert_eq!(
            with_manifest,
            manifest_snapshot(temp.path()),
            "unrelated files must not trip the manifest watch"
        );

        fs::create_dir_all(temp.path().join("convex")).expect("adapter dir should build");
        let with_adapter_dir = manifest_snapshot(temp.path());
        assert_ne!(
            with_manifest, with_adapter_dir,
            "adapter dir creation must trip the watch"
        );

        fs::write(temp.path().join("firebase.json"), "{}").expect("firebase.json should write");
        assert_ne!(
            with_adapter_dir,
            manifest_snapshot(temp.path()),
            "a firebase.json write must trip the watch"
        );
    }
}
