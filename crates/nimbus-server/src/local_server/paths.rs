use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalServerPlatform {
    Linux,
    MacOs,
    Windows,
}

impl LocalServerPlatform {
    pub fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::MacOs
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Self::Linux
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalServerPaths {
    pub auth_token_path: PathBuf,
    pub server_discovery_path: PathBuf,
    pub audit_log_path: PathBuf,
}

impl LocalServerPaths {
    pub fn resolve_for_current_platform() -> io::Result<Self> {
        let env = env_map(env::vars_os());
        Self::resolve_for_platform(LocalServerPlatform::current(), &env)
    }

    pub fn resolve_for_platform(
        platform: LocalServerPlatform,
        env: &BTreeMap<String, OsString>,
    ) -> io::Result<Self> {
        match platform {
            LocalServerPlatform::Linux => resolve_linux_paths(env),
            LocalServerPlatform::MacOs => resolve_macos_paths(env),
            LocalServerPlatform::Windows => resolve_windows_paths(env),
        }
    }

    pub fn ensure_auth_parent_dir(&self) -> io::Result<()> {
        ensure_secure_parent_dir(&self.auth_token_path)
    }

    pub fn ensure_run_state_parent_dir(&self) -> io::Result<()> {
        ensure_secure_parent_dir(&self.server_discovery_path)
    }

    pub fn ensure_audit_parent_dir(&self) -> io::Result<()> {
        ensure_secure_parent_dir(&self.audit_log_path)
    }
}

pub(crate) fn env_map(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
) -> BTreeMap<String, OsString> {
    vars.into_iter()
        .map(|(key, value)| (key.to_string_lossy().into_owned(), value))
        .collect()
}

fn resolve_linux_paths(env: &BTreeMap<String, OsString>) -> io::Result<LocalServerPaths> {
    let home = home_dir(env, LocalServerPlatform::Linux)?;
    let data_root = env_path(env, "XDG_DATA_HOME")
        .unwrap_or_else(|| home.join(".local").join("share"))
        .join("nimbus");
    let state_root = env_path(env, "XDG_STATE_HOME")
        .unwrap_or_else(|| home.join(".local").join("state"))
        .join("nimbus");
    let server_discovery_path = if let Some(runtime_root) = env_path(env, "XDG_RUNTIME_DIR") {
        runtime_root.join("nimbus").join("server.json")
    } else {
        state_root.join("run").join("server.json")
    };
    Ok(LocalServerPaths {
        auth_token_path: data_root.join("auth").join("token"),
        server_discovery_path,
        audit_log_path: state_root.join("logs").join("access.jsonl"),
    })
}

fn resolve_macos_paths(env: &BTreeMap<String, OsString>) -> io::Result<LocalServerPaths> {
    let home = home_dir(env, LocalServerPlatform::MacOs)?;
    let application_support_root = home
        .join("Library")
        .join("Application Support")
        .join("nimbus");
    let server_discovery_path = if let Some(tmpdir) = env_path(env, "TMPDIR") {
        tmpdir.join("nimbus").join("server.json")
    } else {
        application_support_root.join("run").join("server.json")
    };
    Ok(LocalServerPaths {
        auth_token_path: application_support_root.join("auth").join("token"),
        server_discovery_path,
        audit_log_path: home
            .join("Library")
            .join("Logs")
            .join("nimbus")
            .join("access.jsonl"),
    })
}

fn resolve_windows_paths(env: &BTreeMap<String, OsString>) -> io::Result<LocalServerPaths> {
    let local_app_data = env_path(env, "LOCALAPPDATA").unwrap_or_else(|| {
        user_profile_dir(env)
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default"))
            .join("AppData")
            .join("Local")
    });
    let nimbus_root = local_app_data.join("nimbus");
    Ok(LocalServerPaths {
        auth_token_path: nimbus_root.join("auth").join("token.json"),
        server_discovery_path: nimbus_root.join("run").join("server.json"),
        audit_log_path: nimbus_root.join("logs").join("access.jsonl"),
    })
}

fn env_path(env: &BTreeMap<String, OsString>, key: &str) -> Option<PathBuf> {
    env.get(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn home_dir(
    env: &BTreeMap<String, OsString>,
    platform: LocalServerPlatform,
) -> io::Result<PathBuf> {
    match platform {
        LocalServerPlatform::Windows => user_profile_dir(env).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "USERPROFILE is not set; cannot resolve local server directories",
            )
        }),
        LocalServerPlatform::Linux | LocalServerPlatform::MacOs => env_path(env, "HOME")
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "HOME is not set; cannot resolve local server directories",
                )
            }),
    }
}

fn user_profile_dir(env: &BTreeMap<String, OsString>) -> Option<PathBuf> {
    env_path(env, "USERPROFILE").or_else(|| {
        let drive = env.get("HOMEDRIVE")?;
        let path = env.get("HOMEPATH")?;
        if drive.is_empty() || path.is_empty() {
            return None;
        }
        Some(PathBuf::from(drive).join(path))
    })
}

fn ensure_secure_parent_dir(path: &Path) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path {} does not have a parent directory", path.display()),
        )
    })?;
    fs::create_dir_all(parent)?;
    set_secure_directory_permissions(parent)?;
    Ok(())
}

#[cfg(unix)]
fn set_secure_directory_permissions(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_secure_directory_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(entries: &[(&str, &str)]) -> BTreeMap<String, OsString> {
        entries
            .iter()
            .map(|(key, value)| (key.to_string(), OsString::from(value)))
            .collect()
    }

    #[test]
    fn linux_paths_use_xdg_overrides() {
        let paths = LocalServerPaths::resolve_for_platform(
            LocalServerPlatform::Linux,
            &env(&[
                ("HOME", "/Users/jack"),
                ("XDG_DATA_HOME", "/tmp/data"),
                ("XDG_STATE_HOME", "/tmp/state"),
                ("XDG_RUNTIME_DIR", "/tmp/runtime"),
            ]),
        )
        .expect("linux paths should resolve");

        assert_eq!(
            paths.auth_token_path,
            PathBuf::from("/tmp/data/nimbus/auth/token")
        );
        assert_eq!(
            paths.server_discovery_path,
            PathBuf::from("/tmp/runtime/nimbus/server.json")
        );
        assert_eq!(
            paths.audit_log_path,
            PathBuf::from("/tmp/state/nimbus/logs/access.jsonl")
        );
    }

    #[test]
    fn linux_paths_fall_back_to_home_convention() {
        let paths = LocalServerPaths::resolve_for_platform(
            LocalServerPlatform::Linux,
            &env(&[("HOME", "/Users/jack")]),
        )
        .expect("linux fallback paths should resolve");

        assert_eq!(
            paths.auth_token_path,
            PathBuf::from("/Users/jack/.local/share/nimbus/auth/token")
        );
        assert_eq!(
            paths.server_discovery_path,
            PathBuf::from("/Users/jack/.local/state/nimbus/run/server.json")
        );
        assert_eq!(
            paths.audit_log_path,
            PathBuf::from("/Users/jack/.local/state/nimbus/logs/access.jsonl")
        );
    }

    #[test]
    fn macos_paths_prefer_tmpdir_for_run_state() {
        let paths = LocalServerPaths::resolve_for_platform(
            LocalServerPlatform::MacOs,
            &env(&[
                ("HOME", "/Users/jack"),
                ("TMPDIR", "/private/tmp/nimbus-test"),
            ]),
        )
        .expect("macos paths should resolve");

        assert_eq!(
            paths.auth_token_path,
            PathBuf::from("/Users/jack/Library/Application Support/nimbus/auth/token")
        );
        assert_eq!(
            paths.server_discovery_path,
            PathBuf::from("/private/tmp/nimbus-test/nimbus/server.json")
        );
        assert_eq!(
            paths.audit_log_path,
            PathBuf::from("/Users/jack/Library/Logs/nimbus/access.jsonl")
        );
    }

    #[test]
    fn macos_paths_fall_back_to_application_support_run_state() {
        let paths = LocalServerPaths::resolve_for_platform(
            LocalServerPlatform::MacOs,
            &env(&[("HOME", "/Users/jack")]),
        )
        .expect("macos fallback paths should resolve");

        assert_eq!(
            paths.server_discovery_path,
            PathBuf::from("/Users/jack/Library/Application Support/nimbus/run/server.json")
        );
    }

    #[test]
    fn windows_paths_use_localappdata_with_userprofile_fallback() {
        let explicit = LocalServerPaths::resolve_for_platform(
            LocalServerPlatform::Windows,
            &env(&[("LOCALAPPDATA", r"C:\Users\jack\AppData\Local")]),
        )
        .expect("windows paths should resolve");
        assert_eq!(
            explicit.auth_token_path,
            PathBuf::from(r"C:\Users\jack\AppData\Local")
                .join("nimbus")
                .join("auth")
                .join("token.json")
        );
        assert_eq!(
            explicit.server_discovery_path,
            PathBuf::from(r"C:\Users\jack\AppData\Local")
                .join("nimbus")
                .join("run")
                .join("server.json")
        );
        assert_eq!(
            explicit.audit_log_path,
            PathBuf::from(r"C:\Users\jack\AppData\Local")
                .join("nimbus")
                .join("logs")
                .join("access.jsonl")
        );

        let fallback = LocalServerPaths::resolve_for_platform(
            LocalServerPlatform::Windows,
            &env(&[("USERPROFILE", r"C:\Users\jack")]),
        )
        .expect("windows fallback paths should resolve");
        assert_eq!(
            fallback.auth_token_path,
            PathBuf::from(r"C:\Users\jack")
                .join("AppData")
                .join("Local")
                .join("nimbus")
                .join("auth")
                .join("token.json")
        );
    }
}
