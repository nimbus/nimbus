use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use tempfile::tempdir;

use crate::backends::v8::embedder::JsErrorBox;

use super::render::render_runtime_test_spawn_bundle_source;
use super::types::{RuntimeTestSpawnMode, RuntimeTestSpawnPlan};

fn copy_dir_recursive(source: &Path, destination: &Path) -> std::result::Result<(), JsErrorBox> {
    std::fs::create_dir_all(destination).map_err(|error| {
        JsErrorBox::generic(format!(
            "node_compat subprocess copy should create {}: {error}",
            destination.display()
        ))
    })?;

    for entry in std::fs::read_dir(source).map_err(|error| {
        JsErrorBox::generic(format!(
            "node_compat subprocess copy should read {}: {error}",
            source.display()
        ))
    })? {
        let entry = entry.map_err(|error| {
            JsErrorBox::generic(format!(
                "node_compat subprocess copy should enumerate {}: {error}",
                source.display()
            ))
        })?;
        let entry_path = entry.path();
        let target_path = destination.join(entry.file_name());
        let file_type = entry.file_type().map_err(|error| {
            JsErrorBox::generic(format!(
                "node_compat subprocess copy should stat {}: {error}",
                entry_path.display()
            ))
        })?;
        if file_type.is_dir() {
            copy_dir_recursive(&entry_path, &target_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&entry_path, &target_path).map_err(|error| {
                JsErrorBox::generic(format!(
                    "node_compat subprocess copy should copy {} -> {}: {error}",
                    entry_path.display(),
                    target_path.display()
                ))
            })?;
        }
    }

    Ok(())
}

pub(super) fn rewrite_bundle_string(value: &str, source_root: &Path, target_root: &Path) -> String {
    value.replace(
        source_root.to_string_lossy().as_ref(),
        target_root.to_string_lossy().as_ref(),
    )
}

pub(super) fn rewrite_bundle_path(
    candidate: &Path,
    source_root: &Path,
    target_root: &Path,
) -> PathBuf {
    let canonical_candidate =
        std::fs::canonicalize(candidate).unwrap_or_else(|_| candidate.to_path_buf());
    let canonical_source_root =
        std::fs::canonicalize(source_root).unwrap_or_else(|_| source_root.to_path_buf());
    if let Ok(relative) = canonical_candidate.strip_prefix(&canonical_source_root) {
        return target_root.join(relative);
    }
    PathBuf::from(rewrite_bundle_string(
        candidate.to_string_lossy().as_ref(),
        source_root,
        target_root,
    ))
}

pub(super) fn rewrite_bundle_env(
    env: &BTreeMap<String, String>,
    source_root: &Path,
    target_root: &Path,
) -> BTreeMap<String, String> {
    env.iter()
        .map(|(key, value)| {
            (
                key.clone(),
                rewrite_bundle_string(value, source_root, target_root),
            )
        })
        .collect()
}

fn rewrite_bundle_command(command: &str, source_root: &Path, target_root: &Path) -> String {
    let command_path = Path::new(command);
    if command_path.is_absolute() {
        rewrite_bundle_path(command_path, source_root, target_root)
            .to_string_lossy()
            .into_owned()
    } else {
        rewrite_bundle_string(command, source_root, target_root)
    }
}

fn runtime_test_spawn_file_output_syncs(
    plan: &RuntimeTestSpawnPlan,
    bundle_root: &Path,
) -> Vec<RuntimeTestSpawnFileOutputSync> {
    let RuntimeTestSpawnMode::TestRunner {
        reporter_destinations,
        rerun_failures_file,
        ..
    } = &plan.mode
    else {
        return Vec::new();
    };
    let Some(source_bundle_root) = plan.source_bundle_root.as_deref() else {
        return Vec::new();
    };
    if plan.permission_restricted {
        return Vec::new();
    }

    let mut syncs = reporter_destinations
        .iter()
        .filter(|destination| destination.as_str() != "stdout" && destination.as_str() != "stderr")
        .filter_map(|destination| {
            let original = PathBuf::from(destination);
            if !original.is_absolute() {
                return None;
            }
            Some((
                original.clone(),
                rewrite_bundle_path(&original, source_bundle_root, bundle_root),
            ))
        })
        .collect::<Vec<_>>();

    if let Some(rerun_failures_file) = rerun_failures_file {
        let original = PathBuf::from(rerun_failures_file);
        if original.is_absolute() {
            syncs.push((
                original.clone(),
                rewrite_bundle_path(&original, source_bundle_root, bundle_root),
            ));
        }
    }

    syncs
}

pub(super) type RuntimeTestSpawnFileOutputSync = (PathBuf, PathBuf);
pub(super) type RuntimeTestSpawnBundle = (
    tempfile::TempDir,
    PathBuf,
    Vec<RuntimeTestSpawnFileOutputSync>,
);

pub(super) fn sync_runtime_test_spawn_file_outputs(
    syncs: &[RuntimeTestSpawnFileOutputSync],
) -> std::result::Result<(), JsErrorBox> {
    for (original_path, rewritten_path) in syncs {
        if !rewritten_path.exists() {
            continue;
        }
        if let Some(parent) = original_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                JsErrorBox::generic(format!(
                    "node_compat subprocess output sync should create {}: {error}",
                    parent.display()
                ))
            })?;
        }
        std::fs::copy(rewritten_path, original_path).map_err(|error| {
            JsErrorBox::generic(format!(
                "node_compat subprocess output sync should copy {} -> {}: {error}",
                rewritten_path.display(),
                original_path.display()
            ))
        })?;
    }
    Ok(())
}

pub(super) fn write_runtime_test_spawn_bundle(
    plan: &RuntimeTestSpawnPlan,
) -> std::result::Result<RuntimeTestSpawnBundle, JsErrorBox> {
    let tempdir = tempdir().map_err(|error| {
        JsErrorBox::generic(format!("node_compat tempdir should build: {error}"))
    })?;
    let bundle_dir = tempdir.path().join("app/.nimbus/convex");
    std::fs::create_dir_all(&bundle_dir).map_err(|error| {
        JsErrorBox::generic(format!(
            "node_compat spawn bundle dir should build: {error}"
        ))
    })?;
    let bundle_dir = std::fs::canonicalize(&bundle_dir).unwrap_or(bundle_dir);
    let file_output_syncs = runtime_test_spawn_file_output_syncs(plan, &bundle_dir);
    let rendered_command = if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
        if plan.permission_restricted {
            plan.command.clone()
        } else {
            rewrite_bundle_command(&plan.command, source_bundle_root, &bundle_dir)
        }
    } else {
        plan.command.clone()
    };

    if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
        copy_dir_recursive(source_bundle_root, &bundle_dir)?;
    }

    if let Some(cwd) = plan.cwd.as_deref() {
        let cwd_node_modules = cwd.join("node_modules");
        let bundle_node_modules = bundle_dir.join("node_modules");
        if cwd_node_modules.is_dir() && !bundle_node_modules.exists() {
            // Node's eval children resolve bare packages from the caller's cwd.
            // The compat subprocess bundle executes from its own temp root, so
            // mirror the caller's node_modules tree when present to preserve
            // that package-resolution contract.
            copy_dir_recursive(&cwd_node_modules, &bundle_node_modules)?;
        }
    }

    let bundle_source =
        render_runtime_test_spawn_bundle_source(plan, &bundle_dir, &rendered_command)?;

    let bundle_path = bundle_dir.join("bundle.mjs");
    std::fs::write(&bundle_path, bundle_source).map_err(|error| {
        JsErrorBox::generic(format!(
            "node_compat subprocess bundle should write: {error}"
        ))
    })?;

    Ok((tempdir, bundle_path, file_output_syncs))
}
