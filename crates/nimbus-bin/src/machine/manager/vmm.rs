//! Per-provider VMM backend seam.
//!
//! A [`MachineVmmBackend`] owns everything that differs between the macOS
//! micro-VM monitors Nimbus can drive: which VMM binary to resolve, how to
//! construct its launch command line, whether it needs the external gvproxy
//! user-mode network stack, and the gvproxy listen-mode ↔ on-VMM net-device
//! pairing. The lifecycle in `manager.rs`/`readiness.rs`/`stop.rs` stays
//! provider-agnostic and talks to the resolved backend through this trait.
//!
//! krunkit is the only implemented backend today. vfkit (the second macOS VMM)
//! and wsl2 resolve to explicit "not available yet" errors so selection is a
//! deliberate, fail-closed opt-in rather than silent auto-detection.

use std::path::{Path, PathBuf};

use nimbus::Error;

use super::super::guest_config::GUEST_MACHINE_CONFIG_MOUNT_TAG;
use super::super::{MachineBootstrapMode, MachineConfigRecord, MachinePaths, MachineProvider};
use super::helpers::{known_helper_candidates, resolve_helper_binary};
use super::launch::{
    MachineCommandLine, build_virtio_vsock_listen_arg, build_virtiofs_arg, build_virtiofs_args,
};
use super::{DEFAULT_KRUNKIT_BINARY, DEFAULT_MACHINE_MAC_ADDRESS, KRUNKIT_ENV, READY_VSOCK_PORT};

/// Everything `MachineLaunchPlan::build` knows about a boot that a VMM backend
/// needs to assemble its launch command. Borrowed for the duration of the build
/// so the backend never owns plan state.
pub(super) struct VmmLaunchContext<'a> {
    pub(super) paths: &'a MachinePaths,
    pub(super) config: &'a MachineConfigRecord,
    pub(super) image_path: &'a Path,
    pub(super) efi_variable_store_path: &'a Path,
    pub(super) rest_uri: &'a str,
    pub(super) bootstrap_mode: MachineBootstrapMode,
    pub(super) machine_config_bundle_dir: Option<&'a Path>,
}

/// The per-provider VMM contract. One implementation per macOS micro-VM monitor.
pub(super) trait MachineVmmBackend {
    /// The provider this backend serves.
    fn provider(&self) -> MachineProvider;

    /// Resolve the VMM binary, honoring the per-VMM env override first, then the
    /// bundled/known helper directories.
    fn resolve_vmm_binary(&self) -> Result<PathBuf, Error>;

    /// Whether this VMM relies on the external gvproxy user-mode network stack.
    /// Provider-managed networking (e.g. WSL2) owns its own host networking and
    /// does not need gvproxy.
    fn requires_gvproxy(&self) -> bool {
        !self.provider().uses_provider_networking()
    }

    /// The gvproxy listen-mode arguments that pair with this VMM's net device.
    /// Both macOS micro-VM monitors share the unixgram listen contract; only the
    /// on-VMM net-device syntax in [`build_launch_command`](Self::build_launch_command)
    /// differs.
    fn gvproxy_listen_args(&self, socket_path: &Path) -> Vec<String>;

    /// Construct the VMM launch command line for the already-resolved binary.
    fn build_launch_command(
        &self,
        vmm_binary: &Path,
        ctx: &VmmLaunchContext<'_>,
    ) -> Result<MachineCommandLine, Error>;
}

/// Resolve the VMM backend for a provider. Krunkit is the only implemented
/// backend; vfkit and wsl2 fail closed with explicit messages until their
/// backends land. This is the single provider gate on the start path —
/// `MachineLaunchPlan::build` calls it before any host state mutates, so an
/// unsupported provider never partially starts a machine.
pub(super) fn vmm_backend(provider: MachineProvider) -> Result<Box<dyn MachineVmmBackend>, Error> {
    match provider {
        MachineProvider::Krunkit => Ok(Box::new(KrunkitVmmBackend)),
        MachineProvider::Vfkit => Err(Error::InvalidInput(
            "the vfkit machine provider is not implemented yet".to_owned(),
        )),
        MachineProvider::Wsl2 => Err(Error::InvalidInput(
            "the WSL2 machine provider is not available on this host yet".to_owned(),
        )),
    }
}

/// krunkit: the libkrun-based macOS micro-VM monitor. Bootstraps the
/// Nimbus-managed `applehv` disk over EFI and attaches gvproxy through a
/// `virtio-net,type=unixgram` device.
pub(super) struct KrunkitVmmBackend;

impl MachineVmmBackend for KrunkitVmmBackend {
    fn provider(&self) -> MachineProvider {
        MachineProvider::Krunkit
    }

    fn resolve_vmm_binary(&self) -> Result<PathBuf, Error> {
        // krunkit is not bundled in the Nimbus archive; it is installed by the
        // cask's declared dependency, so resolution has no bundled candidates
        // and falls through to the Homebrew/Podman known directories.
        resolve_helper_binary(
            KRUNKIT_ENV,
            DEFAULT_KRUNKIT_BINARY,
            &[],
            &known_helper_candidates(DEFAULT_KRUNKIT_BINARY),
        )
    }

    fn gvproxy_listen_args(&self, socket_path: &Path) -> Vec<String> {
        vec![
            "-listen-vfkit".to_owned(),
            format!("unixgram://{}", socket_path.display()),
        ]
    }

    fn build_launch_command(
        &self,
        vmm_binary: &Path,
        ctx: &VmmLaunchContext<'_>,
    ) -> Result<MachineCommandLine, Error> {
        let paths = ctx.paths;
        let mut args = vec![
            "--cpus".to_owned(),
            ctx.config.resources.cpus.to_string(),
            "--memory".to_owned(),
            ctx.config.resources.memory_mib.to_string(),
            "--bootloader".to_owned(),
            format!(
                "efi,variable-store={},create",
                ctx.efi_variable_store_path.display()
            ),
            "--restful-uri".to_owned(),
            ctx.rest_uri.to_owned(),
            "--pidfile".to_owned(),
            paths.krunkit_pid_path.display().to_string(),
            "--log-file".to_owned(),
            paths.krunkit_log_path.display().to_string(),
            "--device".to_owned(),
            format!("virtio-blk,path={},format=raw", ctx.image_path.display()),
            "--device".to_owned(),
            format!(
                "virtio-net,type=unixgram,path={},mac={},offloading=on,vfkitMagic=on",
                paths.gvproxy_socket_path.display(),
                DEFAULT_MACHINE_MAC_ADDRESS
            ),
            "--device".to_owned(),
            format!(
                "virtio-serial,logFilePath={}",
                paths.machine_log_path.display()
            ),
        ];
        if matches!(
            ctx.bootstrap_mode,
            MachineBootstrapMode::Ignition | MachineBootstrapMode::BootcMachineConfig
        ) {
            args.extend([
                "--device".to_owned(),
                build_virtio_vsock_listen_arg(READY_VSOCK_PORT, &paths.ready_socket_path),
            ]);
        }
        if ctx.bootstrap_mode == MachineBootstrapMode::Ignition {
            args.extend([
                "--device".to_owned(),
                build_virtio_vsock_listen_arg(1024, &paths.ignition_socket_path),
            ]);
        }
        if let Some(bundle_dir) = ctx.machine_config_bundle_dir {
            args.extend([
                "--device".to_owned(),
                build_virtiofs_arg(bundle_dir, GUEST_MACHINE_CONFIG_MOUNT_TAG),
            ]);
        }
        args.extend(
            ctx.config
                .volumes
                .iter()
                .flat_map(build_virtiofs_args)
                .collect::<Vec<_>>(),
        );

        Ok(MachineCommandLine {
            program: vmm_binary.to_path_buf(),
            args,
        })
    }
}
