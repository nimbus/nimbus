use super::*;
use std::sync::{Mutex, OnceLock};

fn license_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct LicenseEnvGuard {
    key: &'static str,
    previous: Option<std::ffi::OsString>,
}

impl LicenseEnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = env::var_os(key);
        unsafe { env::set_var(key, value) };
        Self { key, previous }
    }

    fn clear(key: &'static str) -> Self {
        let previous = env::var_os(key);
        unsafe { env::remove_var(key) };
        Self { key, previous }
    }
}

impl Drop for LicenseEnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { env::set_var(self.key, value) },
            None => unsafe { env::remove_var(self.key) },
        }
    }
}

#[test]
fn resolve_license_path_returns_explicit_path() {
    let path = Path::new("/explicit/license.json");
    let result = super::boot::resolve_license_path(Some(path));
    assert_eq!(result, Some(PathBuf::from("/explicit/license.json")));
}

#[test]
fn resolve_license_path_defers_to_env_when_set() {
    let _lock = license_env_lock()
        .lock()
        .expect("license env lock should not be poisoned");
    let _guard = LicenseEnvGuard::set(nimbus::LICENSE_FILE_ENV, "/env/license.json");
    let result = super::boot::resolve_license_path(None);
    assert!(
        result.is_none(),
        "should return None so LicenseState::load handles the env var, got: {result:?}"
    );
}

#[test]
fn resolve_license_path_returns_xdg_default_when_file_exists() {
    let _lock = license_env_lock()
        .lock()
        .expect("license env lock should not be poisoned");
    let _guard = LicenseEnvGuard::clear(nimbus::LICENSE_FILE_ENV);
    let temp = tempfile::tempdir().expect("tempdir should build");
    let config_dir = temp.path().join("nimbus");
    fs::create_dir_all(&config_dir).expect("config dir should build");
    fs::write(config_dir.join("license.json"), "{}").expect("license file should write");
    let _xdg_guard = LicenseEnvGuard::set("XDG_CONFIG_HOME", temp.path().to_str().unwrap());
    let result = super::boot::resolve_license_path(None);
    assert_eq!(
        result,
        Some(config_dir.join("license.json")),
        "should return the XDG default path when the file exists"
    );
}

#[test]
fn resolve_license_path_returns_none_when_no_xdg_default() {
    let _lock = license_env_lock()
        .lock()
        .expect("license env lock should not be poisoned");
    let _guard = LicenseEnvGuard::clear(nimbus::LICENSE_FILE_ENV);
    let temp = tempfile::tempdir().expect("tempdir should build");
    let _xdg_guard = LicenseEnvGuard::set("XDG_CONFIG_HOME", temp.path().to_str().unwrap());
    let result = super::boot::resolve_license_path(None);
    assert!(
        result.is_none(),
        "should return None when XDG license file does not exist, got: {result:?}"
    );
}
