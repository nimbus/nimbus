use std::path::{Path, PathBuf};

use nimbus::Error;

use super::{
    DEFAULT_GVPROXY_BINARY, DEFAULT_KRUNKIT_BINARY, GVPROXY_ENV, HELPER_BINARY_DIR_ENV,
    KRUNKIT_ENV, MachineHelperBinaryPaths, PODMAN_DARWIN_HELPER_DIRECTORIES,
};

impl MachineHelperBinaryPaths {
    pub(super) fn resolve() -> Result<Self, Error> {
        let bundled_gvproxy = bundled_helper_candidates(DEFAULT_GVPROXY_BINARY);
        let known_krunkit = known_helper_candidates(DEFAULT_KRUNKIT_BINARY);
        let known_gvproxy = known_helper_candidates(DEFAULT_GVPROXY_BINARY);
        Ok(Self {
            krunkit: resolve_helper_binary(
                KRUNKIT_ENV,
                DEFAULT_KRUNKIT_BINARY,
                &[],
                &known_krunkit,
            )?,
            gvproxy: resolve_helper_binary(
                GVPROXY_ENV,
                DEFAULT_GVPROXY_BINARY,
                &bundled_gvproxy,
                &known_gvproxy,
            )?,
        })
    }
}

pub(super) fn resolve_helper_binary(
    env_name: &str,
    command_name: &str,
    preferred_candidates: &[PathBuf],
    fallbacks: &[PathBuf],
) -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os(env_name) {
        return resolve_existing_file(PathBuf::from(path), env_name);
    }
    if let Some(path) = helper_binary_dir_candidate(command_name) {
        return Ok(path);
    }
    for candidate in preferred_candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }
    for fallback in fallbacks {
        if fallback.is_file() {
            return Ok(fallback.clone());
        }
    }
    Err(Error::InvalidInput(format!(
        "required helper '{command_name}' was not found; set {env_name}, set {HELPER_BINARY_DIR_ENV}, or install it in a supported packaged or Homebrew helper directory"
    )))
}

fn helper_binary_dir_candidate(command_name: &str) -> Option<PathBuf> {
    let helper_dir = std::env::var_os(HELPER_BINARY_DIR_ENV)?;
    let candidate = PathBuf::from(helper_dir).join(command_name);
    candidate.is_file().then_some(candidate)
}

pub(super) fn known_helper_candidates(helper_name: &str) -> Vec<PathBuf> {
    PODMAN_DARWIN_HELPER_DIRECTORIES
        .iter()
        .map(|directory| PathBuf::from(directory).join(helper_name))
        .collect()
}

fn bundled_helper_candidates(helper_name: &str) -> Vec<PathBuf> {
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

#[cfg(test)]
fn helper_env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
pub(crate) struct MachineHelperEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous_krunkit: Option<std::ffi::OsString>,
    previous_gvproxy: Option<std::ffi::OsString>,
    previous_helper_dir: Option<std::ffi::OsString>,
    previous_path: Option<std::ffi::OsString>,
}

#[cfg(test)]
impl MachineHelperEnvGuard {
    pub(crate) fn install_stub_binaries(dir: &Path) -> Self {
        let krunkit_path = dir.join("krunkit");
        let gvproxy_path = dir.join("gvproxy");
        write_helper_stub(&krunkit_path, "krunkit");
        write_helper_stub(&gvproxy_path, "gvproxy");
        Self::set_paths(&krunkit_path, &gvproxy_path)
    }

    pub(crate) fn set_paths(krunkit_path: &Path, gvproxy_path: &Path) -> Self {
        let lock = helper_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_krunkit = std::env::var_os(KRUNKIT_ENV);
        let previous_gvproxy = std::env::var_os(GVPROXY_ENV);
        let previous_helper_dir = std::env::var_os(HELPER_BINARY_DIR_ENV);
        let previous_path = std::env::var_os("PATH");
        unsafe {
            std::env::set_var(KRUNKIT_ENV, krunkit_path);
            std::env::set_var(GVPROXY_ENV, gvproxy_path);
        }
        Self {
            _lock: lock,
            previous_krunkit,
            previous_gvproxy,
            previous_helper_dir,
            previous_path,
        }
    }

    pub(crate) fn with_helper_binary_dir(dir: &Path) -> Self {
        let lock = helper_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_krunkit = std::env::var_os(KRUNKIT_ENV);
        let previous_gvproxy = std::env::var_os(GVPROXY_ENV);
        let previous_helper_dir = std::env::var_os(HELPER_BINARY_DIR_ENV);
        let previous_path = std::env::var_os("PATH");
        unsafe {
            std::env::remove_var(KRUNKIT_ENV);
            std::env::remove_var(GVPROXY_ENV);
            std::env::set_var(HELPER_BINARY_DIR_ENV, dir);
        }
        Self {
            _lock: lock,
            previous_krunkit,
            previous_gvproxy,
            previous_helper_dir,
            previous_path,
        }
    }

    pub(crate) fn with_path_only(dir: &Path) -> Self {
        let lock = helper_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_krunkit = std::env::var_os(KRUNKIT_ENV);
        let previous_gvproxy = std::env::var_os(GVPROXY_ENV);
        let previous_helper_dir = std::env::var_os(HELPER_BINARY_DIR_ENV);
        let previous_path = std::env::var_os("PATH");
        unsafe {
            std::env::remove_var(KRUNKIT_ENV);
            std::env::remove_var(GVPROXY_ENV);
            std::env::remove_var(HELPER_BINARY_DIR_ENV);
            std::env::set_var("PATH", dir);
        }
        Self {
            _lock: lock,
            previous_krunkit,
            previous_gvproxy,
            previous_helper_dir,
            previous_path,
        }
    }
}

#[cfg(test)]
impl Drop for MachineHelperEnvGuard {
    fn drop(&mut self) {
        match &self.previous_krunkit {
            Some(path) => unsafe { std::env::set_var(KRUNKIT_ENV, path) },
            None => unsafe { std::env::remove_var(KRUNKIT_ENV) },
        }
        match &self.previous_gvproxy {
            Some(path) => unsafe { std::env::set_var(GVPROXY_ENV, path) },
            None => unsafe { std::env::remove_var(GVPROXY_ENV) },
        }
        match &self.previous_helper_dir {
            Some(path) => unsafe { std::env::set_var(HELPER_BINARY_DIR_ENV, path) },
            None => unsafe { std::env::remove_var(HELPER_BINARY_DIR_ENV) },
        }
        match &self.previous_path {
            Some(path) => unsafe { std::env::set_var("PATH", path) },
            None => unsafe { std::env::remove_var("PATH") },
        }
    }
}

#[cfg(test)]
pub(super) fn write_helper_stub(path: &Path, _helper_name: &str) {
    crate::test_support::write_executable_stub(path, "#!/bin/sh\n");
}
