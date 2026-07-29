use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use nimbus::Error;
use nimbus_machine::MachineForwarderAuthority;
use nimbus_network::{LocalPortLeaseAuthority, NetworkResourceGeneration};

use super::super::bootstrap::resolve_ignition_file;
use super::super::guest_config::render_machine_config_bundle;
use super::super::{
    MachineBootstrapMode, MachineConfigRecord, MachinePaths, MachineStateRecord, MachineVolume,
    describe_machine_image_source, machine_bootstrap_mode,
};
use super::helper_paths::resolve_gvproxy_binary;
use super::image::resolve_bootable_image_path;
use super::ports::PreparedMachineSshPortLease;
use super::vmm::{MachineVmmBackend, VmmLaunchContext, vmm_backend};
use super::{MachineHelperBinaryPaths, MachineRuntimeState, READY_VSOCK_PORT, mount_tag};

#[derive(Debug)]
pub(super) struct MachineLaunchPlan {
    pub(super) runtime: MachineRuntimeState,
    pub(super) ssh_port_lease: PreparedMachineSshPortLease,
    /// The gvproxy user-mode network helper command required by every backend
    /// admitted through the current host-managed VMM seam.
    ///
    /// Provider-managed networking is a different lifecycle mode and is
    /// rejected by [`vmm_backend`] before this plan can reserve a host lease.
    pub(super) gvproxy_command: MachineCommandLine,
    pub(super) vmm_command: MachineCommandLine,
    pub(super) ignition_file_path: Option<PathBuf>,
    #[cfg(test)]
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
        port_authority: &LocalPortLeaseAuthority,
        paths: &MachinePaths,
        config: &MachineConfigRecord,
        state: &MachineStateRecord,
    ) -> Result<Self, Error> {
        let backend = vmm_backend(config.provider)?;
        if backend.provider() != config.provider {
            return Err(Error::Internal(format!(
                "machine backend resolver returned {:?} for requested provider {:?}",
                backend.provider(),
                config.provider
            )));
        }
        let forwarder_authority = next_machine_forwarder_authority(config, state)?;
        let helper_binaries = MachineHelperBinaryPaths {
            vmm: backend.resolve_vmm_binary()?,
            gvproxy: resolve_gvproxy_binary()?,
        };
        let image_path =
            resolve_bootable_image_path(paths, &config.guest.image_source, config.provider)?;
        let bootstrap_mode = machine_bootstrap_mode(config);
        let ignition_file_path = match bootstrap_mode {
            MachineBootstrapMode::Ignition => Some(resolve_ignition_file(
                paths,
                config,
                READY_VSOCK_PORT,
                &forwarder_authority,
            )?),
            MachineBootstrapMode::BootcMachineConfig | MachineBootstrapMode::ShellScript => None,
        };
        let machine_config_bundle_dir = match bootstrap_mode {
            MachineBootstrapMode::BootcMachineConfig => Some(render_machine_config_bundle(
                paths,
                config,
                READY_VSOCK_PORT,
                &forwarder_authority,
            )?),
            MachineBootstrapMode::ShellScript => None,
            MachineBootstrapMode::Ignition => None,
        };
        let efi_variable_store_path = config
            .guest
            .efi_variable_store_path
            .clone()
            .unwrap_or_else(|| paths.efi_variable_store_path.clone());
        let rest_uri = format!("unix://{}", paths.vmm_endpoint_path.display());
        let vmm_command = backend.build_launch_command(
            &helper_binaries.vmm,
            &VmmLaunchContext {
                paths,
                config,
                image_path: &image_path,
                efi_variable_store_path: &efi_variable_store_path,
                rest_uri: &rest_uri,
                bootstrap_mode,
                machine_config_bundle_dir: machine_config_bundle_dir.as_deref(),
            },
        )?;

        // Reserve and claim only after every fallible pure/provider-command
        // preparation step has succeeded. From this point onward the plan is
        // assembled without another fallible operation, so a returned claim
        // always reaches the explicit pre-provider compensation boundary in
        // `start_machine`.
        let ssh_port_lease =
            PreparedMachineSshPortLease::prepare(port_authority.clone(), &config.name, state)?;
        let ssh_port = ssh_port_lease.selected_port();
        let runtime = MachineRuntimeState {
            helper_binaries: helper_binaries.clone(),
            image_path: image_path.clone(),
            efi_variable_store_path,
            machine_image_source: describe_machine_image_source(&config.guest.image_source),
            ssh_listener_id: ssh_port_lease.listener_id().clone(),
            forwarder_authority,
            ssh_port,
            rest_uri: rest_uri.clone(),
            ready_vsock_port: READY_VSOCK_PORT,
        };

        // Every backend admitted by `vmm_backend` currently uses Nimbus-owned
        // host networking. Keep that state structural: a valid launch plan
        // always carries the gvproxy command corresponding to its claimed SSH
        // listener. Provider-managed networking must enter through its own
        // lifecycle seam rather than accidentally combining a host lease with
        // no host networking effect.
        let gvproxy_command = MachineCommandLine {
            program: helper_binaries.gvproxy.clone(),
            args: build_gvproxy_args(backend.as_ref(), paths, ssh_port),
            // gvproxy writes its own `-log-file`; no stdout+stderr capture needed.
            capture_log_path: None,
        };

        Ok(Self {
            runtime,
            ssh_port_lease,
            gvproxy_command,
            vmm_command,
            ignition_file_path,
            #[cfg(test)]
            machine_config_bundle_dir,
        })
    }

    pub(super) fn runtime(&self) -> &MachineRuntimeState {
        &self.runtime
    }

    pub(super) fn ssh_port_lease(&self) -> &PreparedMachineSshPortLease {
        &self.ssh_port_lease
    }
}

fn next_machine_forwarder_authority(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<MachineForwarderAuthority, Error> {
    let provider_instance = config.network_authority.provider_instance();
    let generation = match state.runtime.as_ref() {
        None => NetworkResourceGeneration::new(1),
        Some(runtime) => {
            if runtime.forwarder_authority.provider_instance() != provider_instance {
                return Err(Error::conflict(format!(
                    "machine '{}' persisted runtime belongs to a different parent-issued \
                     forwarder provider",
                    config.name
                )));
            }
            runtime
                .forwarder_authority
                .generation()
                .checked_next()
                .ok_or_else(|| {
                    Error::conflict(format!(
                        "machine '{}' exhausted its forwarder provider generation",
                        config.name
                    ))
                })?
        }
    };
    Ok(MachineForwarderAuthority::new(
        provider_instance.clone(),
        generation,
    ))
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
