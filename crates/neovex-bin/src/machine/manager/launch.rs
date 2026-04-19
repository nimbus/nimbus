use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use neovex::Error;

use super::super::bootstrap::{GUEST_NEOVEX_SOCKET, resolve_ignition_file};
use super::super::{
    MachineBootstrapMode, MachineConfigRecord, MachinePaths, MachineStateRecord, MachineVolume,
    describe_machine_image_source,
};
use super::image::resolve_bootable_image_path;
use super::ports::allocate_machine_ssh_port;
use super::{
    DEFAULT_MACHINE_MAC_ADDRESS, MACHINE_API_FORWARD_USER, MachineHelperBinaryPaths,
    MachineRuntimeState, READY_VSOCK_PORT, mount_tag,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineLaunchPlan {
    pub(super) runtime: MachineRuntimeState,
    pub(super) gvproxy_command: MachineCommandLine,
    pub(super) krunkit_command: MachineCommandLine,
    pub(super) ignition_file_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineCommandLine {
    pub(super) program: PathBuf,
    pub(super) args: Vec<String>,
}

impl MachineCommandLine {
    pub(super) fn spawn(&self) -> Result<Child, Error> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
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
        let helper_binaries = MachineHelperBinaryPaths::resolve()?;
        let image_path =
            resolve_bootable_image_path(paths, &config.guest.image_source, config.provider)?;
        let ignition_file_path = match config.provider.bootstrap_mode() {
            MachineBootstrapMode::Ignition => {
                Some(resolve_ignition_file(paths, config, READY_VSOCK_PORT)?)
            }
            MachineBootstrapMode::ShellScript => None,
        };
        let ssh_port = allocate_machine_ssh_port(&config.roots, &config.name, state)?;
        let rest_uri = format!("unix://{}", paths.krunkit_endpoint_path.display());
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

        let gvproxy_command = MachineCommandLine {
            program: helper_binaries.gvproxy.clone(),
            args: build_gvproxy_args(paths, config, ssh_port),
        };

        let mut krunkit_args = vec![
            "--cpus".to_owned(),
            config.resources.cpus.to_string(),
            "--memory".to_owned(),
            config.resources.memory_mib.to_string(),
            "--bootloader".to_owned(),
            format!(
                "efi,variable-store={},create",
                runtime.efi_variable_store_path.display()
            ),
            "--restful-uri".to_owned(),
            rest_uri,
            "--pidfile".to_owned(),
            paths.krunkit_pid_path.display().to_string(),
            "--log-file".to_owned(),
            paths.krunkit_log_path.display().to_string(),
            "--device".to_owned(),
            format!("virtio-blk,path={},format=raw", image_path.display()),
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
        if config.provider.bootstrap_mode() == MachineBootstrapMode::Ignition {
            krunkit_args.extend([
                "--device".to_owned(),
                build_virtio_vsock_listen_arg(READY_VSOCK_PORT, &paths.ready_socket_path),
                "--device".to_owned(),
                build_virtio_vsock_listen_arg(1024, &paths.ignition_socket_path),
            ]);
        }
        krunkit_args.extend(
            config
                .volumes
                .iter()
                .flat_map(build_virtiofs_args)
                .collect::<Vec<_>>(),
        );

        let krunkit_command = MachineCommandLine {
            program: helper_binaries.krunkit.clone(),
            args: krunkit_args,
        };

        Ok(Self {
            runtime,
            gvproxy_command,
            krunkit_command,
            ignition_file_path,
        })
    }

    pub(super) fn runtime(&self) -> &MachineRuntimeState {
        &self.runtime
    }
}

fn build_gvproxy_args(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    ssh_port: u16,
) -> Vec<String> {
    let mut args = vec![
        "-listen-vfkit".to_owned(),
        format!("unixgram://{}", paths.gvproxy_socket_path.display()),
        "-pid-file".to_owned(),
        paths.gvproxy_pid_path.display().to_string(),
        "-log-file".to_owned(),
        paths.gvproxy_log_path.display().to_string(),
        "-ssh-port".to_owned(),
        ssh_port.to_string(),
    ];

    if let Some(identity_path) = config.guest.ssh_identity_path.as_ref() {
        // Match Podman's machine-plumbing shape: gvproxy owns the host-local
        // forwarded socket and reaches the guest system socket over SSH.
        // The guest machine API lives at /run/neovex/neovex.sock, so we
        // forward as root rather than the interactive SSH user.
        args.extend([
            "-forward-sock".to_owned(),
            paths.api_socket_path.display().to_string(),
            "-forward-dest".to_owned(),
            GUEST_NEOVEX_SOCKET.to_owned(),
            "-forward-user".to_owned(),
            MACHINE_API_FORWARD_USER.to_owned(),
            "-forward-identity".to_owned(),
            identity_path.display().to_string(),
        ]);
    }

    args
}

pub(super) fn build_virtio_vsock_listen_arg(port: u32, socket_path: &Path) -> String {
    // Match Podman's vfkit/libkrun contract: the host owns these Unix sockets
    // and krunkit must connect the guest-side vsock device to that listener.
    format!(
        "virtio-vsock,port={port},socketURL={},listen",
        socket_path.display()
    )
}

fn build_virtiofs_args(volume: &MachineVolume) -> Vec<String> {
    vec![
        "--device".to_owned(),
        format!(
            "virtio-fs,sharedDir={},mountTag={}",
            volume.source.display(),
            mount_tag(&volume.target)
        ),
    ]
}
