use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use nimbus_sandbox::backends::krun::KrunSandboxBackendConfig;

const DEFAULT_CONTAINER_PROVIDER_BINARY_DIRS: &[&str] = &[
    "/usr/local/libexec/podman",
    "/usr/local/lib/podman",
    "/usr/libexec/podman",
    "/usr/lib/podman",
];

pub(crate) fn default_container_provider_binary_dirs() -> Vec<PathBuf> {
    DEFAULT_CONTAINER_PROVIDER_BINARY_DIRS
        .iter()
        .map(PathBuf::from)
        .collect()
}

pub(crate) fn resolve_binary(
    name: &str,
    path_env: Option<&OsStr>,
    helper_binary_dirs: &[PathBuf],
) -> Option<PathBuf> {
    let binary_name = Path::new(name);
    if binary_name.components().count() > 1 {
        return is_executable_file(binary_name).then(|| binary_name.to_path_buf());
    }

    for directory in helper_binary_dirs {
        let candidate = directory.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }

    let path_env = path_env?;
    std::env::split_paths(path_env)
        .map(|directory| directory.join(name))
        .find(|candidate| is_executable_file(candidate))
}

pub(crate) fn apply_resolved_krun_runtime_paths(
    config: &mut KrunSandboxBackendConfig,
    path_env: Option<&OsStr>,
    helper_binary_dirs: &[PathBuf],
) {
    if let Some(path) = resolve_binary("conmon", path_env, helper_binary_dirs) {
        config.conmon_path = path;
    }
    if let Some(path) = resolve_binary("buildah", path_env, helper_binary_dirs) {
        config.buildah_path = path;
    }
    if let Some(path) = resolve_binary("netavark", path_env, helper_binary_dirs) {
        config.netavark_path = path;
    }
    if let Some(path) = resolve_binary("aardvark-dns", path_env, helper_binary_dirs) {
        config.aardvark_dns_path = path;
    }
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        std::fs::metadata(path)
            .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        path.is_file()
    }
}
