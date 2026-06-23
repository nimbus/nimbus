//! Per-provider VMM backend seam.
//!
//! A [`MachineVmmBackend`] owns everything that differs between the macOS
//! micro-VM monitors Nimbus can drive: which VMM binary to resolve, how to
//! construct its launch command line, whether it needs the external gvproxy
//! user-mode network stack, and the gvproxy listen-mode ↔ on-VMM net-device
//! pairing. The lifecycle in `manager.rs`/`readiness.rs`/`stop.rs` stays
//! provider-agnostic and talks to the resolved backend through this trait.
//!
//! krunkit (libkrun) and vfkit (Apple Virtualization.framework) are the two
//! implemented macOS backends. Both drive the same Nimbus-managed `applehv`
//! guest over EFI and attach gvproxy through a unixgram socket; they differ only
//! in the block- and net-device grammar and in diagnostics: krunkit writes its
//! own `--log-file`, while vfkit (no such flag) has the spawn path capture its
//! stdout+stderr into `vmm_log_path` instead, so failed-boot triage works for
//! both. wsl2 resolves to an explicit "not available yet"
//! error so selection stays a deliberate, fail-closed opt-in rather than silent
//! auto-detection.

use std::path::{Path, PathBuf};

use nimbus::Error;

use super::super::guest_config::GUEST_MACHINE_CONFIG_MOUNT_TAG;
use super::super::{MachineBootstrapMode, MachineConfigRecord, MachinePaths, MachineProvider};
use super::helpers::{bundled_helper_candidates, known_helper_candidates, resolve_helper_binary};
use super::launch::{
    MachineCommandLine, build_virtio_vsock_listen_arg, build_virtiofs_arg, build_virtiofs_args,
};
use super::{
    DEFAULT_KRUNKIT_BINARY, DEFAULT_MACHINE_MAC_ADDRESS, DEFAULT_VFKIT_BINARY, KRUNKIT_ENV,
    READY_VSOCK_PORT, VFKIT_ENV,
};

/// Ignition's well-known guest vsock port. The host serves the Ignition payload
/// on a listening Unix socket and the guest dials this port to fetch it; both
/// applehv backends wire the same device.
const IGNITION_VSOCK_PORT: u32 = 1024;

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
    /// Both macOS micro-VM monitors share the unixgram listen contract (gvproxy
    /// `-listen-vfkit unixgram://…`); only the on-VMM net-device syntax in
    /// [`build_launch_command`](Self::build_launch_command) differs, so the
    /// default suits every applehv backend. A future backend with a different
    /// host transport overrides this.
    fn gvproxy_listen_args(&self, socket_path: &Path) -> Vec<String> {
        gvproxy_unixgram_listen_args(socket_path)
    }

    /// Construct the VMM launch command line for the already-resolved binary.
    fn build_launch_command(
        &self,
        vmm_binary: &Path,
        ctx: &VmmLaunchContext<'_>,
    ) -> Result<MachineCommandLine, Error>;
}

/// Resolve the VMM backend for a provider. krunkit and vfkit are implemented;
/// wsl2 fails closed with an explicit message until its backend lands. This is
/// the single provider gate on the start path — `MachineLaunchPlan::build` calls
/// it before any VMM or gvproxy process is spawned, so an unsupported provider
/// never partially starts a machine.
pub(super) fn vmm_backend(provider: MachineProvider) -> Result<Box<dyn MachineVmmBackend>, Error> {
    match provider {
        MachineProvider::Krunkit => Ok(Box::new(KrunkitVmmBackend)),
        MachineProvider::Vfkit => Ok(Box::new(VfkitVmmBackend)),
        MachineProvider::Wsl2 => Err(provider.unavailable_error()),
    }
}

/// The gvproxy listen-mode arguments both applehv backends share. gvproxy
/// listens in vfkit unixgram mode (`-listen-vfkit unixgram://<sock>`) for
/// krunkit and vfkit alike — krunkit reuses vfkit's host transport, so only the
/// on-VMM net-device grammar (`KrunkitVmmBackend` vs `VfkitVmmBackend`) differs.
fn gvproxy_unixgram_listen_args(socket_path: &Path) -> Vec<String> {
    vec![
        "-listen-vfkit".to_owned(),
        format!("unixgram://{}", socket_path.display()),
    ]
}

/// The leading VMM arguments that krunkit and vfkit share verbatim: CPU/memory
/// sizing, the EFI bootloader pointed at the machine's variable store, the
/// restful control endpoint, and the pidfile slot the readiness/stop lifecycle
/// watches.
fn base_vmm_args(ctx: &VmmLaunchContext<'_>) -> Vec<String> {
    vec![
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
        ctx.paths.vmm_pid_path.display().to_string(),
    ]
}

/// Append the device arguments that every applehv backend wires identically: the
/// guest console serial log, the bootstrap vsock listeners (machine-ready and,
/// for Ignition, the Ignition payload), the machine-config virtiofs bundle, and
/// the user volumes. krunkit and vfkit speak the same grammar for these devices;
/// only the preceding block and net devices differ between them.
fn push_shared_applehv_devices(args: &mut Vec<String>, ctx: &VmmLaunchContext<'_>) {
    let paths = ctx.paths;
    args.extend([
        "--device".to_owned(),
        format!(
            "virtio-serial,logFilePath={}",
            paths.machine_log_path.display()
        ),
    ]);
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
            build_virtio_vsock_listen_arg(IGNITION_VSOCK_PORT, &paths.ignition_socket_path),
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

    fn build_launch_command(
        &self,
        vmm_binary: &Path,
        ctx: &VmmLaunchContext<'_>,
    ) -> Result<MachineCommandLine, Error> {
        let paths = ctx.paths;
        let mut args = base_vmm_args(ctx);
        // krunkit exposes its own diagnostic log; vfkit does not.
        args.extend([
            "--log-file".to_owned(),
            paths.vmm_log_path.display().to_string(),
        ]);
        args.extend([
            "--device".to_owned(),
            format!("virtio-blk,path={},format=raw", ctx.image_path.display()),
            "--device".to_owned(),
            format!(
                "virtio-net,type=unixgram,path={},mac={},offloading=on,vfkitMagic=on",
                paths.gvproxy_socket_path.display(),
                DEFAULT_MACHINE_MAC_ADDRESS
            ),
        ]);
        push_shared_applehv_devices(&mut args, ctx);

        Ok(MachineCommandLine {
            program: vmm_binary.to_path_buf(),
            args,
            // krunkit already writes its diagnostic output to `--log-file`
            // (set above to vmm_log_path); redirecting its stdout+stderr into the
            // same file would duplicate every line, so leave capture off.
            capture_log_path: None,
        })
    }
}

/// vfkit: the Apple Virtualization.framework macOS micro-VM monitor. Boots the
/// same Nimbus-managed `applehv` disk over EFI as krunkit, but speaks the vfkit
/// device grammar: a bare `virtio-blk,path=` block device (no `format=`) and a
/// `virtio-net,unixSocketPath=` net device dialing the gvproxy `--listen-vfkit`
/// unixgram socket. vfkit has no `--log-file`; the guest console still lands in
/// the shared serial log.
pub(super) struct VfkitVmmBackend;

impl MachineVmmBackend for VfkitVmmBackend {
    fn provider(&self) -> MachineProvider {
        MachineProvider::Vfkit
    }

    fn resolve_vmm_binary(&self) -> Result<PathBuf, Error> {
        // vfkit is bundled in the Nimbus archive (pinned, signed + notarized) and
        // is also installable via `brew install vfkit`, so resolution prefers the
        // bundled `libexec` copy (and the `NIMBUS_MACHINE_VFKIT` override) before
        // falling back to the known Homebrew/Podman helper directories.
        resolve_helper_binary(
            VFKIT_ENV,
            DEFAULT_VFKIT_BINARY,
            &bundled_helper_candidates(DEFAULT_VFKIT_BINARY),
            &known_helper_candidates(DEFAULT_VFKIT_BINARY),
        )
    }

    fn build_launch_command(
        &self,
        vmm_binary: &Path,
        ctx: &VmmLaunchContext<'_>,
    ) -> Result<MachineCommandLine, Error> {
        let paths = ctx.paths;
        let mut args = base_vmm_args(ctx);
        args.extend([
            "--device".to_owned(),
            format!("virtio-blk,path={}", ctx.image_path.display()),
            "--device".to_owned(),
            format!(
                "virtio-net,unixSocketPath={},mac={}",
                paths.gvproxy_socket_path.display(),
                DEFAULT_MACHINE_MAC_ADDRESS
            ),
        ]);
        push_shared_applehv_devices(&mut args, ctx);

        Ok(MachineCommandLine {
            program: vmm_binary.to_path_buf(),
            args,
            // vfkit has no `--log-file`, so a boot that dies before the guest
            // console comes up would otherwise leave nothing to triage. Capture
            // its stdout+stderr into vmm_log_path so failed-boot diagnostics are
            // recoverable.
            capture_log_path: Some(paths.vmm_log_path.clone()),
        })
    }
}
