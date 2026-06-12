//! Provision embedded JS package payloads into a developer app (BPD2).
//!
//! Materializes the binary's embedded, dependency-closed package payloads into
//! `<app>/.nimbus/packages/<dir>/` with a `.version` stamp (the embedded
//! manifest digest), idempotently and atomically. Scaffolds reference these via
//! `file:./.nimbus/packages/<dir>` specifiers (BPD3). `ensure` provisions when
//! the payload is absent (fresh `init`, or a clone where `.nimbus/` is
//! gitignored) and re-provisions when the stamp drifts from the binary's
//! embedded payload (after a binary upgrade); `init`, `dev`, `codegen`, and
//! `deploy` call it so a changed payload also forces a fresh Node dependency
//! install (BPD5).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::{fs, io};

use clap::{Args, Subcommand};
use serde::Deserialize;

use nimbus_assets::js_packages;

const PACKAGES_REL: &str = ".nimbus/packages";
const STAMP_REL: &str = ".nimbus/packages/.version";

#[derive(Debug, Subcommand)]
pub(crate) enum PackagesCommand {
    /// Provision embedded Nimbus JS packages into an app's `.nimbus/packages/`.
    Provision(ProvisionArgs),
    /// Verify provisioned package bytes against the binary's embedded checksums.
    Verify(VerifyArgs),
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct ProvisionArgs {
    /// What to provision: `all`, or an adapter
    /// (`convex`|`firebase`|`mongodb`|`dynamodb`|`nimbus`); dependency closure
    /// is included automatically.
    #[arg(default_value = "all")]
    target: String,

    /// App directory to provision into.
    #[arg(long, default_value = ".")]
    app_dir: PathBuf,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct VerifyArgs {
    /// App directory whose `.nimbus/packages/` is verified.
    #[arg(long, default_value = ".")]
    app_dir: PathBuf,
}

pub(crate) async fn run_packages_command(
    command: PackagesCommand,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        PackagesCommand::Provision(args) => {
            let selection = Selection::parse(&args.target)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            let outcome = provision_packages(&args.app_dir, &selection)?;
            let where_ = args.app_dir.join(PACKAGES_REL);
            if outcome.changed {
                crate::cli_ux::write_stderr_line(&format!(
                    "Provisioned {} package(s) into {}: {}",
                    outcome.provisioned.len(),
                    where_.display(),
                    outcome.provisioned.join(", "),
                ))?;
            } else {
                crate::cli_ux::write_stderr_line(&format!(
                    "Packages already provisioned in {} (up to date)",
                    where_.display(),
                ))?;
            }
            Ok(())
        }
        PackagesCommand::Verify(args) => {
            let where_ = args.app_dir.join(PACKAGES_REL);
            // First confirm the binary's own embedded payload is intact, then
            // confirm the app's provisioned bytes match it.
            js_packages::verify_manifest_integrity().map_err(io::Error::other)?;
            match verify_provisioned(&args.app_dir) {
                Ok(count) => {
                    crate::cli_ux::write_stderr_line(&format!(
                        "Verified {count} provisioned file(s) in {} against embedded checksums",
                        where_.display(),
                    ))?;
                    Ok(())
                }
                Err(error) => Err(Box::new(io::Error::other(error))),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Selection {
    All,
    Adapter(String),
}

impl Selection {
    pub(crate) fn parse(target: &str) -> Result<Self, String> {
        match target {
            "all" => Ok(Selection::All),
            "convex" | "firebase" | "mongodb" | "dynamodb" | "nimbus" => {
                Ok(Selection::Adapter(target.to_string()))
            }
            other => Err(format!(
                "unknown provision target {other:?} \
                 (expected: all|convex|firebase|mongodb|dynamodb|nimbus)"
            )),
        }
    }
}

pub(crate) struct ProvisionOutcome {
    pub provisioned: Vec<String>,
    pub changed: bool,
}

#[derive(Deserialize)]
struct PkgDeps {
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
}

/// `dependencies` names declared by an embedded package (read from its embedded
/// `package.json`). Used to resolve the provisioning closure.
fn embedded_deps(dir: &str) -> BTreeSet<String> {
    match js_packages::file_bytes(dir, "package.json") {
        Some(bytes) => serde_json::from_slice::<PkgDeps>(&bytes)
            .map(|p| p.dependencies.into_keys().collect())
            .unwrap_or_default(),
        None => BTreeSet::new(),
    }
}

/// Transitive closure of staging dirs to provision for `selection`. For an
/// adapter, this pulls in every embedded root it (transitively) depends on
/// (e.g. `firebase` pulls the three co-provisioned third-party roots).
fn closure(selection: &Selection) -> Vec<String> {
    let manifest = js_packages::manifest();
    match selection {
        Selection::All => manifest.packages.iter().map(|p| p.dir.clone()).collect(),
        Selection::Adapter(start) => {
            let start_dir = manifest
                .packages
                .iter()
                .find(|p| {
                    p.dir == *start
                        || p.name == *start
                        || p.source_dir.as_deref() == Some(start.as_str())
                })
                .map(|p| p.dir.clone())
                .unwrap_or_else(|| start.clone());
            let name_to_dir: BTreeMap<String, String> = manifest
                .packages
                .iter()
                .map(|p| (p.name.clone(), p.dir.clone()))
                .collect();
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut queue: VecDeque<String> = VecDeque::from([start_dir]);
            while let Some(dir) = queue.pop_front() {
                if !seen.insert(dir.clone()) {
                    continue;
                }
                for dep in embedded_deps(&dir) {
                    if let Some(dep_dir) = name_to_dir.get(&dep) {
                        queue.push_back(dep_dir.clone());
                    }
                }
            }
            seen.into_iter().collect()
        }
    }
}

fn stamp_path(app_dir: &Path) -> PathBuf {
    app_dir.join(STAMP_REL)
}

fn read_stamp(app_dir: &Path) -> Option<String> {
    fs::read_to_string(stamp_path(app_dir))
        .ok()
        .map(|s| s.trim().to_string())
}

fn remove_stamp_if_present(app_dir: &Path) -> io::Result<()> {
    match fs::remove_file(stamp_path(app_dir)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Write one embedded package's files into `<app>/.nimbus/packages/<dir>/`,
/// cleaning any prior copy first so a partial/corrupt copy is fully rewritten.
fn write_package(app_dir: &Path, dir: &str) -> io::Result<()> {
    let dest_root = app_dir.join(PACKAGES_REL).join(dir);
    if dest_root.exists() {
        fs::remove_dir_all(&dest_root)?;
    }
    for file in js_packages::package_files(dir).map_err(io::Error::other)? {
        let dest = dest_root.join(&file.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, file.bytes)?;
    }
    Ok(())
}

/// Provision the selected package closure into `<app>/.nimbus/packages/`.
/// Idempotent (a no-op when the stamp already matches and every closure dir is
/// present) and atomic at the completion boundary: the `.version` stamp is
/// written only after every file, so an interrupted run leaves a stale/absent
/// stamp and the next call rewrites.
pub(crate) fn provision_packages(
    app_dir: &Path,
    selection: &Selection,
) -> io::Result<ProvisionOutcome> {
    let dirs = closure(selection);
    let want_stamp = js_packages::manifest_digest();
    let stamp_matches = read_stamp(app_dir).as_deref() == Some(want_stamp.as_str());

    if stamp_matches && verify_package_dirs(app_dir, &dirs).is_ok() {
        return Ok(ProvisionOutcome {
            provisioned: dirs,
            changed: false,
        });
    }

    fs::create_dir_all(app_dir.join(PACKAGES_REL))?;
    remove_stamp_if_present(app_dir)?;
    for dir in &dirs {
        write_package(app_dir, dir)?;
    }
    fs::write(stamp_path(app_dir), format!("{want_stamp}\n"))?;
    Ok(ProvisionOutcome {
        provisioned: dirs,
        changed: true,
    })
}

/// Ensure `<app>/.nimbus/packages/` is provisioned for `selection` and current
/// with this binary. Provisions when absent (a fresh `init`, or a clone where
/// `.nimbus/` is gitignored and therefore not committed) and re-provisions on
/// binary-version drift, forcing a Node dependency reinstall so installed copies
/// can't go stale. Returns whether anything changed. `init` (after scaffolding),
/// `dev` (before the install loop), `codegen`, and `deploy` call this so the
/// supported offline flow never needs a manual `nimbus packages provision`.
pub(crate) fn ensure(app_dir: &Path, selection: &Selection) -> io::Result<bool> {
    let outcome = provision_packages(app_dir, selection)?;
    if !outcome.changed {
        return Ok(false);
    }
    // The app's package.json/lockfile (`file:` specifiers) don't change when the
    // provisioned bytes do, so the Node dependency-install fingerprint would Skip
    // and keep stale copies. Force a reinstall (a no-op before the first install).
    force_node_reinstall(app_dir)?;
    Ok(true)
}

pub(crate) fn ensure_known_app_packages(app_dir: &Path) -> io::Result<bool> {
    let Some(selection) = selection_for_app_dir(app_dir) else {
        return Ok(false);
    };
    ensure(app_dir, &selection)
}

fn selection_for_app_dir(app_dir: &Path) -> Option<Selection> {
    if app_dir.join("convex").is_dir() {
        return Some(Selection::Adapter("convex".to_string()));
    }
    if app_dir.join("nimbus").is_dir() {
        return Some(Selection::Adapter("nimbus".to_string()));
    }
    None
}

/// Drop the installed copies of the provisioned packages and clear the Node
/// dependency-install fingerprint so the next `auto_install_node_dependencies`
/// refreshes them from the freshly re-provisioned `.nimbus/packages/` — no stale
/// node_modules after a binary upgrade (BPD5, cond 26).
fn force_node_reinstall(app_dir: &Path) -> io::Result<()> {
    let manifest = js_packages::manifest();
    let node_modules = app_dir.join("node_modules");
    for pkg in &manifest.packages {
        let installed = node_modules.join(&pkg.name);
        if installed.exists() {
            fs::remove_dir_all(&installed)?;
        }
    }
    crate::node::clear_node_dependency_state(app_dir)
}

/// Verify every provisioned file under `<app>/.nimbus/packages/` matches the
/// binary's embedded manifest checksum. Returns the number of files verified, or
/// an error naming the first missing or tampered file. This proves the bytes on
/// disk are exactly the binary-owned bytes (BPD7, cond 21) — independent of the
/// embedded-side `js_packages::verify_manifest_integrity`, which checks the bytes
/// compiled into the binary rather than what was written into the app.
pub(crate) fn verify_provisioned(app_dir: &Path) -> Result<usize, String> {
    let manifest = js_packages::manifest();
    let root = app_dir.join(PACKAGES_REL);
    if !root.is_dir() {
        return Err(format!(
            "no provisioned package directory found at {}",
            root.display()
        ));
    }
    let dirs: Vec<String> = manifest
        .packages
        .iter()
        // An app may have provisioned only a subset (e.g. `provision firebase`).
        // Skip packages whose directory is entirely absent; a package that is
        // present but missing files still fails below.
        .filter(|pkg| root.join(&pkg.dir).exists())
        .map(|pkg| pkg.dir.clone())
        .collect();
    verify_package_dirs(app_dir, &dirs)
}

fn verify_package_dirs(app_dir: &Path, dirs: &[String]) -> Result<usize, String> {
    let manifest = js_packages::manifest();
    let root = app_dir.join(PACKAGES_REL);
    if dirs.is_empty() {
        return Err(format!(
            "no provisioned package files found in {}",
            root.display()
        ));
    }
    let mut verified = 0usize;
    for dir in dirs {
        let pkg = manifest
            .packages
            .iter()
            .find(|pkg| pkg.dir == *dir)
            .ok_or_else(|| format!("unknown embedded package directory {dir}"))?;
        let package_root = root.join(&pkg.dir);
        if !package_root.is_dir() {
            return Err(format!("missing provisioned package directory {}", pkg.dir));
        }
        for file in &pkg.files {
            let path = package_root.join(&file.path);
            let bytes = fs::read(&path)
                .map_err(|e| format!("missing provisioned file {}/{}: {e}", pkg.dir, file.path))?;
            let digest = nimbus_assets::integrity::sha256_hex(&bytes);
            if digest != file.sha256 {
                return Err(format!(
                    "checksum mismatch for provisioned {}/{}: expected {}, got {}",
                    pkg.dir, file.path, file.sha256, digest
                ));
            }
            verified += 1;
        }
    }
    if verified == 0 {
        return Err(format!(
            "no provisioned package files found in {}",
            root.display()
        ));
    }
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provision_all_writes_every_package_and_stamp() {
        let app = tempfile::tempdir().unwrap();
        let outcome = provision_packages(app.path(), &Selection::All).unwrap();
        assert!(outcome.changed);
        for pkg in js_packages::manifest().packages {
            assert!(
                app.path()
                    .join(PACKAGES_REL)
                    .join(&pkg.dir)
                    .join("package.json")
                    .is_file(),
                "missing provisioned package {}",
                pkg.dir
            );
        }
        assert_eq!(
            read_stamp(app.path()).as_deref(),
            Some(js_packages::manifest_digest().as_str())
        );
    }

    #[test]
    fn provision_is_idempotent() {
        let app = tempfile::tempdir().unwrap();
        provision_packages(app.path(), &Selection::All).unwrap();
        let second = provision_packages(app.path(), &Selection::All).unwrap();
        assert!(
            !second.changed,
            "second provision with the same binary must be a no-op"
        );
    }

    #[test]
    fn adapter_provision_includes_dependency_closure() {
        let app = tempfile::tempdir().unwrap();
        provision_packages(app.path(), &Selection::Adapter("firebase".into())).unwrap();
        let root = app.path().join(PACKAGES_REL);
        for dir in [
            "firebase",
            "@bufbuild/protobuf",
            "@connectrpc/connect",
            "@connectrpc/connect-web",
        ] {
            assert!(
                root.join(dir).join("package.json").is_file(),
                "closure missing {dir}"
            );
        }
        assert!(
            !root.join("@nimbus/mongodb").exists(),
            "unrelated adapter must not be provisioned"
        );
        assert!(
            !root.join("@nimbus/dynamodb").exists(),
            "unrelated adapter must not be provisioned"
        );
    }

    #[test]
    fn corrupt_partial_provision_is_rewritten() {
        let app = tempfile::tempdir().unwrap();
        provision_packages(app.path(), &Selection::All).unwrap();
        // Simulate a partial/corrupt copy: delete a provisioned file.
        let victim = app
            .path()
            .join(PACKAGES_REL)
            .join("@nimbus/mongodb")
            .join("package.json");
        fs::remove_file(&victim).unwrap();
        // With the file missing, all_present is false → provision rewrites.
        let outcome = provision_packages(app.path(), &Selection::All).unwrap();
        assert!(outcome.changed, "missing file must trigger a rewrite");
        assert!(victim.is_file(), "rewrite must restore the file");
    }

    #[test]
    fn corrupt_non_manifest_file_is_rewritten_even_when_stamp_matches() {
        let app = tempfile::tempdir().unwrap();
        provision_packages(app.path(), &Selection::All).unwrap();
        let victim = app
            .path()
            .join(PACKAGES_REL)
            .join("@nimbus/mongodb")
            .join("uri.js");
        let mut bytes = fs::read(&victim).unwrap();
        bytes.push(b'X');
        fs::write(&victim, bytes).unwrap();

        let outcome = provision_packages(app.path(), &Selection::All).unwrap();

        assert!(
            outcome.changed,
            "checksum drift with a matching stamp must trigger a rewrite"
        );
        let restored = fs::read(&victim).unwrap();
        let expected = js_packages::file_bytes("@nimbus/mongodb", "uri.js").unwrap();
        assert_eq!(restored, expected, "rewrite must restore original bytes");
    }

    #[test]
    fn ensure_reprovisions_on_stamp_drift_and_noops_on_match() {
        let app = tempfile::tempdir().unwrap();
        provision_packages(app.path(), &Selection::All).unwrap();
        // Simulate an upgraded binary: stale stamp.
        fs::write(stamp_path(app.path()), "stale-digest\n").unwrap();
        assert!(
            ensure(app.path(), &Selection::All).unwrap(),
            "stamp drift must re-provision"
        );
        assert_eq!(
            read_stamp(app.path()).as_deref(),
            Some(js_packages::manifest_digest().as_str())
        );
        assert!(
            !ensure(app.path(), &Selection::All).unwrap(),
            "matching stamp must be a no-op"
        );
    }

    #[test]
    fn ensure_on_drift_forces_node_reinstall() {
        let app = tempfile::tempdir().unwrap();
        provision_packages(app.path(), &Selection::All).unwrap();
        // Simulate a prior install: a provisioned package copied into
        // node_modules plus a recorded Node dependency-install fingerprint.
        let installed = app.path().join("node_modules").join("convex");
        fs::create_dir_all(&installed).unwrap();
        fs::write(installed.join("package.json"), "{}").unwrap();
        let state = app
            .path()
            .join(".nimbus")
            .join("cache")
            .join("node")
            .join("dependency-state.json");
        fs::create_dir_all(state.parent().unwrap()).unwrap();
        fs::write(&state, "{}").unwrap();
        // Simulate an upgraded binary: stale stamp forces a re-provision.
        fs::write(stamp_path(app.path()), "stale-digest\n").unwrap();

        assert!(
            ensure(app.path(), &Selection::All).unwrap(),
            "stamp drift must re-provision"
        );

        assert!(
            !installed.exists(),
            "stale node_modules copy must be removed so npm reinstalls from the refreshed source"
        );
        assert!(
            !state.exists(),
            "dependency-install fingerprint must be cleared so the next install re-evaluates"
        );
    }

    #[test]
    fn ensure_provisions_when_absent() {
        let app = tempfile::tempdir().unwrap();
        // A fresh `init`/clone has no `.nimbus/packages` — ensure must provision
        // the requested closure (not no-op), so the supported flow needs no
        // manual `nimbus packages provision`.
        assert!(
            ensure(app.path(), &Selection::Adapter("convex".into())).unwrap(),
            "absent packages must be provisioned"
        );
        assert!(app.path().join(PACKAGES_REL).join("convex").is_dir());
        assert!(
            app.path()
                .join(PACKAGES_REL)
                .join("@nimbus/nimbus")
                .is_dir(),
            "closure is provisioned"
        );
        // The convex closure does not pull unrelated adapters.
        assert!(!app.path().join(PACKAGES_REL).join("firebase").exists());
        // A second ensure with the matching stamp is a no-op.
        assert!(!ensure(app.path(), &Selection::Adapter("convex".into())).unwrap());
    }

    #[test]
    fn ensure_provisions_requested_closure_after_subset_stamp_match() {
        let app = tempfile::tempdir().unwrap();
        provision_packages(app.path(), &Selection::Adapter("firebase".into())).unwrap();
        assert_eq!(
            read_stamp(app.path()).as_deref(),
            Some(js_packages::manifest_digest().as_str())
        );
        assert!(
            !app.path().join(PACKAGES_REL).join("convex").exists(),
            "firebase-only provision should not pre-create convex"
        );

        assert!(
            ensure(app.path(), &Selection::Adapter("convex".into())).unwrap(),
            "matching global stamp is not enough; missing requested closure must provision"
        );
        assert!(app.path().join(PACKAGES_REL).join("convex").is_dir());
        assert!(
            app.path()
                .join(PACKAGES_REL)
                .join("@nimbus/nimbus")
                .is_dir()
        );
    }

    #[test]
    fn provisioned_bytes_verify_and_tamper_is_detected() {
        let app = tempfile::tempdir().unwrap();
        provision_packages(app.path(), &Selection::All).unwrap();
        // Positive: freshly provisioned bytes match the embedded manifest checksums.
        let verified =
            verify_provisioned(app.path()).expect("provisioned bytes match embedded checksums");
        assert!(verified > 0, "must verify at least one provisioned file");
        // Negative: tampering with a provisioned file on disk is detected and named.
        let victim = app
            .path()
            .join(PACKAGES_REL)
            .join("@nimbus/mongodb")
            .join("uri.js");
        let mut bytes = fs::read(&victim).unwrap();
        bytes.push(b'X');
        fs::write(&victim, &bytes).unwrap();
        let err = verify_provisioned(app.path())
            .expect_err("tampered provisioned bytes must be rejected");
        assert!(
            err.contains("@nimbus/mongodb/uri.js"),
            "error must name the tampered file: {err}"
        );
    }

    #[test]
    fn verify_provisioned_skips_unprovisioned_packages() {
        let app = tempfile::tempdir().unwrap();
        // Provision only the firebase closure — not convex/nimbus/mongodb/dynamodb.
        provision_packages(app.path(), &Selection::Adapter("firebase".into())).unwrap();
        // Verification covers the provisioned subset and must not fail on the
        // packages this app never provisioned.
        let verified = verify_provisioned(app.path()).expect("subset provision verifies");
        assert!(verified > 0, "must verify the firebase closure files");
        assert!(
            !app.path().join(PACKAGES_REL).join("convex").exists(),
            "convex was not provisioned for a firebase-only selection"
        );
    }

    #[test]
    fn verify_provisioned_rejects_missing_or_empty_payload() {
        let app = tempfile::tempdir().unwrap();
        let missing = verify_provisioned(app.path())
            .expect_err("missing .nimbus/packages must not verify successfully");
        assert!(
            missing.contains("no provisioned package directory"),
            "unexpected missing-dir error: {missing}"
        );

        fs::create_dir_all(app.path().join(PACKAGES_REL)).unwrap();
        let empty = verify_provisioned(app.path()).expect_err("empty package root must not verify");
        assert!(
            empty.contains("no provisioned package files"),
            "unexpected empty-root error: {empty}"
        );
    }
}
