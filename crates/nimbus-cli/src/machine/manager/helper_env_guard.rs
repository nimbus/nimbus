//! Test-only guard for the machine helper-binary env overrides
//! (`KRUNKIT_ENV`, `VFKIT_ENV`, `GVPROXY_ENV`, `HELPER_BINARY_DIR_ENV`, `PATH`).
//!
//! Production helper resolution lives in `super::helper_paths`; this module
//! only exists so tests can mutate those process-wide env vars under a shared
//! lock and restore them on drop.

use std::path::Path;

use super::{GVPROXY_ENV, HELPER_BINARY_DIR_ENV, KRUNKIT_ENV, VFKIT_ENV};

fn helper_env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

pub(crate) struct MachineHelperEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous_krunkit: Option<std::ffi::OsString>,
    previous_vfkit: Option<std::ffi::OsString>,
    previous_gvproxy: Option<std::ffi::OsString>,
    previous_helper_dir: Option<std::ffi::OsString>,
    previous_path: Option<std::ffi::OsString>,
}

impl MachineHelperEnvGuard {
    pub(crate) fn install_stub_binaries(dir: &Path) -> Self {
        let krunkit_path = dir.join("krunkit");
        let gvproxy_path = dir.join("gvproxy");
        let vfkit_path = dir.join("vfkit");
        write_helper_stub(&krunkit_path, "krunkit");
        write_helper_stub(&gvproxy_path, "gvproxy");
        write_helper_stub(&vfkit_path, "vfkit");
        let guard = Self::set_paths(&krunkit_path, &gvproxy_path);
        // vfkit is the opt-in VMM backend. Install its stub and point the
        // `NIMBUS_MACHINE_VFKIT` override at it under the same env lock so the
        // vfkit launch path resolves a binary in tests; `guard` captured the
        // previous value before this set, so Drop still restores it.
        unsafe {
            std::env::set_var(VFKIT_ENV, &vfkit_path);
        }
        guard
    }

    pub(crate) fn set_paths(krunkit_path: &Path, gvproxy_path: &Path) -> Self {
        let lock = helper_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_krunkit = std::env::var_os(KRUNKIT_ENV);
        let previous_vfkit = std::env::var_os(VFKIT_ENV);
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
            previous_vfkit,
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
        let previous_vfkit = std::env::var_os(VFKIT_ENV);
        let previous_gvproxy = std::env::var_os(GVPROXY_ENV);
        let previous_helper_dir = std::env::var_os(HELPER_BINARY_DIR_ENV);
        let previous_path = std::env::var_os("PATH");
        unsafe {
            std::env::remove_var(KRUNKIT_ENV);
            std::env::remove_var(VFKIT_ENV);
            std::env::remove_var(GVPROXY_ENV);
            std::env::set_var(HELPER_BINARY_DIR_ENV, dir);
        }
        Self {
            _lock: lock,
            previous_krunkit,
            previous_vfkit,
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
        let previous_vfkit = std::env::var_os(VFKIT_ENV);
        let previous_gvproxy = std::env::var_os(GVPROXY_ENV);
        let previous_helper_dir = std::env::var_os(HELPER_BINARY_DIR_ENV);
        let previous_path = std::env::var_os("PATH");
        unsafe {
            std::env::remove_var(KRUNKIT_ENV);
            std::env::remove_var(VFKIT_ENV);
            std::env::remove_var(GVPROXY_ENV);
            std::env::remove_var(HELPER_BINARY_DIR_ENV);
            std::env::set_var("PATH", dir);
        }
        Self {
            _lock: lock,
            previous_krunkit,
            previous_vfkit,
            previous_gvproxy,
            previous_helper_dir,
            previous_path,
        }
    }
}

impl Drop for MachineHelperEnvGuard {
    fn drop(&mut self) {
        match &self.previous_krunkit {
            Some(path) => unsafe { std::env::set_var(KRUNKIT_ENV, path) },
            None => unsafe { std::env::remove_var(KRUNKIT_ENV) },
        }
        match &self.previous_vfkit {
            Some(path) => unsafe { std::env::set_var(VFKIT_ENV, path) },
            None => unsafe { std::env::remove_var(VFKIT_ENV) },
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

pub(super) fn write_helper_stub(path: &Path, _helper_name: &str) {
    crate::test_support::write_executable_stub(path, "#!/bin/sh\n");
}
