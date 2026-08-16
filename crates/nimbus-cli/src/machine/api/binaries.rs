use super::*;
use crate::provider_binaries::resolve_binary;

#[derive(Clone, Copy)]
pub(super) struct MachineApiBinaryRequirement {
    pub(super) name: &'static str,
    required_for_operations: &'static [&'static str],
}

/// Canonical operator-facing order: OS lifecycle, image materialization,
/// process monitor/runtime, then network helpers.
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
        name: "buildah",
        required_for_operations: &[MACHINE_API_WORKLOAD_PROVISION_PHASE_OPERATION],
    },
    MachineApiBinaryRequirement {
        name: "conmon",
        required_for_operations: &[MACHINE_API_WORKLOAD_PROVISION_PHASE_OPERATION],
    },
    MachineApiBinaryRequirement {
        name: "crun",
        required_for_operations: &[MACHINE_API_WORKLOAD_PROVISION_PHASE_OPERATION],
    },
    MachineApiBinaryRequirement {
        name: "netavark",
        required_for_operations: &[MACHINE_API_WORKLOAD_PROVISION_PHASE_OPERATION],
    },
    MachineApiBinaryRequirement {
        name: "aardvark-dns",
        required_for_operations: &[MACHINE_API_WORKLOAD_PROVISION_PHASE_OPERATION],
    },
];

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
