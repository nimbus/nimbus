//! Per-machine filesystem paths and env-based root resolution.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;

use nimbus_core::Error;
use serde::Serialize;

pub const DEFAULT_MACHINE_RUNTIME_ROOT: &str = "/tmp/nimbus";
pub const MACHINE_RUNTIME_ROOT_ENV: &str = "NIMBUS_MACHINE_RUNTIME_ROOT";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MachinePaths {
    pub name: String,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub data_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub config_path: PathBuf,
    pub generated_ignition_path: PathBuf,
    pub state_path: PathBuf,
    pub guest_config_bundle_dir: PathBuf,
    pub image_cache_dir: PathBuf,
    pub guest_binary_cache_dir: PathBuf,
    pub materialized_image_path: PathBuf,
    pub api_socket_path: PathBuf,
    pub ready_socket_path: PathBuf,
    pub ignition_socket_path: PathBuf,
    pub gvproxy_socket_path: PathBuf,
    /// Restful control endpoint for the active VMM (krunkit/vfkit `--restful-uri`).
    /// One VMM runs per machine, so this is a single provider-neutral slot.
    pub vmm_endpoint_path: PathBuf,
    pub efi_variable_store_path: PathBuf,
    pub api_forward_pid_path: PathBuf,
    pub gvproxy_pid_path: PathBuf,
    /// Durable parent-authenticated process birth receipt for the exact
    /// gvproxy incarnation. The numeric pidfile remains provider output, not
    /// signaling authority.
    pub gvproxy_process_identity_path: PathBuf,
    /// Pidfile for the active VMM process (krunkit/vfkit `--pidfile`). The
    /// readiness/stop lifecycle reads this slot regardless of provider.
    pub vmm_pid_path: PathBuf,
    pub api_forward_log_path: PathBuf,
    pub machine_log_path: PathBuf,
    pub gvproxy_log_path: PathBuf,
    /// Diagnostic log for the active VMM. krunkit writes it via `--log-file`.
    /// vfkit has no such flag, so the spawn path instead captures vfkit's
    /// stdout+stderr into this same file, keeping failed-boot triage uniform
    /// across providers (the guest console log still lives in
    /// [`machine_log_path`](Self::machine_log_path) for both).
    pub vmm_log_path: PathBuf,
}

impl MachinePaths {
    pub fn ensure_directories(&self) -> Result<(), Error> {
        fs::create_dir_all(&self.config_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine config directory {}: {error}",
                self.config_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.state_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine state directory {}: {error}",
                self.state_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.data_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine data directory {}: {error}",
                self.data_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.image_cache_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine image cache directory {}: {error}",
                self.image_cache_dir.display()
            ))
        })?;
        fs::create_dir_all(&self.guest_binary_cache_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create guest binary cache directory {}: {error}",
                self.guest_binary_cache_dir.display()
            ))
        })?;
        let materialized_parent = self.materialized_image_path.parent().ok_or_else(|| {
            Error::Internal(format!(
                "failed to resolve parent directory for machine image {}",
                self.materialized_image_path.display()
            ))
        })?;
        fs::create_dir_all(materialized_parent).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine image data directory {}: {error}",
                materialized_parent.display()
            ))
        })?;
        self.ensure_runtime_directories()
    }

    pub fn ensure_runtime_directories(&self) -> Result<(), Error> {
        fs::create_dir_all(&self.runtime_dir).map_err(|error| {
            Error::Internal(format!(
                "failed to create machine runtime directory {}: {error}",
                self.runtime_dir.display()
            ))
        })
    }

    pub fn krunkit_gvproxy_socket_path(&self) -> PathBuf {
        PathBuf::from(format!("{}-krun.sock", self.gvproxy_socket_path.display()))
    }
}

pub(crate) fn resolve_config_root_with_env(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<PathBuf, Error> {
    if let Some(path) = lookup("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir_with_env(lookup)?
        .join(".config")
        .join("nimbus")
        .join("machine"))
}

pub(crate) fn resolve_state_root_with_env(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<PathBuf, Error> {
    if let Some(path) = lookup("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir_with_env(lookup)?
        .join(".local")
        .join("state")
        .join("nimbus")
        .join("machine"))
}

pub(crate) fn resolve_data_root_with_env(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<PathBuf, Error> {
    if let Some(path) = lookup("XDG_DATA_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir_with_env(lookup)?
        .join(".local")
        .join("share")
        .join("nimbus")
        .join("machine"))
}

pub(crate) fn resolve_cache_root_with_env(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<PathBuf, Error> {
    if let Some(path) = lookup("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("nimbus").join("machine"));
    }
    Ok(resolve_home_dir_with_env(lookup)?
        .join(".cache")
        .join("nimbus")
        .join("machine"))
}

fn resolve_home_dir_with_env(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> Result<PathBuf, Error> {
    if let Some(home) = lookup("HOME") {
        return Ok(PathBuf::from(home));
    }
    if cfg!(windows) {
        if let Some(profile) = lookup("USERPROFILE") {
            return Ok(PathBuf::from(profile));
        }
        if let (Some(drive), Some(path)) = (lookup("HOMEDRIVE"), lookup("HOMEPATH"))
            && !drive.is_empty()
            && !path.is_empty()
        {
            return Ok(PathBuf::from(drive).join(path));
        }
    }
    Err(Error::InvalidInput(
        "HOME is not set; cannot resolve machine directories".to_owned(),
    ))
}

pub fn resolve_runtime_root() -> PathBuf {
    resolve_runtime_root_with_env(&mut |name| env::var_os(name))
}

pub(crate) fn resolve_runtime_root_with_env(
    lookup: &mut impl FnMut(&str) -> Option<OsString>,
) -> PathBuf {
    lookup(MACHINE_RUNTIME_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MACHINE_RUNTIME_ROOT))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::roots::MachineRootLayout;

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn machine_paths_ensure_directories_creates_config_state_data_cache_and_runtime_roots() {
        let root = unique_temp_dir("ensure-directories");
        let layout = MachineRootLayout::test_sibling_roots(
            root.join("config"),
            root.join("state"),
            root.join("runtime"),
        );
        let paths = layout.paths("default");

        paths
            .ensure_directories()
            .expect("machine directories should be created");

        assert!(paths.config_dir.is_dir());
        assert!(paths.state_dir.is_dir());
        assert!(paths.data_dir.is_dir());
        assert!(paths.image_cache_dir.is_dir());
        assert!(paths.guest_binary_cache_dir.is_dir());
        assert!(
            paths
                .materialized_image_path
                .parent()
                .expect("materialized image should have parent")
                .is_dir()
        );
        assert!(paths.runtime_dir.is_dir());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn machine_paths_ensure_directories_reports_create_failures_with_path_context() {
        let root = unique_temp_dir("ensure-directories-error");
        fs::create_dir_all(&root).expect("temp root should create");
        fs::write(root.join("config"), b"not a directory").expect("blocking file should write");
        let layout = MachineRootLayout::test_sibling_roots(
            root.join("config"),
            root.join("state"),
            root.join("runtime"),
        );
        let paths = layout.paths("default");

        let error = paths
            .ensure_directories()
            .expect_err("file in place of config directory should fail");

        let message = error.to_string();
        assert!(
            message.contains("failed to create machine config directory")
                && message.contains("default"),
            "{message}"
        );

        let _ = fs::remove_dir_all(root);
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "nimbus-machine-{}-{}-{label}",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
