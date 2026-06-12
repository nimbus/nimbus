//! Live re-detection for the dev loop (DXL1/DXL2). A poll loop watches
//! the app's manifests — `package.json`, `firebase.json`, `.firebaserc`,
//! and the adapter source dirs — and re-runs detection when any of them
//! change. Two adoption lanes hang off a rescan:
//!
//! - **Wire surfaces (DXL1)**: presentation-only by construction (D6).
//!   Every wire listener has been serving since boot on the ports the
//!   boot-time [`WirePlan`] resolved, so a rescan refreshes the
//!   Nimbus-owned `.env.local` keys and prints a notice pointing at
//!   endpoints that never moved.
//! - **App adapter (DXL2)**: a newly detected adapter is adopted through
//!   exactly the boot-time flow — the Firebase lane stays behind the
//!   same fail-closed import scan (the only app-mutating path), the
//!   Convex/Cloud Functions lane provisions + installs, and the adopted
//!   source roots are registered with the codegen watch loop over a
//!   watch channel. An adoption failure downgrades to a warning and
//!   keeps the previous adapter, so the next manifest change retries.
//!
//! The listener set, the main listener, and any open subscriptions are
//! untouched in both lanes — nothing restarts.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::cli_ux;

use super::adapter::{DevAdapter, detect_dev_adapter};
use super::env_file::write_env_local_nimbus_keys;
use super::firebase_project::discover_project_tenant;
use super::surfaces::{WireSurfaces, detect_wire_surfaces};
use super::wire::{AWS_SDK_V2_HINT, WirePlan};

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
    let mut notices: Vec<String> = wire
        .surface_presentations(surfaces)
        .into_iter()
        .zip(wire.surface_presentations(previous))
        .filter(|(now, before)| now.detected && !before.detected)
        .map(|(now, _)| {
            format!(
                "detected {}: {} added to .env.local \
                 (listener already serving on 127.0.0.1:{})",
                now.dependency_label, now.env_keys_label, now.port.port
            )
        })
        .collect();
    // The aws-sdk v2 hint is not a surface (D3): it never enables a
    // listener or env keys, so it announces from the shared hint text.
    if surfaces.aws_sdk_v2_hint && !previous.aws_sdk_v2_hint {
        notices.push(AWS_SDK_V2_HINT.to_string());
    }
    Ok(WireRescan { surfaces, notices })
}

/// Everything the manifest watch loop needs from the boot-time plan.
pub(super) struct ManifestWatch<'a> {
    pub(super) app_dir: &'a Path,
    pub(super) wire: &'a WirePlan,
    pub(super) initial_surfaces: WireSurfaces,
    /// The adapter detection resolved at boot; `None` when dev started
    /// without one (D8 keeps that session serving so an adapter can be
    /// adopted here).
    pub(super) initial_adapter: Option<DevAdapter>,
    /// The tenant the boot-time plan auto-created. A Firestore client app
    /// adopted mid-session may map to a different project tenant; that
    /// earns an honest restart notice instead of a silent mismatch.
    pub(super) boot_auto_tenant: String,
    /// Registers the adopted adapter's source roots with the codegen
    /// watch loop.
    pub(super) watch_roots: &'a tokio::sync::watch::Sender<Vec<PathBuf>>,
}

/// Long-running manifest watch arm of the dev loop. Polls the manifest
/// snapshot and rescans on change. Runs for the life of the session.
pub(super) async fn run_manifest_watch_loop(
    watch: ManifestWatch<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut snapshot = manifest_snapshot(watch.app_dir);
    let mut surfaces = watch.initial_surfaces;
    let mut adapter = watch.initial_adapter.clone();
    loop {
        tokio::time::sleep(MANIFEST_POLL_INTERVAL).await;
        let next = manifest_snapshot(watch.app_dir);
        if next == snapshot {
            continue;
        }
        snapshot = next;
        match rescan_wire_presentation(watch.app_dir, watch.wire, surfaces) {
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
        match adopt_app_adapter(&watch, adapter.as_ref()).await {
            Ok(rescan) => {
                for notice in &rescan.notices {
                    let _ = cli_ux::write_stderr_prefixed_line("info:", notice);
                }
                adapter = rescan.adapter;
            }
            // The previous adapter stays in effect: `adapter` is unchanged,
            // so the next manifest change re-detects and retries.
            Err(error) => {
                let _ = cli_ux::write_stderr_prefixed_line(
                    "warning:",
                    &format!(
                        "mid-session adapter adoption failed: {error} \
                         (fix the issue and touch package.json to retry)"
                    ),
                );
            }
        }
    }
}

/// Outcome of one adapter re-detection: the adapter now in effect plus
/// the notices the adoption earned. Mirrors [`WireRescan`] — adoption
/// produces data, the watch loop owns all printing.
pub(super) struct AdapterRescan {
    pub(super) adapter: Option<DevAdapter>,
    pub(super) notices: Vec<String>,
}

/// Re-detect the app adapter and adopt a change through the boot-time
/// flow. The Firebase lane is the only one that mutates the app, and it
/// stays behind the same fail-closed import scan as boot — a refusal
/// propagates as `Err` with nothing changed. The Convex/Cloud Functions
/// lane provisions the embedded packages and installs Node dependencies
/// exactly like boot, then registers its source roots with the codegen
/// watch loop (whose baseline reset gives the adopted sources their
/// initial codegen). Returns the adapter now in effect; on `Err` the
/// caller keeps the previous adapter so the next manifest change retries.
async fn adopt_app_adapter(
    watch: &ManifestWatch<'_>,
    previous: Option<&DevAdapter>,
) -> Result<AdapterRescan, Box<dyn std::error::Error>> {
    let detected = detect_dev_adapter(watch.app_dir)?;
    let mut notices = Vec::new();
    if detected.as_ref() == previous {
        return Ok(AdapterRescan {
            adapter: detected,
            notices,
        });
    }
    match &detected {
        None => {
            register_watch_roots(watch.watch_roots, Vec::new());
            notices.push(
                "adapter markers removed; codegen watch idled (the server keeps serving)"
                    .to_string(),
            );
        }
        Some(DevAdapter::FirestoreClient) => {
            super::firebase::wire_firestore_client_app(watch.app_dir)?;
            crate::node::auto_install_node_dependencies(watch.app_dir).await?;
            register_watch_roots(watch.watch_roots, Vec::new());
            notices.push(
                "firebase dependency adopted: package.json now points at the drop-in \
                 firebase package"
                    .to_string(),
            );
            // The boot-time plan already created its auto tenant; this
            // session cannot retroactively map a different project id.
            let mapping = discover_project_tenant(watch.app_dir)?;
            if mapping.tenant != watch.boot_auto_tenant {
                notices.push(format!(
                    "this app addresses project '{}' ({}), but this session serves \
                     tenant '{}'; restart `nimbus dev` to map the project",
                    mapping.tenant,
                    mapping.describe_source(),
                    watch.boot_auto_tenant
                ));
            }
        }
        Some(adapter) => {
            if let Some(target) = adapter.provision_target() {
                let selection = crate::provision::Selection::parse(target)
                    .expect("adapter provision target must be a known selection");
                crate::provision::ensure(watch.app_dir, &selection)?;
            }
            if adapter.needs_node_dependencies() {
                for install_dir in adapter.npm_install_dirs(watch.app_dir) {
                    crate::node::auto_install_node_dependencies(&install_dir).await?;
                }
            }
            register_watch_roots(watch.watch_roots, adapter.source_roots().to_vec());
            notices.push(format!(
                "detected {} sources; codegen watch is now active",
                adapter.name()
            ));
        }
    }
    Ok(AdapterRescan {
        adapter: detected,
        notices,
    })
}

/// Send the roots only when they actually change, so a re-adoption with
/// identical roots never resets the codegen watch baseline.
fn register_watch_roots(sender: &tokio::sync::watch::Sender<Vec<PathBuf>>, roots: Vec<PathBuf>) {
    sender.send_if_modified(|current| {
        if *current == roots {
            false
        } else {
            *current = roots;
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::super::plan::resolve_dev_plan;
    use super::super::watch::run_dev_watch_loop;
    use super::super::{DevCommand, DevTailLogsMode};
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

    #[tokio::test]
    async fn mid_session_firebase_refusal_mutates_nothing_and_keeps_loop_alive() {
        // DXL2 refusal gate: a `firebase` dependency added mid-session goes
        // through the same fail-closed import scan as boot. An uncovered
        // import refuses with zero app mutation, and the manifest loop keeps
        // serving wire-surface presentation afterwards — the refusal is a
        // retriable warning, not a crash.
        let temp = tempdir().expect("tempdir should build");
        fs::write(temp.path().join("package.json"), r#"{"dependencies": {}}"#)
            .expect("package.json should write");
        fs::create_dir_all(temp.path().join("src")).expect("src dir should build");
        fs::write(
            temp.path().join("src/auth.ts"),
            "import { getAuth } from \"firebase/auth\";\n",
        )
        .expect("uncovered source should write");

        let wire = WirePlan::fixture();
        let (watch_roots_tx, watch_roots_rx) = tokio::sync::watch::channel(Vec::new());
        let manifest_loop = run_manifest_watch_loop(ManifestWatch {
            app_dir: temp.path(),
            wire: &wire,
            initial_surfaces: WireSurfaces::default(),
            initial_adapter: None,
            boot_auto_tenant: "demo".to_string(),
            watch_roots: &watch_roots_tx,
        });
        tokio::pin!(manifest_loop);
        // Prime once so the baseline snapshot predates the dependency writes.
        let primed = tokio::time::timeout(Duration::from_millis(50), &mut manifest_loop).await;
        assert!(primed.is_err(), "the manifest watch loop must keep running");

        let with_firebase = r#"{"dependencies": {"firebase": "^11.0.0", "mongodb": "^6.0.0"}}"#;
        fs::write(temp.path().join("package.json"), with_firebase)
            .expect("package.json should gain firebase + mongodb");

        // The same rescan that refuses the firebase adoption must still land
        // the mongodb wire keys: a refusal never blocks presentation.
        let env_path = temp.path().join(".env.local");
        let wait_for_mongodb_key = async {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                if let Ok(content) = fs::read_to_string(&env_path)
                    && content.contains("NIMBUS_MONGODB_URL=")
                {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    ".env.local should gain NIMBUS_MONGODB_URL within 10s of the manifest change"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        tokio::select! {
            _ = &mut manifest_loop => panic!("the manifest watch loop must not exit on refusal"),
            _ = wait_for_mongodb_key => {}
        }

        // A second manifest change after the refusal must still rescan: the
        // loop survived and keeps adopting what it can.
        let with_dynamodb = r#"{"dependencies": {"firebase": "^11.0.0", "mongodb": "^6.0.0", "@aws-sdk/client-dynamodb": "^3.600.0"}}"#;
        fs::write(temp.path().join("package.json"), with_dynamodb)
            .expect("package.json should gain the DynamoDB SDK");
        let wait_for_dynamodb_key = async {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            loop {
                if let Ok(content) = fs::read_to_string(&env_path)
                    && content.contains("NIMBUS_DYNAMODB_ENDPOINT=")
                {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    ".env.local should gain NIMBUS_DYNAMODB_ENDPOINT within 10s — the loop \
                     must stay alive after a refused adoption"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        tokio::select! {
            _ = &mut manifest_loop => panic!("the manifest watch loop must not exit on refusal"),
            _ = wait_for_dynamodb_key => {}
        }

        // Both refused adoptions mutated nothing: manifest bytes intact,
        // nothing provisioned, nothing installed, no watch roots registered.
        assert_eq!(
            fs::read(temp.path().join("package.json")).expect("package.json should read"),
            with_dynamodb.as_bytes(),
            "a refused adoption must leave package.json byte-identical"
        );
        assert!(
            !temp.path().join(".nimbus").exists(),
            "a refused adoption must not provision anything into the app"
        );
        assert!(
            !temp.path().join("node_modules").exists(),
            "a refused adoption must not install anything"
        );
        assert!(
            watch_roots_rx.borrow().is_empty(),
            "a refused adoption must not register watch roots"
        );
    }

    #[tokio::test]
    async fn mid_session_firebase_adoption_runs_scan_gate() {
        // DXL2 adoption gate: a `firebase` dependency added mid-session runs
        // the same scan → wire → install flow as boot. A covered app is
        // rewired to the provisioned drop-in and its Node dependencies are
        // installed without restarting anything; a Firestore client app has
        // no codegen sources, so no watch roots register.
        if let Err(error) = crate::node::ensure_node22_runtime_available() {
            eprintln!(
                "skipping mid_session_firebase_adoption_runs_scan_gate; Node.js baseline unavailable: {error}"
            );
            return;
        }
        if let Err(error) = crate::node::ensure_npm_available() {
            eprintln!(
                "skipping mid_session_firebase_adoption_runs_scan_gate; npm unavailable: {error}"
            );
            return;
        }

        let temp = tempdir().expect("tempdir should build");
        fs::write(temp.path().join("package.json"), r#"{"dependencies": {}}"#)
            .expect("package.json should write");
        fs::create_dir_all(temp.path().join("src")).expect("src dir should build");
        fs::write(
            temp.path().join("src/db.ts"),
            "import { initializeApp } from \"firebase/app\";\n\
             import { getFirestore, collection, addDoc } from \"firebase/firestore\";\n",
        )
        .expect("covered source should write");

        let wire = WirePlan::fixture();
        let (watch_roots_tx, watch_roots_rx) = tokio::sync::watch::channel(Vec::new());
        let manifest_loop = run_manifest_watch_loop(ManifestWatch {
            app_dir: temp.path(),
            wire: &wire,
            initial_surfaces: WireSurfaces::default(),
            initial_adapter: None,
            boot_auto_tenant: "demo".to_string(),
            watch_roots: &watch_roots_tx,
        });
        tokio::pin!(manifest_loop);
        let primed = tokio::time::timeout(Duration::from_millis(50), &mut manifest_loop).await;
        assert!(primed.is_err(), "the manifest watch loop must keep running");

        fs::write(
            temp.path().join("package.json"),
            r#"{"dependencies": {"firebase": "^11.0.0"}}"#,
        )
        .expect("package.json should gain firebase");

        // The scan passes (every import is covered), so the adoption rewires
        // the manifest at the provisioned drop-in...
        let manifest_path = temp.path().join("package.json");
        let wait_for_rewire = async {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
            loop {
                if let Ok(content) = fs::read_to_string(&manifest_path)
                    && content.contains("\"firebase\": \"file:./.nimbus/packages/firebase\"")
                {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "package.json should be rewired to the provisioned drop-in within 30s"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        tokio::select! {
            _ = &mut manifest_loop => panic!("the manifest watch loop must not exit"),
            _ = wait_for_rewire => {}
        }

        // ...and then installs it, exactly like boot.
        let installed_marker = temp.path().join("node_modules/firebase/package.json");
        let wait_for_install = async {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
            loop {
                if installed_marker.is_file() {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "npm should install the provisioned firebase package within 120s"
                );
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        };
        tokio::select! {
            _ = &mut manifest_loop => panic!("the manifest watch loop must not exit"),
            _ = wait_for_install => {}
        }

        assert!(
            temp.path()
                .join(".nimbus/packages/firebase/package.json")
                .is_file(),
            "the drop-in package payload must be provisioned"
        );
        assert!(
            watch_roots_rx.borrow().is_empty(),
            "a Firestore client app has no codegen sources to register"
        );
    }

    #[tokio::test]
    async fn mid_session_convex_adoption_registers_watch_roots() {
        // DXL2 Convex lane against both live loops: a `convex/` directory
        // created mid-session is adopted through the boot-time flow —
        // provision the embedded packages, install Node dependencies, then
        // register the source roots with the codegen watch loop, whose
        // baseline reset gives the adopted sources their initial codegen.
        // Nothing restarts at any point.
        if let Err(error) = crate::node::ensure_node22_runtime_available() {
            eprintln!(
                "skipping mid_session_convex_adoption_registers_watch_roots; Node.js baseline unavailable: {error}"
            );
            return;
        }
        if let Err(error) = crate::node::ensure_npm_available() {
            eprintln!(
                "skipping mid_session_convex_adoption_registers_watch_roots; npm unavailable: {error}"
            );
            return;
        }
        if nimbus_assets::js_packages::manifest().tooling.is_empty() {
            eprintln!(
                "skipping mid_session_convex_adoption_registers_watch_roots; embedded codegen \
                 tooling unavailable (run `make build-packages`)"
            );
            return;
        }

        // Codegen runs real Node tooling; keep the fixture on the repo's
        // filesystem like the codegen tests do, behind a `.git` boundary so
        // the app-dir walk-up stops inside the tempdir.
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("crate manifest dir should have repo root");
        let repo_target = repo_root.join("target");
        fs::create_dir_all(&repo_target).expect("repo target dir should exist");
        let temp =
            tempfile::tempdir_in(&repo_target).expect("tempdir in repo target should create");
        fs::create_dir_all(temp.path().join(".git")).expect(".git boundary should create");
        fs::write(temp.path().join("package.json"), r#"{"dependencies": {}}"#)
            .expect("package.json should write");

        let command = DevCommand {
            port: 0,
            app_dir: None,
            compose_file: Vec::new(),
            once: false,
            skip_codegen: false,
            debug_node_apis: false,
            tail_logs: DevTailLogsMode::PauseOnSync,
            data_dir: None,
            no_open: true,
        };
        let plan = resolve_dev_plan(command, temp.path()).expect("dev plan should resolve");
        // D8: no adapter at boot still resolves a serving plan — start gets
        // no app dir and the codegen watch starts with no roots.
        assert!(plan.adapter.is_none(), "the plan must start adapterless");
        assert!(
            plan.start_command.app_dir.is_none(),
            "an adapterless session passes start no app dir (D8)"
        );
        assert!(plan.initial_watch_roots().is_empty());

        let watch_plan = plan.watch_plan();
        let boot_auto_tenant = plan
            .start_command
            .auto_tenant
            .clone()
            .expect("dev plan sets an auto tenant");
        let (watch_roots_tx, watch_roots_rx) =
            tokio::sync::watch::channel(plan.initial_watch_roots());
        let probe_rx = watch_roots_tx.subscribe();
        let manifest_loop = run_manifest_watch_loop(ManifestWatch {
            app_dir: &plan.app_dir,
            wire: &plan.wire,
            initial_surfaces: plan.wire_surfaces,
            initial_adapter: plan.adapter.clone(),
            boot_auto_tenant,
            watch_roots: &watch_roots_tx,
        });
        let codegen_loop = run_dev_watch_loop(watch_plan, watch_roots_rx);
        let dev_loops = async {
            tokio::select! {
                result = manifest_loop => result,
                result = codegen_loop => result,
            }
        };
        tokio::pin!(dev_loops);
        // Prime both loops before the fixture lands: the codegen loop must
        // take its initial (empty) roots first, so the registration's
        // baseline reset is what earns the adopted sources their codegen.
        let primed = tokio::time::timeout(Duration::from_millis(50), &mut dev_loops).await;
        assert!(primed.is_err(), "the dev loops must keep running");

        let convex_dir = plan.app_dir.join("convex");
        fs::create_dir_all(&convex_dir).expect("convex source dir should create");
        fs::write(
            convex_dir.join("messages.ts"),
            r#"
import { query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async () => [],
});
"#,
        )
        .expect("convex source fixture should write");

        // Adoption → provision → install → root registration → initial
        // codegen, all while both loops keep running.
        let generated_api = plan.app_dir.join("convex/_generated/api.ts");
        let wait_for_codegen = async {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
            loop {
                if generated_api.is_file() {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "the adopted convex sources should earn their initial codegen within 180s"
                );
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        };
        tokio::select! {
            _ = &mut dev_loops => panic!("the dev loops must not exit"),
            _ = wait_for_codegen => {}
        }

        assert_eq!(
            *probe_rx.borrow(),
            vec![plan.app_dir.join("convex")],
            "adoption must register the convex source root with the codegen watch"
        );
        let manifest = fs::read_to_string(plan.app_dir.join("package.json"))
            .expect("package.json should read");
        assert!(
            manifest.contains("\"convex\": \"file:./.nimbus/packages/convex\""),
            "package.json must point convex at the provisioned copy: {manifest}"
        );
        assert!(
            plan.app_dir.join("node_modules/convex").exists(),
            "the provisioned convex package must be installed"
        );
        assert!(
            plan.app_dir.join(".nimbus/convex/functions.json").is_file(),
            "codegen must emit the functions manifest for the adopted sources"
        );
    }
}
