use std::path::{Path, PathBuf};

use nimbus::Error;

use super::{DEFAULT_GVPROXY_BINARY, GVPROXY_ENV, HELPER_BINARY_DIR_ENV};

/// Resolve the gvproxy user-mode network helper from the bundled `libexec`
/// copy or the `NIMBUS_MACHINE_GVPROXY` override. VMM binary resolution is
/// owned by each provider's [`MachineVmmBackend`](super::vmm::MachineVmmBackend)
/// instead.
pub(super) fn resolve_gvproxy_binary() -> Result<PathBuf, Error> {
    resolve_helper_binary(
        GVPROXY_ENV,
        DEFAULT_GVPROXY_BINARY,
        &bundled_helper_candidates(DEFAULT_GVPROXY_BINARY),
    )
}

/// Resolve a machine helper that the macOS release archive bundles in
/// `libexec`. Precedence: the per-helper env override, then the
/// `NIMBUS_MACHINE_HELPER_BINARY_DIR` override, then the bundled copy next to
/// the nimbus executable. There is no Homebrew, Podman, or `PATH` fallback: a
/// helper outside the pinned bundle runs only when an operator names it.
pub(super) fn resolve_helper_binary(
    env_name: &str,
    command_name: &str,
    bundled_candidates: &[PathBuf],
) -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os(env_name) {
        return resolve_existing_file(PathBuf::from(path), env_name);
    }
    if let Some(path) = helper_binary_dir_candidate(command_name) {
        return Ok(path);
    }
    for candidate in bundled_candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }
    let bundled_locations = if bundled_candidates.is_empty() {
        "<unknown: the nimbus executable path did not resolve>".to_owned()
    } else {
        bundled_candidates
            .iter()
            .map(|candidate| candidate.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    Err(Error::InvalidInput(format!(
        "required helper '{command_name}' was not found; set {env_name} (or {HELPER_BINARY_DIR_ENV}) to override it, or restore the bundled copy at {bundled_locations}"
    )))
}

fn helper_binary_dir_candidate(command_name: &str) -> Option<PathBuf> {
    let helper_dir = std::env::var_os(HELPER_BINARY_DIR_ENV)?;
    let candidate = PathBuf::from(helper_dir).join(command_name);
    candidate.is_file().then_some(candidate)
}

pub(super) fn bundled_helper_candidates(helper_name: &str) -> Vec<PathBuf> {
    let Ok(current_exe) = std::env::current_exe() else {
        return Vec::new();
    };

    let mut candidates = bundled_helper_candidates_for_executable(&current_exe, helper_name);
    if let Ok(canonical_exe) = current_exe.canonicalize() {
        for candidate in bundled_helper_candidates_for_executable(&canonical_exe, helper_name) {
            push_unique_path(&mut candidates, candidate);
        }
    }
    candidates
}

pub(super) fn bundled_helper_candidates_for_executable(
    executable_path: &Path,
    helper_name: &str,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let Some(executable_dir) = executable_path.parent() else {
        return candidates;
    };

    push_unique_path(
        &mut candidates,
        executable_dir.join("libexec").join(helper_name),
    );
    if executable_dir.file_name().and_then(|value| value.to_str()) == Some("bin")
        && let Some(prefix_dir) = executable_dir.parent()
    {
        push_unique_path(
            &mut candidates,
            prefix_dir.join("libexec").join(helper_name),
        );
    }
    candidates
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.contains(&candidate) {
        paths.push(candidate);
    }
}

fn resolve_existing_file(path: PathBuf, env_name: &str) -> Result<PathBuf, Error> {
    if path.is_file() {
        return Ok(path);
    }
    Err(Error::InvalidInput(format!(
        "{env_name} points to {}, but that file does not exist",
        path.display()
    )))
}
