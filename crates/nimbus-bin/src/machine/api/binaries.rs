use super::*;

#[derive(Clone, Copy)]
pub(super) struct MachineApiBinaryRequirement {
    pub(super) name: &'static str,
    required_for_operations: &'static [&'static str],
}

const DEFAULT_GUEST_HELPER_BINARY_DIRS: &[&str] = &[
    "/usr/local/libexec/podman",
    "/usr/local/lib/podman",
    "/usr/libexec/podman",
    "/usr/lib/podman",
];

pub(super) const STANDARD_CONTAINER_BINARY_REQUIREMENTS: &[MachineApiBinaryRequirement] = &[
    MachineApiBinaryRequirement {
        name: "bootc",
        required_for_operations: &[
            MACHINE_API_BOOTC_STATUS_OPERATION,
            MACHINE_API_BOOTC_SWITCH_OPERATION,
            MACHINE_API_BOOTC_UPGRADE_OPERATION,
            MACHINE_API_BOOTC_ROLLBACK_OPERATION,
        ],
    },
    MachineApiBinaryRequirement {
        name: "conmon",
        required_for_operations: &[
            MACHINE_API_IMAGE_START_OPERATION,
            MACHINE_API_BUILD_START_OPERATION,
        ],
    },
    MachineApiBinaryRequirement {
        name: "crun",
        required_for_operations: &[
            MACHINE_API_IMAGE_START_OPERATION,
            MACHINE_API_BUILD_START_OPERATION,
        ],
    },
    MachineApiBinaryRequirement {
        name: "netavark",
        required_for_operations: &[
            MACHINE_API_IMAGE_START_OPERATION,
            MACHINE_API_BUILD_START_OPERATION,
        ],
    },
    MachineApiBinaryRequirement {
        name: "aardvark-dns",
        required_for_operations: &[
            MACHINE_API_IMAGE_START_OPERATION,
            MACHINE_API_BUILD_START_OPERATION,
        ],
    },
];

pub(crate) fn default_guest_helper_binary_dirs() -> Vec<PathBuf> {
    DEFAULT_GUEST_HELPER_BINARY_DIRS
        .iter()
        .map(PathBuf::from)
        .collect()
}

pub(super) fn apply_resolved_runtime_paths(
    config: &mut ContainerSandboxBackendConfig,
    path_env: Option<&OsStr>,
    helper_binary_dirs: &[PathBuf],
) {
    if let Some(path) = resolve_binary("conmon", path_env, helper_binary_dirs) {
        config.conmon_path = path;
    }
    if let Some(path) = resolve_binary("crun", path_env, helper_binary_dirs) {
        config.runtime_path = path;
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

pub(super) fn resolve_binary_statuses(
    path_env: Option<&OsStr>,
    helper_binary_dirs: &[PathBuf],
) -> Vec<MachineApiBinaryStatus> {
    STANDARD_CONTAINER_BINARY_REQUIREMENTS
        .iter()
        .map(|requirement| {
            let resolved_path = resolve_binary(requirement.name, path_env, helper_binary_dirs);
            MachineApiBinaryStatus {
                name: requirement.name.to_owned(),
                present: resolved_path.is_some(),
                resolved_path: resolved_path.map(|path| path.display().to_string()),
                required_for_operations: requirement
                    .required_for_operations
                    .iter()
                    .map(|operation| (*operation).to_owned())
                    .collect(),
            }
        })
        .collect()
}

pub(super) fn resolve_binary(
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
