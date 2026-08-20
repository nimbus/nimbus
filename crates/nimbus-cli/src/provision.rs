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
//!
//! Adapter provisioning also wires the app's `package.json`: when the selected
//! adapter's root package (`convex`, `firebase`, …) appears in `dependencies`
//! with a registry spec — a migrated app — the spec is rewritten in place to
//! `file:./.nimbus/packages/<dir>`, and the dependency is added when absent.
//! That makes one command (or `nimbus dev` alone, for app dirs Nimbus already
//! recognizes) the whole migration: no manual `npm pkg set` step. The rewrite
//! preserves the order and raw formatting of every untouched `package.json`
//! entry.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::{fs, io};

use clap::{Args, Subcommand};
use serde::Deserialize;

use nimbus_assets::js_packages;

use crate::app_manifest;

const PACKAGES_REL: &str = ".nimbus/packages";
const STAMP_REL: &str = ".nimbus/packages/.version";

#[derive(Debug, Subcommand)]
pub(crate) enum PackagesCommand {
    /// Install embedded Nimbus JS packages into an app: stage them under
    /// `.nimbus/packages/`, point the app's `package.json` dependency at the
    /// staged copy, and run the Node dependency install.
    Install(InstallArgs),
    /// Undo `install` for one adapter: restore the dependency spec it replaced
    /// (or remove the dependency it added) and drop the staged packages.
    Uninstall(UninstallArgs),
    /// Verify provisioned package bytes against the binary's embedded checksums.
    Verify(VerifyArgs),
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct InstallArgs {
    /// What to install: `all`, or an adapter
    /// (`convex`|`firebase`|`mongodb`|`dynamodb`|`nimbus`); dependency closure
    /// is included automatically.
    #[arg(default_value = "all")]
    target: String,

    /// App directory to install into.
    #[arg(long, default_value = ".")]
    app_dir: PathBuf,

    /// Stage and wire only; skip the Node dependency install.
    #[arg(long)]
    no_node_install: bool,
}

#[derive(Debug, Args)]
#[command(help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct UninstallArgs {
    /// Which adapter to uninstall
    /// (`convex`|`firebase`|`mongodb`|`dynamodb`|`nimbus`).
    target: String,

    /// App directory to uninstall from.
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
        PackagesCommand::Install(args) => {
            let selection = Selection::parse(&args.target)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            let outcome = provision_packages(&args.app_dir, &selection)?;
            let where_ = args.app_dir.join(PACKAGES_REL);
            if outcome.changed {
                crate::cli_ux::write_stderr_line(&format!(
                    "Staged {} package(s) into {}: {}",
                    outcome.provisioned.len(),
                    where_.display(),
                    outcome.provisioned.join(", "),
                ))?;
            } else {
                crate::cli_ux::write_stderr_line(&format!(
                    "Packages already staged in {} (up to date)",
                    where_.display(),
                ))?;
            }

            let wired =
                wire_app_dependency_inner(&args.app_dir, &selection, DetachHandling::Clear)?;
            match &wired {
                Some((name, spec)) => {
                    crate::cli_ux::write_stderr_line(&format!(
                        "Wired \"{name}\": \"{spec}\" in package.json"
                    ))?;
                }
                None => {
                    if let Some((name, dir)) = adapter_root(&selection)
                        && !args.app_dir.join("package.json").is_file()
                    {
                        crate::cli_ux::write_stderr_line(&format!(
                            "No package.json here — add \"{name}\": \"{}\" to your app's \
                             dependencies, then run `npm install`",
                            provisioned_spec(&dir),
                        ))?;
                    }
                }
            }

            // `install` completes: staged bytes and a rewritten specifier are
            // inert until node_modules reflects them. Forcing the reinstall
            // first stops an already-installed registry copy from satisfying
            // the fingerprint and leaving the old package in place.
            if outcome.changed || wired.is_some() {
                force_node_reinstall(&args.app_dir)?;
            }
            install_node_dependencies(&args.app_dir, args.no_node_install).await?;
            Ok(())
        }
        PackagesCommand::Uninstall(args) => {
            let selection = Selection::parse(&args.target)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            if adapter_root(&selection).is_none() {
                return Err(Box::new(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "uninstall needs one adapter \
                     (convex|firebase|mongodb|dynamodb|nimbus), not `all`",
                )));
            }

            match detach_app_dependency(&args.app_dir, &selection)? {
                DetachOutcome::Restored { name, previous } => {
                    crate::cli_ux::write_stderr_line(&format!(
                        "Restored \"{name}\": \"{previous}\" in package.json"
                    ))?;
                }
                DetachOutcome::Removed { name } => {
                    crate::cli_ux::write_stderr_line(&format!(
                        "Removed \"{name}\" from package.json dependencies"
                    ))?;
                }
                DetachOutcome::NotWired => {
                    crate::cli_ux::write_stderr_line(&format!(
                        "Nothing to uninstall: {} is not wired to a staged package here",
                        args.target,
                    ))?;
                }
            }

            match remove_staged_packages(&args.app_dir)? {
                StagedRemoval::Removed => {
                    crate::cli_ux::write_stderr_line(&format!(
                        "Removed staged packages in {}",
                        args.app_dir.join(PACKAGES_REL).display(),
                    ))?;
                }
                StagedRemoval::StillInUse => {
                    crate::cli_ux::write_stderr_line(
                        "Kept staged packages: another dependency still points into \
                         .nimbus/packages/",
                    )?;
                }
                StagedRemoval::Absent => {}
            }

            force_node_reinstall(&args.app_dir)?;
            crate::cli_ux::write_stderr_line(
                "Run `npm install` to update node_modules. \
                 `nimbus packages install <target>` re-wires it.",
            )?;
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
                "unknown package target {other:?} \
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
/// can't go stale. For an adapter selection this also wires the app's
/// `package.json` dependency at the provisioned copy, so a registry-installed
/// app migrates the first time any app-scoped Nimbus command runs in it.
/// Returns whether anything changed. `init` (after scaffolding), `dev` (before
/// the install loop), `codegen`, and `deploy` call this so the supported
/// offline flow never needs a manual `nimbus packages install`.
pub(crate) fn ensure(app_dir: &Path, selection: &Selection) -> io::Result<bool> {
    let outcome = provision_packages(app_dir, selection)?;
    let wired = wire_app_dependency(app_dir, selection)?;
    if let Some((name, spec)) = &wired {
        crate::cli_ux::write_stderr_line(&format!("Wired \"{name}\": \"{spec}\" in package.json"))?;
    }
    if !outcome.changed && wired.is_none() {
        return Ok(false);
    }
    // The app's package.json/lockfile (`file:` specifiers) don't change when the
    // provisioned bytes do, so the Node dependency-install fingerprint would Skip
    // and keep stale copies. And a just-wired migrated app may have a registry
    // copy installed with no recorded fingerprint, which would RecordState
    // instead of installing. Force a reinstall in both cases (a no-op before
    // the first install).
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

/// The embedded root package an adapter selection wires into an app's
/// `package.json`, as `(npm name, staging dir)`. `Selection::All` has no single
/// root to wire.
fn adapter_root(selection: &Selection) -> Option<(String, String)> {
    let Selection::Adapter(start) = selection else {
        return None;
    };
    js_packages::manifest()
        .packages
        .iter()
        .find(|p| {
            p.dir == *start || p.name == *start || p.source_dir.as_deref() == Some(start.as_str())
        })
        .map(|p| (p.name.clone(), p.dir.clone()))
}

/// The `file:` specifier apps use to reference a provisioned package (BPD3).
pub(crate) fn provisioned_spec(dir: &str) -> String {
    format!("file:./{PACKAGES_REL}/{dir}")
}

/// Point the app's `package.json` dependency for the selected adapter's root
/// package at the provisioned copy — the step that turns a registry-installed
/// app (`"convex": "^1.x"`, `"firebase": "^11.x"`) into a migrated one without
/// a manual `npm pkg set`. A displaced registry spec is recorded so
/// `nimbus packages uninstall` can put it back, and a recorded detach is
/// honored: once a developer uninstalls a package, the automatic wiring that
/// runs on every `dev`, `codegen`, and `deploy` must not silently re-apply it.
/// Returns the `(name, spec)` that was written, or `None` when there is nothing
/// to do: `Selection::All`, no `package.json` in the app, a recorded detach, or
/// the dependency already carrying the provisioned spec (scaffolds).
pub(crate) fn wire_app_dependency(
    app_dir: &Path,
    selection: &Selection,
) -> io::Result<Option<(String, String)>> {
    wire_app_dependency_inner(app_dir, selection, DetachHandling::Honor)
}

/// Whether a recorded detach blocks wiring (automatic wiring) or is cleared by
/// it (the explicit `nimbus packages install`, which is the developer asking
/// for the wiring back).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetachHandling {
    Honor,
    Clear,
}

fn wire_app_dependency_inner(
    app_dir: &Path,
    selection: &Selection,
    detach: DetachHandling,
) -> io::Result<Option<(String, String)>> {
    let Some((name, dir)) = adapter_root(selection) else {
        return Ok(None);
    };
    let manifest_path = app_dir.join("package.json");
    let Some(text) = read_manifest(&manifest_path)? else {
        return Ok(None);
    };
    let invalid = |error: String| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "cannot wire \"{name}\" in {}: {error}",
                manifest_path.display()
            ),
        )
    };

    let record = app_manifest::wiring_record(&text, &name).map_err(invalid)?;
    if record == Some(app_manifest::WiringRecord::Detached) && detach == DetachHandling::Honor {
        return Ok(None);
    }

    let spec = provisioned_spec(&dir);
    let displaced = app_manifest::dependency_spec(&text, &name).map_err(invalid)?;
    let mut updated = match app_manifest::set_dependency(&text, &name, &spec).map_err(invalid)? {
        Some(rewritten) => rewritten,
        None if record != Some(app_manifest::WiringRecord::Detached) => return Ok(None),
        // Already carrying the provisioned spec, but still marked detached:
        // clearing the record is the whole edit.
        None => text.clone(),
    };

    // Remember the displaced registry spec, but never overwrite an existing
    // record — the first one holds the app's true pre-Nimbus state. When Nimbus
    // added the dependency itself there is nothing to restore, and the absence
    // of a record already means "remove it on uninstall".
    let wanted = match (&record, displaced) {
        (Some(app_manifest::WiringRecord::Restorable { .. }), _) => record.clone(),
        (_, Some(previous)) if previous != spec => {
            Some(app_manifest::WiringRecord::Restorable { previous })
        }
        _ => None,
    };
    if let Some(rewritten) =
        app_manifest::set_wiring_record(&updated, &name, wanted.as_ref()).map_err(invalid)?
    {
        updated = rewritten;
    }

    if updated == text {
        return Ok(None);
    }
    fs::write(&manifest_path, updated)?;
    Ok(Some((name, spec)))
}

/// What `uninstall` did to the app's `package.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DetachOutcome {
    /// The recorded pre-Nimbus spec was restored.
    Restored { name: String, previous: String },
    /// Nimbus had added the dependency, so it was removed.
    Removed { name: String },
    /// Nothing was wired: no `package.json`, or the dependency does not point
    /// at the provisioned copy.
    NotWired,
}

/// Undo `wire_app_dependency` for the selected adapter root: restore the
/// recorded registry spec (or drop the dependency Nimbus added), then record
/// the detach so automatic wiring does not re-apply it on the next command.
fn detach_app_dependency(app_dir: &Path, selection: &Selection) -> io::Result<DetachOutcome> {
    let Some((name, dir)) = adapter_root(selection) else {
        return Ok(DetachOutcome::NotWired);
    };
    let manifest_path = app_dir.join("package.json");
    let Some(text) = read_manifest(&manifest_path)? else {
        return Ok(DetachOutcome::NotWired);
    };
    let invalid = |error: String| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "cannot unwire \"{name}\" in {}: {error}",
                manifest_path.display()
            ),
        )
    };

    let spec = provisioned_spec(&dir);
    let current = app_manifest::dependency_spec(&text, &name).map_err(invalid)?;
    if current.as_deref() != Some(spec.as_str()) {
        return Ok(DetachOutcome::NotWired);
    }
    let record = app_manifest::wiring_record(&text, &name).map_err(invalid)?;

    let (mut updated, outcome) = match record {
        Some(app_manifest::WiringRecord::Restorable { previous }) => (
            app_manifest::set_dependency(&text, &name, &previous).map_err(invalid)?,
            DetachOutcome::Restored {
                name: name.clone(),
                previous,
            },
        ),
        _ => (
            app_manifest::remove_dependency(&text, &name).map_err(invalid)?,
            DetachOutcome::Removed { name: name.clone() },
        ),
    };
    let mut text_out = updated.take().unwrap_or_else(|| text.clone());
    if let Some(rewritten) = app_manifest::set_wiring_record(
        &text_out,
        &name,
        Some(&app_manifest::WiringRecord::Detached),
    )
    .map_err(invalid)?
    {
        text_out = rewritten;
    }
    fs::write(&manifest_path, text_out)?;
    Ok(outcome)
}

/// Read an app's `package.json`, treating an absent file as "nothing to edit".
fn read_manifest(manifest_path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(manifest_path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Complete an `install` by refreshing `node_modules`, or say plainly why it
/// did not. A missing `package.json` or npm is a normal state for an app that
/// only uses the wire-protocol adapters, so neither is an error here — the
/// staged bytes and the rewritten specifier are already correct on disk.
async fn install_node_dependencies(
    app_dir: &Path,
    skip: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if skip {
        crate::cli_ux::write_stderr_line(
            "Skipped the Node dependency install (--no-node-install); \
             run `npm install` to pick it up",
        )?;
        return Ok(());
    }
    if !app_dir.join("package.json").is_file() {
        return Ok(());
    }
    if let Err(error) = crate::node_runtime::ensure_npm_available() {
        crate::cli_ux::write_stderr_line(&format!(
            "Skipped the Node dependency install ({error}); \
             run `npm install` once npm is available"
        ))?;
        return Ok(());
    }
    crate::node_runtime::auto_install_node_dependencies(app_dir).await
}

/// Whether `uninstall` could drop the staged package tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StagedRemoval {
    Removed,
    /// Another adapter's dependency still resolves through `.nimbus/packages/`,
    /// so the shared closure has to stay.
    StillInUse,
    Absent,
}

/// Remove `<app>/.nimbus/packages/` unless a dependency still points into it.
/// The staged tree is a dependency closure shared by every wired adapter, so
/// uninstalling one of two wired adapters must not delete the other's copy.
fn remove_staged_packages(app_dir: &Path) -> io::Result<StagedRemoval> {
    let root = app_dir.join(PACKAGES_REL);
    if !root.exists() {
        return Ok(StagedRemoval::Absent);
    }
    if let Some(text) = read_manifest(&app_dir.join("package.json"))?
        && app_manifest::has_dependency_with_prefix(&text, &format!("file:./{PACKAGES_REL}/"))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
    {
        return Ok(StagedRemoval::StillInUse);
    }
    fs::remove_dir_all(&root)?;
    Ok(StagedRemoval::Removed)
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
    crate::node_runtime::clear_node_dependency_state(app_dir)
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
            let digest = nimbus_assets::js_packages::sha256_hex(&bytes);
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
        // manual `nimbus packages install`.
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
    fn wiring_rewires_registry_spec_and_records_what_it_displaced() {
        let app = tempfile::tempdir().unwrap();
        fs::write(
            app.path().join("package.json"),
            concat!(
                "{\n",
                "  \"name\": \"my-app\",\n",
                "  \"private\": true,\n",
                "  \"dependencies\": {\n",
                "    \"convex\": \"^1.17.0\",\n",
                "    \"react\": \"^19.0.0\"\n",
                "  },\n",
                "  \"devDependencies\": {\n",
                "    \"typescript\": \"~5.5.0\"\n",
                "  }\n",
                "}\n",
            ),
        )
        .unwrap();

        let wired = wire_app_dependency(app.path(), &Selection::Adapter("convex".into()))
            .unwrap()
            .expect("registry spec must be rewired");
        assert_eq!(
            wired,
            (
                "convex".to_string(),
                "file:./.nimbus/packages/convex".to_string()
            )
        );
        let rewritten = fs::read_to_string(app.path().join("package.json")).unwrap();
        assert_eq!(
            rewritten,
            concat!(
                "{\n",
                "  \"name\": \"my-app\",\n",
                "  \"private\": true,\n",
                "  \"dependencies\": {\n",
                "    \"convex\": \"file:./.nimbus/packages/convex\",\n",
                "    \"react\": \"^19.0.0\"\n",
                "  },\n",
                "  \"devDependencies\": {\n",
                "    \"typescript\": \"~5.5.0\"\n",
                "  },\n",
                "  \"nimbus\": {\n",
                "    \"packages\": {\n",
                "      \"convex\": { \"previous\": \"^1.17.0\" }\n",
                "    }\n",
                "  }\n",
                "}\n",
            ),
            "only the convex spec may change, plus the record of the spec it \
             displaced; order and formatting must hold"
        );
    }

    #[test]
    fn wiring_is_idempotent_once_spec_matches() {
        let app = tempfile::tempdir().unwrap();
        fs::write(
            app.path().join("package.json"),
            "{\n  \"dependencies\": {\n    \"convex\": \"^1.17.0\"\n  }\n}\n",
        )
        .unwrap();
        wire_app_dependency(app.path(), &Selection::Adapter("convex".into()))
            .unwrap()
            .expect("first call wires");
        let after_first = fs::read_to_string(app.path().join("package.json")).unwrap();

        let second = wire_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();
        assert!(second.is_none(), "already-wired spec must be a no-op");
        assert_eq!(
            fs::read_to_string(app.path().join("package.json")).unwrap(),
            after_first,
            "no-op must not rewrite the file"
        );
    }

    #[test]
    fn wiring_inserts_missing_dependency_at_sorted_position() {
        let app = tempfile::tempdir().unwrap();
        fs::write(
            app.path().join("package.json"),
            concat!(
                "{\n",
                "  \"dependencies\": {\n",
                "    \"@aws-sdk/client-dynamodb\": \"^3.0.0\",\n",
                "    \"react\": \"^19.0.0\"\n",
                "  }\n",
                "}\n",
            ),
        )
        .unwrap();

        wire_app_dependency(app.path(), &Selection::Adapter("firebase".into()))
            .unwrap()
            .expect("missing dependency must be inserted");
        let rewritten = fs::read_to_string(app.path().join("package.json")).unwrap();
        assert_eq!(
            rewritten,
            concat!(
                "{\n",
                "  \"dependencies\": {\n",
                "    \"@aws-sdk/client-dynamodb\": \"^3.0.0\",\n",
                "    \"firebase\": \"file:./.nimbus/packages/firebase\",\n",
                "    \"react\": \"^19.0.0\"\n",
                "  }\n",
                "}\n",
            ),
            "insert must land at the npm-sorted position"
        );
    }

    #[test]
    fn wiring_creates_dependencies_object_when_absent() {
        let app = tempfile::tempdir().unwrap();
        fs::write(
            app.path().join("package.json"),
            "{\n  \"name\": \"plain\",\n  \"private\": true\n}\n",
        )
        .unwrap();

        wire_app_dependency(app.path(), &Selection::Adapter("firebase".into()))
            .unwrap()
            .expect("missing dependencies object must be created");
        let rewritten = fs::read_to_string(app.path().join("package.json")).unwrap();
        assert_eq!(
            rewritten,
            concat!(
                "{\n",
                "  \"name\": \"plain\",\n",
                "  \"private\": true,\n",
                "  \"dependencies\": {\n",
                "    \"firebase\": \"file:./.nimbus/packages/firebase\"\n",
                "  }\n",
                "}\n",
            ),
        );
    }

    #[test]
    fn wiring_skips_without_package_json_and_for_all_selection() {
        let app = tempfile::tempdir().unwrap();
        // No package.json: nothing to wire, nothing created.
        assert!(
            wire_app_dependency(app.path(), &Selection::Adapter("convex".into()))
                .unwrap()
                .is_none()
        );
        assert!(!app.path().join("package.json").exists());

        // Selection::All has no single root to wire — even a registry spec stays.
        let before = "{\n  \"dependencies\": {\n    \"convex\": \"^1.17.0\"\n  }\n}\n";
        fs::write(app.path().join("package.json"), before).unwrap();
        assert!(
            wire_app_dependency(app.path(), &Selection::All)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            fs::read_to_string(app.path().join("package.json")).unwrap(),
            before
        );
    }

    #[test]
    fn ensure_wires_migrated_registry_dependency_and_forces_reinstall() {
        let app = tempfile::tempdir().unwrap();
        // Payload already current — the only pending change is the migrated
        // app's registry spec and its stale installed copy.
        provision_packages(app.path(), &Selection::Adapter("convex".into())).unwrap();
        fs::write(
            app.path().join("package.json"),
            "{\n  \"dependencies\": {\n    \"convex\": \"^1.17.0\"\n  }\n}\n",
        )
        .unwrap();
        let installed = app.path().join("node_modules").join("convex");
        fs::create_dir_all(&installed).unwrap();
        fs::write(installed.join("package.json"), "{}").unwrap();

        assert!(
            ensure(app.path(), &Selection::Adapter("convex".into())).unwrap(),
            "wiring alone must report a change"
        );
        let text = fs::read_to_string(app.path().join("package.json")).unwrap();
        assert!(
            text.contains("\"convex\": \"file:./.nimbus/packages/convex\""),
            "registry spec must be repointed at the provisioned copy: {text}"
        );
        assert!(
            !installed.exists(),
            "stale registry copy must be removed so npm reinstalls the provisioned package"
        );
        assert!(
            !ensure(app.path(), &Selection::Adapter("convex".into())).unwrap(),
            "already-wired, already-provisioned app must be a no-op"
        );
    }

    /// The manifest of an app whose dependency Nimbus added itself stays free of
    /// a wiring record: there is no earlier spec to restore, and `uninstall`
    /// already treats an absent record as "remove the dependency".
    #[test]
    fn wiring_records_nothing_when_nimbus_adds_the_dependency() {
        let app = tempfile::tempdir().unwrap();
        fs::write(
            app.path().join("package.json"),
            "{\n  \"dependencies\": {\n    \"react\": \"^19.0.0\"\n  }\n}\n",
        )
        .unwrap();

        wire_app_dependency(app.path(), &Selection::Adapter("convex".into()))
            .unwrap()
            .expect("missing dependency must be added");
        let text = fs::read_to_string(app.path().join("package.json")).unwrap();
        assert!(
            !text.contains("\"nimbus\""),
            "no displaced spec means no record: {text}"
        );
        assert_eq!(
            app_manifest::wiring_record(&text, "convex").unwrap(),
            None,
            "an added dependency carries no record"
        );
    }

    /// A later re-wire must not overwrite the first record. Only the original
    /// spec restores the app to its pre-Nimbus state.
    #[test]
    fn wiring_keeps_the_first_recorded_spec() {
        let app = tempfile::tempdir().unwrap();
        let manifest = app.path().join("package.json");
        fs::write(
            &manifest,
            "{\n  \"dependencies\": {\n    \"convex\": \"^1.17.0\"\n  }\n}\n",
        )
        .unwrap();
        wire_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();

        // Simulate a hand edit that points the spec somewhere else, then re-wire.
        let text = fs::read_to_string(&manifest).unwrap();
        fs::write(
            &manifest,
            app_manifest::set_dependency(&text, "convex", "^1.19.0")
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        wire_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();

        assert_eq!(
            app_manifest::wiring_record(&fs::read_to_string(&manifest).unwrap(), "convex").unwrap(),
            Some(app_manifest::WiringRecord::Restorable {
                previous: "^1.17.0".to_string()
            }),
            "the first displaced spec is the one that restores the app"
        );
    }

    #[test]
    fn uninstall_restores_the_recorded_registry_spec() {
        let app = tempfile::tempdir().unwrap();
        let manifest = app.path().join("package.json");
        let original = concat!(
            "{\n",
            "  \"name\": \"my-app\",\n",
            "  \"dependencies\": {\n",
            "    \"convex\": \"^1.17.0\",\n",
            "    \"react\": \"^19.0.0\"\n",
            "  }\n",
            "}\n",
        );
        fs::write(&manifest, original).unwrap();
        wire_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();

        let outcome =
            detach_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();
        assert_eq!(
            outcome,
            DetachOutcome::Restored {
                name: "convex".to_string(),
                previous: "^1.17.0".to_string(),
            }
        );
        let text = fs::read_to_string(&manifest).unwrap();
        assert_eq!(
            app_manifest::dependency_spec(&text, "convex").unwrap(),
            Some("^1.17.0".to_string()),
            "uninstall must put the registry spec back, not merely drop the key"
        );
    }

    #[test]
    fn uninstall_removes_a_dependency_nimbus_added() {
        let app = tempfile::tempdir().unwrap();
        let manifest = app.path().join("package.json");
        fs::write(
            &manifest,
            "{\n  \"dependencies\": {\n    \"react\": \"^19.0.0\"\n  }\n}\n",
        )
        .unwrap();
        wire_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();

        let outcome =
            detach_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();
        assert_eq!(
            outcome,
            DetachOutcome::Removed {
                name: "convex".to_string()
            }
        );
        let text = fs::read_to_string(&manifest).unwrap();
        assert_eq!(
            app_manifest::dependency_spec(&text, "convex").unwrap(),
            None,
            "a dependency Nimbus added must not survive uninstall"
        );
        assert_eq!(
            app_manifest::dependency_spec(&text, "react").unwrap(),
            Some("^19.0.0".to_string()),
            "unrelated dependencies must be untouched"
        );
    }

    /// The wiring in `ensure` runs on every `dev`, `codegen`, and `deploy`, so
    /// without a recorded detach an uninstall would silently undo itself.
    #[test]
    fn uninstall_stops_automatic_rewiring_and_install_restores_it() {
        let app = tempfile::tempdir().unwrap();
        let manifest = app.path().join("package.json");
        fs::write(
            &manifest,
            "{\n  \"dependencies\": {\n    \"convex\": \"^1.17.0\"\n  }\n}\n",
        )
        .unwrap();
        wire_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();
        detach_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();
        let detached = fs::read_to_string(&manifest).unwrap();

        let rewired =
            wire_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();
        assert!(rewired.is_none(), "automatic wiring must honor the detach");
        assert_eq!(
            fs::read_to_string(&manifest).unwrap(),
            detached,
            "a honored detach must not rewrite the manifest"
        );

        let reinstalled = wire_app_dependency_inner(
            app.path(),
            &Selection::Adapter("convex".into()),
            DetachHandling::Clear,
        )
        .unwrap();
        assert!(
            reinstalled.is_some(),
            "`packages install` is the developer asking for the wiring back"
        );
        let text = fs::read_to_string(&manifest).unwrap();
        assert_eq!(
            app_manifest::dependency_spec(&text, "convex").unwrap(),
            Some(provisioned_spec("convex")),
        );
        assert_eq!(
            app_manifest::wiring_record(&text, "convex").unwrap(),
            Some(app_manifest::WiringRecord::Restorable {
                previous: "^1.17.0".to_string()
            }),
            "reinstalling re-records the spec so a later uninstall still restores it"
        );
    }

    #[test]
    fn uninstall_reports_nothing_to_do_when_not_wired() {
        let app = tempfile::tempdir().unwrap();
        fs::write(
            app.path().join("package.json"),
            "{\n  \"dependencies\": {\n    \"convex\": \"^1.17.0\"\n  }\n}\n",
        )
        .unwrap();
        assert_eq!(
            detach_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap(),
            DetachOutcome::NotWired,
            "a registry spec Nimbus never wired must be left alone"
        );
        assert_eq!(
            fs::read_to_string(app.path().join("package.json")).unwrap(),
            "{\n  \"dependencies\": {\n    \"convex\": \"^1.17.0\"\n  }\n}\n",
        );
    }

    /// The staged tree is one dependency closure shared by every wired adapter,
    /// so uninstalling one of two must not delete the other's package.
    #[test]
    fn uninstall_keeps_staged_packages_another_dependency_still_uses() {
        let app = tempfile::tempdir().unwrap();
        fs::write(app.path().join("package.json"), "{}\n").unwrap();
        provision_packages(app.path(), &Selection::All).unwrap();
        wire_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();
        wire_app_dependency(app.path(), &Selection::Adapter("firebase".into())).unwrap();

        detach_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();
        assert_eq!(
            remove_staged_packages(app.path()).unwrap(),
            StagedRemoval::StillInUse
        );
        assert!(
            app.path().join(PACKAGES_REL).join("firebase").is_dir(),
            "the still-wired adapter keeps its staged copy"
        );

        detach_app_dependency(app.path(), &Selection::Adapter("firebase".into())).unwrap();
        assert_eq!(
            remove_staged_packages(app.path()).unwrap(),
            StagedRemoval::Removed
        );
        assert!(
            !app.path().join(PACKAGES_REL).exists(),
            "the last uninstall drops the staged tree"
        );
    }

    /// `@nimbus/codegen` is wired as a dev dependency, and the install path
    /// resolves both sections. `uninstall` must judge the staged tree by the
    /// same rule, or it deletes the payload a dev-only spec still resolves
    /// through and leaves the app unable to install.
    #[test]
    fn uninstall_keeps_staged_packages_a_dev_dependency_still_uses() {
        let app = tempfile::tempdir().unwrap();
        fs::write(app.path().join("package.json"), "{}\n").unwrap();
        provision_packages(app.path(), &Selection::All).unwrap();
        wire_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();

        let manifest_path = app.path().join("package.json");
        let wired = fs::read_to_string(&manifest_path).unwrap();
        let with_dev = wired.replace(
            "\n}",
            ",\n  \"devDependencies\": {\n    \"@nimbus/codegen\": \"file:./.nimbus/packages/codegen\"\n  }\n}",
        );
        assert_ne!(with_dev, wired, "the fixture must actually add the section");
        fs::write(&manifest_path, &with_dev).unwrap();

        detach_app_dependency(app.path(), &Selection::Adapter("convex".into())).unwrap();
        assert_eq!(
            remove_staged_packages(app.path()).unwrap(),
            StagedRemoval::StillInUse
        );
        assert!(
            app.path().join(PACKAGES_REL).is_dir(),
            "a dev-only spec keeps the staged tree it resolves through"
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
