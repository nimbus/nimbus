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

const NETWORK_STATE_DIR_ENV: &str = "NIMBUS_NETWORK_STATE_DIR";

/// Stable, host-local state root for process-wide network allocation authority.
///
/// This type resolves platform policy only. It does not create, authenticate,
/// or open the directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalNodeNetworkRoot(PathBuf);

impl LocalNodeNetworkRoot {
    /// Resolves the current platform's logical-node network root.
    ///
    /// An explicit root takes precedence over `NIMBUS_NETWORK_STATE_DIR`.
    pub fn resolve_for_current_platform(explicit: Option<&Path>) -> io::Result<Self> {
        let env = env_map(env::vars_os());
        Self::resolve_for_platform(LocalServerPlatform::current(), explicit, &env)
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }

    fn resolve_for_platform(
        platform: LocalServerPlatform,
        explicit: Option<&Path>,
        env: &BTreeMap<String, OsString>,
    ) -> io::Result<Self> {
        if let Some(explicit) = explicit {
            return validate_network_root(
                platform,
                explicit.to_path_buf(),
                "explicit network root",
            );
        }

        if let Some(value) = env.get(NETWORK_STATE_DIR_ENV) {
            return validate_network_root(platform, PathBuf::from(value), NETWORK_STATE_DIR_ENV);
        }

        let path = match platform {
            LocalServerPlatform::Linux => {
                let home = home_dir(env, LocalServerPlatform::Linux)?;
                env_path(env, "XDG_STATE_HOME")
                    .unwrap_or_else(|| home.join(".local").join("state"))
                    .join("nimbus")
                    .join("network")
            }
            LocalServerPlatform::MacOs => home_dir(env, LocalServerPlatform::MacOs)?
                .join("Library")
                .join("Application Support")
                .join("nimbus")
                .join("network"),
            LocalServerPlatform::Windows => env_path(env, "LOCALAPPDATA")
                .unwrap_or_else(|| {
                    user_profile_dir(env)
                        .unwrap_or_else(|| PathBuf::from(r"C:\Users\Default"))
                        .join("AppData")
                        .join("Local")
                })
                .join("nimbus")
                .join("network"),
        };

        validate_network_root(platform, path, "platform network root")
    }
}

fn validate_network_root(
    platform: LocalServerPlatform,
    path: PathBuf,
    source: &str,
) -> io::Result<LocalNodeNetworkRoot> {
    if path.as_os_str().is_empty() || !is_absolute_for_platform(platform, &path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{source} must be an absolute, non-empty path"),
        ));
    }

    Ok(LocalNodeNetworkRoot(path))
}

fn is_absolute_for_platform(platform: LocalServerPlatform, path: &Path) -> bool {
    match platform {
        LocalServerPlatform::Linux | LocalServerPlatform::MacOs => {
            path.as_os_str().to_string_lossy().starts_with('/')
        }
        LocalServerPlatform::Windows => {
            let value = path.as_os_str().to_string_lossy();
            let bytes = value.as_bytes();
            let has_drive_root = bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'\\' | b'/');
            has_drive_root || has_complete_windows_unc_root(&value)
        }
    }
}

fn has_complete_windows_unc_root(value: &str) -> bool {
    let Some(remainder) = value
        .strip_prefix(r"\\")
        .or_else(|| value.strip_prefix("//"))
    else {
        return false;
    };
    let mut components = remainder.split(['\\', '/']);
    components.next().is_some_and(|server| !server.is_empty())
        && components.next().is_some_and(|share| !share.is_empty())
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

pub fn env_map(vars: impl IntoIterator<Item = (OsString, OsString)>) -> BTreeMap<String, OsString> {
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

    #[test]
    fn network_root_explicit_path_wins_over_environment_override() {
        let root = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::Linux,
            Some(Path::new("/explicit/network-state")),
            &env(&[
                ("NIMBUS_NETWORK_STATE_DIR", "/environment/network-state"),
                ("XDG_STATE_HOME", "/xdg/state"),
                ("HOME", "/Users/jack"),
            ]),
        )
        .expect("absolute explicit network root should resolve");

        assert_eq!(root.as_path(), Path::new("/explicit/network-state"));
    }

    #[test]
    fn linux_network_root_uses_override_then_xdg_then_home() {
        let overridden = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::Linux,
            None,
            &env(&[
                ("NIMBUS_NETWORK_STATE_DIR", "/override/network-state"),
                ("XDG_STATE_HOME", "/xdg/state"),
                ("HOME", "/Users/jack"),
            ]),
        )
        .expect("linux override should resolve");
        assert_eq!(
            overridden.into_path_buf(),
            PathBuf::from("/override/network-state")
        );

        let xdg = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::Linux,
            None,
            &env(&[("XDG_STATE_HOME", "/xdg/state"), ("HOME", "/Users/jack")]),
        )
        .expect("linux XDG state root should resolve");
        assert_eq!(
            xdg.into_path_buf(),
            PathBuf::from("/xdg/state/nimbus/network")
        );

        let home = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::Linux,
            None,
            &env(&[("HOME", "/Users/jack")]),
        )
        .expect("linux home fallback should resolve");
        assert_eq!(
            home.into_path_buf(),
            PathBuf::from("/Users/jack/.local/state/nimbus/network")
        );
    }

    #[test]
    fn macos_network_root_uses_override_then_application_support() {
        let overridden = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::MacOs,
            None,
            &env(&[
                ("NIMBUS_NETWORK_STATE_DIR", "/override/network-state"),
                ("HOME", "/Users/jack"),
            ]),
        )
        .expect("macOS override should resolve");
        assert_eq!(
            overridden.into_path_buf(),
            PathBuf::from("/override/network-state")
        );

        let fallback = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::MacOs,
            None,
            &env(&[("HOME", "/Users/jack")]),
        )
        .expect("macOS Application Support fallback should resolve");
        assert_eq!(
            fallback.into_path_buf(),
            PathBuf::from("/Users/jack/Library/Application Support/nimbus/network")
        );
    }

    #[test]
    fn windows_network_root_uses_override_localappdata_then_profile() {
        let overridden = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::Windows,
            None,
            &env(&[
                ("NIMBUS_NETWORK_STATE_DIR", r"C:\Nimbus\NetworkState"),
                ("LOCALAPPDATA", r"C:\Users\jack\AppData\Local"),
            ]),
        )
        .expect("Windows override should resolve");
        assert_eq!(
            overridden.into_path_buf(),
            PathBuf::from(r"C:\Nimbus\NetworkState")
        );

        let local_app_data = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::Windows,
            None,
            &env(&[("LOCALAPPDATA", r"C:\Users\jack\AppData\Local")]),
        )
        .expect("Windows LOCALAPPDATA root should resolve");
        assert_eq!(
            local_app_data.into_path_buf(),
            PathBuf::from(r"C:\Users\jack\AppData\Local").join("nimbus/network")
        );

        let profile = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::Windows,
            None,
            &env(&[("USERPROFILE", r"C:\Users\jack")]),
        )
        .expect("Windows profile fallback should resolve");
        assert_eq!(
            profile.into_path_buf(),
            PathBuf::from(r"C:\Users\jack")
                .join("AppData")
                .join("Local")
                .join("nimbus")
                .join("network")
        );
    }

    #[test]
    fn network_root_rejects_empty_and_relative_explicit_paths() {
        for invalid in [Path::new(""), Path::new("relative/network-state")] {
            let error = LocalNodeNetworkRoot::resolve_for_platform(
                LocalServerPlatform::Linux,
                Some(invalid),
                &env(&[("HOME", "/Users/jack")]),
            )
            .expect_err("invalid explicit network root must be rejected");

            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn network_root_absolute_syntax_follows_the_target_platform() {
        assert!(is_absolute_for_platform(
            LocalServerPlatform::Linux,
            Path::new("/var/lib/nimbus/network")
        ));
        assert!(is_absolute_for_platform(
            LocalServerPlatform::MacOs,
            Path::new("/Users/jack/Library/Application Support/nimbus/network")
        ));
        assert!(is_absolute_for_platform(
            LocalServerPlatform::Windows,
            Path::new(r"C:\Users\jack\AppData\Local\nimbus\network")
        ));
        assert!(is_absolute_for_platform(
            LocalServerPlatform::Windows,
            Path::new(r"\\server\share\nimbus\network")
        ));
        assert!(!is_absolute_for_platform(
            LocalServerPlatform::Linux,
            Path::new(r"C:\Nimbus\Network")
        ));
        assert!(!is_absolute_for_platform(
            LocalServerPlatform::Windows,
            Path::new("/var/lib/nimbus/network")
        ));
        for incomplete_unc in [r"\\", r"\\server", r"\\server\", r"\\\share"] {
            assert!(
                !is_absolute_for_platform(LocalServerPlatform::Windows, Path::new(incomplete_unc)),
                "incomplete UNC root must be rejected: {incomplete_unc:?}"
            );
        }
    }

    #[test]
    fn network_root_rejects_empty_and_relative_environment_overrides() {
        for invalid in ["", "relative/network-state"] {
            let error = LocalNodeNetworkRoot::resolve_for_platform(
                LocalServerPlatform::Linux,
                None,
                &env(&[
                    ("NIMBUS_NETWORK_STATE_DIR", invalid),
                    ("HOME", "/Users/jack"),
                ]),
            )
            .expect_err("invalid environment network root must be rejected");

            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn network_root_rejects_relative_platform_fallbacks() {
        let linux_error = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::Linux,
            None,
            &env(&[
                ("XDG_STATE_HOME", "relative/state"),
                ("HOME", "/Users/jack"),
            ]),
        )
        .expect_err("relative XDG state root must be rejected");
        assert_eq!(linux_error.kind(), io::ErrorKind::InvalidInput);

        let macos_error = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::MacOs,
            None,
            &env(&[("HOME", "relative/home")]),
        )
        .expect_err("relative macOS home must be rejected");
        assert_eq!(macos_error.kind(), io::ErrorKind::InvalidInput);

        let windows_error = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::Windows,
            None,
            &env(&[("LOCALAPPDATA", r"relative\AppData\Local")]),
        )
        .expect_err("relative Windows LOCALAPPDATA must be rejected");
        assert_eq!(windows_error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn default_network_root_is_independent_of_project_working_directory() {
        let project_a = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::Linux,
            None,
            &env(&[("HOME", "/Users/jack"), ("PWD", "/workspaces/project-a")]),
        )
        .expect("project A root should resolve");
        let project_b = LocalNodeNetworkRoot::resolve_for_platform(
            LocalServerPlatform::Linux,
            None,
            &env(&[("HOME", "/Users/jack"), ("PWD", "/workspaces/project-b")]),
        )
        .expect("project B root should resolve");

        assert_eq!(project_a, project_b);
    }

    #[test]
    fn resolving_network_root_does_not_create_the_directory() {
        let tempdir = tempfile::tempdir().expect("temporary directory");
        let state_home = tempdir.path().join("state");
        let env = BTreeMap::from([
            ("HOME".to_string(), tempdir.path().as_os_str().to_owned()),
            (
                "XDG_STATE_HOME".to_string(),
                state_home.as_os_str().to_owned(),
            ),
        ]);

        let root =
            LocalNodeNetworkRoot::resolve_for_platform(LocalServerPlatform::Linux, None, &env)
                .expect("network root should resolve without an effect");

        assert_eq!(root.as_path(), state_home.join("nimbus/network"));
        assert!(!root.as_path().exists());
    }
}
