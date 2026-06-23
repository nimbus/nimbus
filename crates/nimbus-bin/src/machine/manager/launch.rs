use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use nimbus::Error;

use super::super::bootstrap::resolve_ignition_file;
use super::super::guest_config::render_machine_config_bundle;
use super::super::{
    MachineBootstrapMode, MachineConfigRecord, MachinePaths, MachineStateRecord, MachineVolume,
    describe_machine_image_source, machine_bootstrap_mode,
};
use super::helpers::resolve_gvproxy_binary;
use super::image::resolve_bootable_image_path;
use super::ports::allocate_machine_ssh_port;
use super::vmm::{MachineVmmBackend, VmmLaunchContext, vmm_backend};
use super::{MachineHelperBinaryPaths, MachineRuntimeState, READY_VSOCK_PORT, mount_tag};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineLaunchPlan {
    pub(super) runtime: MachineRuntimeState,
    /// The gvproxy user-mode network helper command, present only for providers
    /// whose backend [`requires_gvproxy`](MachineVmmBackend::requires_gvproxy).
    /// Provider-managed networking backends leave this `None`.
    pub(super) gvproxy_command: Option<MachineCommandLine>,
    pub(super) vmm_command: MachineCommandLine,
    pub(super) ignition_file_path: Option<PathBuf>,
    pub(super) machine_config_bundle_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineCommandLine {
    pub(super) program: PathBuf,
    pub(super) args: Vec<String>,
    /// Where to capture the child's stdout+stderr for failed-boot triage.
    ///
    /// A VMM that writes its own diagnostic log (krunkit via `--log-file`)
    /// leaves this `None` so its streams are not also redirected into the same
    /// file and duplicated. A VMM with no diagnostic-log flag (vfkit) sets this
    /// to its [`vmm_log_path`](MachinePaths::vmm_log_path) so a boot that dies
    /// before the guest console comes up still leaves captured output to triage.
    /// `None` discards both streams (`/dev/null`), matching the gvproxy helper,
    /// which logs through its own `-log-file`.
    pub(super) capture_log_path: Option<PathBuf>,
}

impl MachineCommandLine {
    pub(super) fn spawn(&self) -> Result<Child, Error> {
        let mut command = Command::new(&self.program);
        command.args(&self.args).stdin(Stdio::null());
        match self.capture_log_path.as_deref() {
            // The VMM has no diagnostic log of its own, so capture its
            // stdout+stderr into the provider's vmm_log_path. Append (never
            // truncate) so an earlier failed boot's output is not clobbered by a
            // restart, and clone the handle so both streams land in one file
            // without interleaving a separately-opened descriptor.
            Some(log_path) => {
                let stdout = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_path)
                    .map_err(|error| {
                        Error::Internal(format!(
                            "failed to open VMM log {} for stdout+stderr capture: {error}",
                            log_path.display()
                        ))
                    })?;
                let stderr = stdout.try_clone().map_err(|error| {
                    Error::Internal(format!(
                        "failed to clone VMM log {} for stdout+stderr capture: {error}",
                        log_path.display()
                    ))
                })?;
                command
                    .stdout(Stdio::from(stdout))
                    .stderr(Stdio::from(stderr));
            }
            // No capture file: the helper writes its own log (krunkit
            // `--log-file`, gvproxy `-log-file`), so discard both streams rather
            // than double-logging the same output into a second sink.
            None => {
                command.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        #[cfg(unix)]
        // SAFETY: `pre_exec` runs in the child process after fork and before
        // exec. The closure only invokes `setsid`, an async-signal-safe libc
        // call that does not touch shared Rust state; the returned OS error is
        // immediately propagated to abort the child launch if session creation
        // fails.
        unsafe {
            // Machine helpers should survive the launching CLI process exiting.
            // Put them in their own session so host validation and normal shell
            // use do not depend on the parent process group remaining alive.
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.spawn().map_err(|error| {
            Error::Internal(format!(
                "failed to start {}: {error}",
                self.program.display()
            ))
        })
    }
}

impl MachineLaunchPlan {
    pub(super) fn build(
        paths: &MachinePaths,
        config: &MachineConfigRecord,
        state: &MachineStateRecord,
    ) -> Result<Self, Error> {
        let backend = vmm_backend(config.provider)?;
        let helper_binaries = MachineHelperBinaryPaths {
            vmm: backend.resolve_vmm_binary()?,
            gvproxy: resolve_gvproxy_binary()?,
        };
        let image_path =
            resolve_bootable_image_path(paths, &config.guest.image_source, config.provider)?;
        let bootstrap_mode = machine_bootstrap_mode(config);
        let ignition_file_path = match bootstrap_mode {
            MachineBootstrapMode::Ignition => {
                Some(resolve_ignition_file(paths, config, READY_VSOCK_PORT)?)
            }
            MachineBootstrapMode::BootcMachineConfig | MachineBootstrapMode::ShellScript => None,
        };
        let machine_config_bundle_dir = match bootstrap_mode {
            MachineBootstrapMode::BootcMachineConfig => Some(render_machine_config_bundle(
                paths,
                config,
                READY_VSOCK_PORT,
            )?),
            MachineBootstrapMode::ShellScript => None,
            MachineBootstrapMode::Ignition => None,
        };
        let ssh_port = allocate_machine_ssh_port(&config.roots, &config.name, state)?;
        let rest_uri = format!("unix://{}", paths.vmm_endpoint_path.display());
        let runtime = MachineRuntimeState {
            helper_binaries: helper_binaries.clone(),
            image_path: image_path.clone(),
            efi_variable_store_path: config
                .guest
                .efi_variable_store_path
                .clone()
                .unwrap_or_else(|| paths.efi_variable_store_path.clone()),
            machine_image_source: describe_machine_image_source(&config.guest.image_source),
            ssh_port,
            rest_uri: rest_uri.clone(),
            ready_vsock_port: READY_VSOCK_PORT,
        };

        // The backend owns whether host networking flows through gvproxy. Only
        // build (and later spawn) the helper when it does; provider-managed
        // networking backends run without it.
        let gvproxy_command = backend.requires_gvproxy().then(|| MachineCommandLine {
            program: helper_binaries.gvproxy.clone(),
            args: build_gvproxy_args(backend.as_ref(), paths, ssh_port),
            // gvproxy writes its own `-log-file`; no stdout+stderr capture needed.
            capture_log_path: None,
        });

        let vmm_command = backend.build_launch_command(
            &helper_binaries.vmm,
            &VmmLaunchContext {
                paths,
                config,
                image_path: &image_path,
                efi_variable_store_path: &runtime.efi_variable_store_path,
                rest_uri: &rest_uri,
                bootstrap_mode,
                machine_config_bundle_dir: machine_config_bundle_dir.as_deref(),
            },
        )?;

        Ok(Self {
            runtime,
            gvproxy_command,
            vmm_command,
            ignition_file_path,
            machine_config_bundle_dir,
        })
    }

    pub(super) fn runtime(&self) -> &MachineRuntimeState {
        &self.runtime
    }
}

fn build_gvproxy_args(
    backend: &dyn MachineVmmBackend,
    paths: &MachinePaths,
    ssh_port: u16,
) -> Vec<String> {
    // The backend owns the listen-mode ↔ VMM attachment pairing; the pid/log/ssh
    // forwarding flags are shared across every gvproxy-backed provider.
    let mut args = backend.gvproxy_listen_args(&paths.gvproxy_socket_path);
    args.extend([
        "-pid-file".to_owned(),
        paths.gvproxy_pid_path.display().to_string(),
        "-log-file".to_owned(),
        paths.gvproxy_log_path.display().to_string(),
        "-ssh-port".to_owned(),
        ssh_port.to_string(),
    ]);
    args
}

pub(super) fn build_virtio_vsock_listen_arg(port: u32, socket_path: &Path) -> String {
    // Match Podman's vfkit/libkrun contract: the host owns these Unix sockets
    // and the VMM must connect the guest-side vsock device to that listener.
    format!(
        "virtio-vsock,port={port},socketURL={},listen",
        socket_path.display()
    )
}

pub(super) fn build_virtiofs_args(volume: &MachineVolume) -> Vec<String> {
    vec![
        "--device".to_owned(),
        build_virtiofs_arg(&volume.source, &mount_tag(&volume.target)),
    ]
}

pub(super) fn build_virtiofs_arg(source: &Path, tag: &str) -> String {
    format!("virtio-fs,sharedDir={},mountTag={}", source.display(), tag)
}
