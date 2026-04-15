use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use flate2::read::GzDecoder;
use libc::{SIGKILL, SIGTERM, kill};
use neovex::Error;
use oci_client::Reference;
use oci_client::client::{Client as OciClient, ClientConfig as OciClientConfig, ClientProtocol};
use oci_client::manifest::{
    IMAGE_MANIFEST_LIST_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE, OCI_IMAGE_INDEX_MEDIA_TYPE,
    OCI_IMAGE_MEDIA_TYPE, OciDescriptor,
};
use oci_client::secrets::RegistryAuth;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;

use super::bootstrap::{GUEST_NEOVEX_SOCKET, resolve_ignition_file};
use super::{
    MachineConfigRecord, MachineImageSource, MachineLifecycle, MachineManagerState, MachinePaths,
    MachineStateRecord, MachineVolume, write_json_file,
};

const DEFAULT_KRUNKIT_BINARY: &str = "krunkit";
const DEFAULT_KRUNKIT_HOMEBREW_PATH: &str = "/opt/homebrew/bin/krunkit";
const DEFAULT_GVPROXY_BINARY: &str = "gvproxy";
const DEFAULT_GVPROXY_HOMEBREW_PATH: &str = "/opt/homebrew/opt/podman/libexec/podman/gvproxy";
const DEFAULT_GVPROXY_INTEL_HOMEBREW_PATH: &str = "/usr/local/opt/podman/libexec/podman/gvproxy";
const DEFAULT_MACHINE_MAC_ADDRESS: &str = "5a:94:ef:e4:0c:ee";
const READY_VSOCK_PORT: u32 = 1025;
const READY_WAIT_TIMEOUT_ENV: &str = "NEOVEX_MACHINE_READY_TIMEOUT_SECS";
const DEFAULT_READY_WAIT_TIMEOUT: Duration = Duration::from_secs(90);
const SSH_READY_WAIT_TIMEOUT_ENV: &str = "NEOVEX_MACHINE_SSH_READY_TIMEOUT_SECS";
const DEFAULT_SSH_READY_WAIT_TIMEOUT: Duration = Duration::from_secs(30);
const GVPROXY_SOCKET_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
const STOP_WAIT_TIMEOUT: Duration = Duration::from_secs(20);
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const KRUNKIT_ENV: &str = "NEOVEX_MACHINE_KRUNKIT";
const GVPROXY_ENV: &str = "NEOVEX_MACHINE_GVPROXY";
const HTTP_IMAGE_TIMEOUT: Duration = Duration::from_secs(300);
const OCI_MACHINE_DISK_TYPE: &str = "raw";
const OCI_MACHINE_OS: &str = "linux";
const OCI_ANNOTATION_TITLE: &str = "org.opencontainers.image.title";
const OCI_ANNOTATION_SOURCE: &str = "org.opencontainers.image.source";
const OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY: &str =
    "io.neovex.machine.attestation.repository";
const OCI_ANNOTATION_MACHINE_NEOVEX_VERSION: &str = "io.neovex.machine.neovex.version";
pub(super) const MACHINE_API_FORWARD_TRANSPORT: &str = "gvproxy-ssh-forwarded-unix-socket";
pub(super) const MACHINE_API_FORWARD_USER: &str = "root";

#[derive(Debug, Deserialize)]
struct RegistryImageIndex {
    manifests: Vec<RegistryManifestDescriptor>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryManifestDescriptor {
    digest: String,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
    platform: Option<RegistryPlatform>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryPlatform {
    architecture: String,
    os: String,
}

#[derive(Debug, Deserialize)]
struct RegistryImageManifest {
    layers: Vec<RegistryLayerDescriptor>,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryLayerDescriptor {
    digest: String,
    size: i64,
    #[serde(rename = "mediaType")]
    media_type: String,
    #[serde(default)]
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct MachineArtifactMetadata {
    attestation_repository: Option<String>,
    source_repository_url: Option<String>,
    neovex_version: Option<String>,
}

#[derive(Debug, Clone)]
struct SelectedMachineArtifact {
    child_reference: Reference,
    layer: RegistryLayerDescriptor,
    metadata: MachineArtifactMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineRuntimeState {
    pub(super) helper_binaries: MachineHelperBinaryPaths,
    pub(super) image_path: PathBuf,
    pub(super) efi_variable_store_path: PathBuf,
    pub(super) ssh_port: u16,
    pub(super) rest_uri: String,
    pub(super) ready_vsock_port: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MachineHelperBinaryPaths {
    pub(super) krunkit: PathBuf,
    pub(super) gvproxy: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MachineLaunchPlan {
    runtime: MachineRuntimeState,
    gvproxy_command: MachineCommandLine,
    krunkit_command: MachineCommandLine,
    ignition_file_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MachineCommandLine {
    program: PathBuf,
    args: Vec<String>,
}

impl MachineCommandLine {
    fn spawn(&self) -> Result<Child, Error> {
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

impl MachineHelperBinaryPaths {
    pub(super) fn resolve() -> Result<Self, Error> {
        Ok(Self {
            krunkit: resolve_helper_binary(
                KRUNKIT_ENV,
                DEFAULT_KRUNKIT_BINARY,
                &[PathBuf::from(DEFAULT_KRUNKIT_HOMEBREW_PATH)],
            )?,
            gvproxy: resolve_helper_binary(
                GVPROXY_ENV,
                DEFAULT_GVPROXY_BINARY,
                &[
                    PathBuf::from(DEFAULT_GVPROXY_HOMEBREW_PATH),
                    PathBuf::from(DEFAULT_GVPROXY_INTEL_HOMEBREW_PATH),
                ],
            )?,
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
        let image_path = resolve_bootable_image_path(paths, &config.guest.image_source)?;
        let ignition_file_path = resolve_ignition_file(paths, config, READY_VSOCK_PORT)?;
        let ssh_port = match state.runtime.as_ref() {
            Some(runtime) => runtime.ssh_port,
            None => allocate_local_port().map_err(|error| {
                Error::Internal(format!("failed to allocate localhost ssh port: {error}"))
            })?,
        };
        let rest_uri = format!("unix://{}", paths.krunkit_endpoint_path.display());
        let runtime = MachineRuntimeState {
            helper_binaries: helper_binaries.clone(),
            image_path: image_path.clone(),
            efi_variable_store_path: config
                .guest
                .efi_variable_store_path
                .clone()
                .unwrap_or_else(|| paths.efi_variable_store_path.clone()),
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
                "virtio-vsock,port={},socketURL={}",
                READY_VSOCK_PORT,
                paths.ready_socket_path.display()
            ),
            "--device".to_owned(),
            format!(
                "virtio-serial,logFilePath={}",
                paths.machine_log_path.display()
            ),
        ];
        krunkit_args.extend([
            "--device".to_owned(),
            format!(
                "virtio-vsock,port=1024,socketURL={}",
                paths.ignition_socket_path.display()
            ),
        ]);
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
            ignition_file_path: Some(ignition_file_path),
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

pub(super) fn start_machine(
    paths: &MachinePaths,
    config: &MachineConfigRecord,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    if matches!(
        state.lifecycle,
        MachineLifecycle::Starting | MachineLifecycle::Running
    ) {
        return Err(Error::Conflict(format!(
            "machine '{}' is already {}",
            paths.name,
            state.lifecycle.as_str()
        )));
    }

    cleanup_runtime_artifacts(paths)?;
    let launch_plan = MachineLaunchPlan::build(paths, config, state)?;

    state.lifecycle = MachineLifecycle::Starting;
    state.manager = MachineManagerState::Launching;
    state.runtime = Some(launch_plan.runtime().clone());
    state.last_error = None;
    write_json_file(&paths.state_path, state)?;

    let ready_listener = bind_ready_listener(&paths.ready_socket_path)?;
    let _ignition_server = match launch_plan.ignition_file_path.as_ref() {
        Some(path) => Some(serve_ignition_file(&paths.ignition_socket_path, path)?),
        None => None,
    };
    let mut gvproxy_child = launch_plan.gvproxy_command.spawn()?;
    if let Err(error) = wait_for_path(
        &paths.gvproxy_socket_path,
        GVPROXY_SOCKET_WAIT_TIMEOUT,
        &mut gvproxy_child,
    ) {
        let _ = cleanup_process(&mut gvproxy_child);
        state.lifecycle = MachineLifecycle::Failed;
        state.manager = MachineManagerState::Failed;
        state.last_error = Some(error.to_string());
        write_json_file(&paths.state_path, state)?;
        return Err(error);
    }

    let mut krunkit_child = launch_plan.krunkit_command.spawn()?;
    match wait_for_ready(
        &ready_listener,
        resolve_ready_wait_timeout(),
        &mut krunkit_child,
        &mut gvproxy_child,
    ) {
        Ok(()) => {
            if let Err(error) = wait_for_ssh_ready(
                config,
                launch_plan.runtime().ssh_port,
                resolve_ssh_ready_wait_timeout(),
                &mut krunkit_child,
                &mut gvproxy_child,
            ) {
                let _ = cleanup_process(&mut krunkit_child);
                let _ = cleanup_process(&mut gvproxy_child);
                state.lifecycle = MachineLifecycle::Failed;
                state.manager = MachineManagerState::Failed;
                state.last_error = Some(error.to_string());
                write_json_file(&paths.state_path, state)?;
                return Err(error);
            }
            state.lifecycle = MachineLifecycle::Running;
            state.manager = MachineManagerState::Ready;
            state.last_error = None;
            write_json_file(&paths.state_path, state)?;
            Ok(())
        }
        Err(error) => {
            let _ = cleanup_process(&mut krunkit_child);
            let _ = cleanup_process(&mut gvproxy_child);
            state.lifecycle = MachineLifecycle::Failed;
            state.manager = MachineManagerState::Failed;
            state.last_error = Some(error.to_string());
            write_json_file(&paths.state_path, state)?;
            Err(error)
        }
    }
}

pub(super) fn stop_machine(
    paths: &MachinePaths,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    if matches!(
        state.lifecycle,
        MachineLifecycle::Stopped | MachineLifecycle::Uninitialized
    ) {
        return Ok(());
    }

    let mut stop_errors = Vec::new();
    if let Err(error) = request_graceful_stop(&paths.krunkit_endpoint_path) {
        stop_errors.push(error.to_string());
    }

    if let Some(pid) = read_pid(&paths.krunkit_pid_path)?
        && let Err(error) = stop_pid(pid, STOP_WAIT_TIMEOUT)
    {
        stop_errors.push(error.to_string());
    }
    if let Some(pid) = read_pid(&paths.gvproxy_pid_path)?
        && let Err(error) = stop_pid(pid, STOP_WAIT_TIMEOUT)
    {
        stop_errors.push(error.to_string());
    }

    cleanup_runtime_artifacts(paths)?;
    state.lifecycle = MachineLifecycle::Stopped;
    state.manager = if state.runtime.is_some() {
        MachineManagerState::HelpersResolved
    } else {
        MachineManagerState::Unconfigured
    };
    state.last_error = if stop_errors.is_empty() {
        None
    } else {
        Some(stop_errors.join("; "))
    };
    write_json_file(&paths.state_path, state)?;
    Ok(())
}

pub(super) fn refresh_machine_state(
    paths: &MachinePaths,
    state: &mut MachineStateRecord,
) -> Result<(), Error> {
    if !matches!(
        state.lifecycle,
        MachineLifecycle::Starting | MachineLifecycle::Running
    ) {
        return Ok(());
    }

    let krunkit_alive = read_pid(&paths.krunkit_pid_path)?
        .map(pid_is_alive)
        .unwrap_or(false);
    let gvproxy_alive = read_pid(&paths.gvproxy_pid_path)?
        .map(pid_is_alive)
        .unwrap_or(false);

    if krunkit_alive && gvproxy_alive {
        if state.lifecycle == MachineLifecycle::Starting && paths.ready_socket_path.exists() {
            state.manager = MachineManagerState::Launching;
        }
        return Ok(());
    }

    state.lifecycle = MachineLifecycle::Failed;
    state.manager = MachineManagerState::Stale;
    state.last_error = Some(format!(
        "machine runtime is stale: krunkit_alive={krunkit_alive} gvproxy_alive={gvproxy_alive}"
    ));
    write_json_file(&paths.state_path, state)
}

pub(super) fn build_ssh_command(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<Command, Error> {
    if state.lifecycle != MachineLifecycle::Running {
        return Err(Error::Conflict(format!(
            "machine '{}' is {} and cannot accept SSH",
            config.name,
            state.lifecycle.as_str()
        )));
    }

    let runtime = state.runtime.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' has no recorded runtime; start it first",
            config.name
        ))
    })?;
    let identity_path = config.guest.ssh_identity_path.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' has no SSH identity configured; re-run `neovex machine init --ssh-identity <path>` or wait for MAC4 guest bootstrap",
            config.name
        ))
    })?;
    if !identity_path.is_file() {
        return Err(Error::InvalidInput(format!(
            "machine '{}' SSH identity does not exist at {}",
            config.name,
            identity_path.display()
        )));
    }

    let mut command = Command::new("ssh");
    append_localhost_ssh_options(
        &mut command,
        identity_path,
        runtime.ssh_port,
        &config.guest.ssh_user,
    );
    Ok(command)
}

fn append_localhost_ssh_options(
    command: &mut Command,
    identity_path: &Path,
    ssh_port: u16,
    ssh_user: &str,
) {
    command
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("IdentitiesOnly=yes")
        .arg("-o")
        .arg("StrictHostKeyChecking=no")
        .arg("-o")
        .arg("UserKnownHostsFile=/dev/null")
        .arg("-o")
        .arg("CheckHostIP=no")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg("-o")
        .arg("SetEnv=LC_ALL=")
        .arg("-i")
        .arg(identity_path)
        .arg("-p")
        .arg(ssh_port.to_string())
        .arg(format!("{ssh_user}@127.0.0.1"));
}

fn bind_ready_listener(path: &Path) -> Result<UnixListener, Error> {
    remove_file_if_exists(path)?;
    let listener = UnixListener::bind(path).map_err(|error| {
        Error::Internal(format!(
            "failed to bind machine ready socket {}: {error}",
            path.display()
        ))
    })?;
    listener.set_nonblocking(true).map_err(|error| {
        Error::Internal(format!(
            "failed to configure machine ready socket {}: {error}",
            path.display()
        ))
    })?;
    Ok(listener)
}

fn serve_ignition_file(
    socket_path: &Path,
    ignition_path: &Path,
) -> Result<thread::JoinHandle<()>, Error> {
    remove_file_if_exists(socket_path)?;
    let bytes = fs::read(ignition_path).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to read ignition file {}: {error}",
            ignition_path.display()
        ))
    })?;
    let listener = UnixListener::bind(socket_path).map_err(|error| {
        Error::Internal(format!(
            "failed to bind ignition socket {}: {error}",
            socket_path.display()
        ))
    })?;
    let response_prefix = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    )
    .into_bytes();
    Ok(thread::spawn(move || {
        while let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(&response_prefix);
            let _ = stream.write_all(&bytes);
            let _ = stream.flush();
        }
    }))
}

fn wait_for_ready(
    listener: &UnixListener,
    timeout: Duration,
    krunkit_child: &mut Child,
    gvproxy_child: &mut Child,
) -> Result<(), Error> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = krunkit_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll krunkit process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "krunkit exited before machine readiness with status {status}"
            )));
        }
        if let Some(status) = gvproxy_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll gvproxy process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "gvproxy exited before machine readiness with status {status}"
            )));
        }

        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut buffer = [0u8; 32];
                let _ = stream.read(&mut buffer);
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => {
                return Err(Error::Internal(format!(
                    "failed while waiting for machine ready signal: {error}"
                )));
            }
        }

        if Instant::now() >= deadline {
            return Err(Error::Internal(format!(
                "machine ready signal did not arrive within {} seconds",
                timeout.as_secs()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn wait_for_ssh_ready(
    config: &MachineConfigRecord,
    ssh_port: u16,
    timeout: Duration,
    krunkit_child: &mut Child,
    gvproxy_child: &mut Child,
) -> Result<(), Error> {
    // Mirror Podman's macOS machine layering: the ready signal alone is not
    // enough to prove host reachability, so only declare the machine started
    // once localhost SSH is actually up too.
    let deadline = Instant::now() + timeout;
    let mut last_probe_error: Option<String>;
    loop {
        if let Some(status) = krunkit_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll krunkit process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "krunkit exited before SSH readiness with status {status}"
            )));
        }
        if let Some(status) = gvproxy_child.try_wait().map_err(|error| {
            Error::Internal(format!("failed to poll gvproxy process state: {error}"))
        })? {
            return Err(Error::Internal(format!(
                "gvproxy exited before SSH readiness with status {status}"
            )));
        }

        if ssh_port_is_listening(ssh_port) {
            if config.guest.ssh_identity_path.is_none() {
                return Ok(());
            }
            match run_silent_ssh_probe(config, ssh_port) {
                Ok(()) => return Ok(()),
                Err(error) => last_probe_error = Some(error.to_string()),
            }
        } else {
            last_probe_error = Some(format!(
                "guest SSH port {ssh_port} is not listening on localhost yet"
            ));
        }

        if Instant::now() >= deadline {
            return Err(Error::Internal(format!(
                "guest SSH readiness did not arrive within {} seconds{}",
                timeout.as_secs(),
                last_probe_error
                    .as_deref()
                    .map(|error| format!(": {error}"))
                    .unwrap_or_default()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn ssh_port_is_listening(ssh_port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{ssh_port}")
            .parse()
            .expect("ssh localhost socket address should parse"),
        Duration::from_millis(100),
    )
    .map(|stream| {
        let _ = stream.shutdown(std::net::Shutdown::Both);
    })
    .is_ok()
}

fn run_silent_ssh_probe(config: &MachineConfigRecord, ssh_port: u16) -> Result<(), Error> {
    let identity_path = config.guest.ssh_identity_path.as_ref().ok_or_else(|| {
        Error::InvalidInput(format!(
            "machine '{}' has no SSH identity configured",
            config.name
        ))
    })?;
    let mut command = Command::new("ssh");
    append_localhost_ssh_options(
        &mut command,
        identity_path,
        ssh_port,
        &config.guest.ssh_user,
    );
    let status = command
        .arg("true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| {
            Error::Internal(format!(
                "failed to run guest SSH readiness probe on localhost:{ssh_port}: {error}"
            ))
        })?;
    if status.success() {
        return Ok(());
    }
    Err(Error::Internal(format!(
        "guest SSH readiness probe failed on localhost:{ssh_port} with status {status}"
    )))
}

fn wait_for_path(path: &Path, timeout: Duration, child: &mut Child) -> Result<(), Error> {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait().map_err(|error| {
            Error::Internal(format!(
                "failed to poll process while waiting for {}: {error}",
                path.display()
            ))
        })? {
            return Err(Error::Internal(format!(
                "process exited before {} appeared with status {status}",
                path.display()
            )));
        }
        if Instant::now() >= deadline {
            return Err(Error::Internal(format!(
                "timed out waiting for {}",
                path.display()
            )));
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn request_graceful_stop(endpoint_path: &Path) -> Result<(), Error> {
    if !endpoint_path.exists() {
        return Ok(());
    }

    let mut stream = UnixStream::connect(endpoint_path).map_err(|error| {
        Error::Internal(format!(
            "failed to connect to krunkit control socket {}: {error}",
            endpoint_path.display()
        ))
    })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| {
            Error::Internal(format!(
                "failed to configure krunkit control socket timeout {}: {error}",
                endpoint_path.display()
            ))
        })?;
    stream
        .write_all(
            b"POST /vm/state HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 17\r\n\r\n{\"state\":\"Stop\"}",
        )
        .map_err(|error| {
            Error::Internal(format!(
                "failed to send krunkit stop request {}: {error}",
                endpoint_path.display()
            ))
        })?;
    let mut response = String::new();
    stream.read_to_string(&mut response).map_err(|error| {
        Error::Internal(format!(
            "failed to read krunkit stop response {}: {error}",
            endpoint_path.display()
        ))
    })?;
    if response.contains("200 OK") {
        return Ok(());
    }
    Err(Error::Internal(format!(
        "krunkit stop request did not return 200 OK: {}",
        response.lines().next().unwrap_or("<empty-response>")
    )))
}

fn stop_pid(pid: i32, timeout: Duration) -> Result<(), Error> {
    if !pid_is_alive(pid) {
        return Ok(());
    }
    send_signal(pid, SIGTERM)?;
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !pid_is_alive(pid) {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }

    send_signal(pid, SIGKILL)?;
    let kill_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < kill_deadline {
        if !pid_is_alive(pid) {
            return Ok(());
        }
        thread::sleep(POLL_INTERVAL);
    }

    Err(Error::Internal(format!(
        "process {pid} did not stop after SIGTERM and SIGKILL"
    )))
}

fn cleanup_process(child: &mut Child) -> Result<(), Error> {
    match child.try_wait() {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            child.kill().map_err(|error| {
                Error::Internal(format!(
                    "failed to terminate child process {}: {error}",
                    child.id()
                ))
            })?;
            child.wait().map(|_| ()).map_err(|error| {
                Error::Internal(format!(
                    "failed to reap child process {}: {error}",
                    child.id()
                ))
            })
        }
        Err(error) => Err(Error::Internal(format!(
            "failed to poll child process {}: {error}",
            child.id()
        ))),
    }
}

fn cleanup_runtime_artifacts(paths: &MachinePaths) -> Result<(), Error> {
    for path in [
        &paths.ready_socket_path,
        &paths.ignition_socket_path,
        &paths.api_socket_path,
        &paths.gvproxy_socket_path,
        &paths.krunkit_endpoint_path,
        &paths.gvproxy_pid_path,
        &paths.krunkit_pid_path,
    ] {
        remove_file_if_exists(path)?;
    }
    for path in [
        &paths.machine_log_path,
        &paths.krunkit_log_path,
        &paths.gvproxy_log_path,
    ] {
        truncate_file(path)?;
    }
    Ok(())
}

fn truncate_file(path: &Path) -> Result<(), Error> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            Error::Internal(format!("failed to create {}: {error}", parent.display()))
        })?;
    }
    fs::write(path, [])
        .map_err(|error| Error::Internal(format!("failed to truncate {}: {error}", path.display())))
}

fn remove_file_if_exists(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Internal(format!(
            "failed to remove {}: {error}",
            path.display()
        ))),
    }
}

fn read_pid(path: &Path) -> Result<Option<i32>, Error> {
    match fs::read_to_string(path) {
        Ok(value) => value.trim().parse::<i32>().map(Some).map_err(|error| {
            Error::Internal(format!(
                "failed to parse pid file {}: {error}",
                path.display()
            ))
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Internal(format!(
            "failed to read pid file {}: {error}",
            path.display()
        ))),
    }
}

fn send_signal(pid: i32, signal: i32) -> Result<(), Error> {
    let rc = unsafe { kill(pid, signal) };
    if rc == 0 || !pid_is_alive(pid) {
        return Ok(());
    }
    Err(Error::Internal(format!(
        "failed to send signal {signal} to process {pid}: {}",
        io::Error::last_os_error()
    )))
}

fn pid_is_alive(pid: i32) -> bool {
    let rc = unsafe { kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn resolve_helper_binary(
    env_name: &str,
    command_name: &str,
    fallbacks: &[PathBuf],
) -> Result<PathBuf, Error> {
    if let Some(path) = std::env::var_os(env_name) {
        return resolve_existing_file(PathBuf::from(path), env_name);
    }
    if let Some(path) = find_in_path(command_name) {
        return Ok(path);
    }
    for fallback in fallbacks {
        if fallback.is_file() {
            return Ok(fallback.clone());
        }
    }
    Err(Error::InvalidInput(format!(
        "required helper '{command_name}' was not found; set {env_name} or install it on PATH"
    )))
}

fn resolve_existing_file(path: PathBuf, env_name: &str) -> Result<PathBuf, Error> {
    if path.is_file() {
        return Ok(path);
    }
    Err(Error::InvalidInput(format!(
        "{env_name} points to {}, but that file does not exist",
        path.display()
    )))
}

fn find_in_path(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|entry| entry.join(binary))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
fn helper_env_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
pub(crate) struct MachineHelperEnvGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    previous_krunkit: Option<std::ffi::OsString>,
    previous_gvproxy: Option<std::ffi::OsString>,
}

#[cfg(test)]
impl MachineHelperEnvGuard {
    pub(crate) fn install_stub_binaries(dir: &Path) -> Self {
        let krunkit_path = dir.join("krunkit");
        let gvproxy_path = dir.join("gvproxy");
        write_helper_stub(&krunkit_path, "krunkit");
        write_helper_stub(&gvproxy_path, "gvproxy");
        Self::set_paths(&krunkit_path, &gvproxy_path)
    }

    pub(crate) fn set_paths(krunkit_path: &Path, gvproxy_path: &Path) -> Self {
        let lock = helper_env_lock()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_krunkit = std::env::var_os(KRUNKIT_ENV);
        let previous_gvproxy = std::env::var_os(GVPROXY_ENV);
        unsafe {
            std::env::set_var(KRUNKIT_ENV, krunkit_path);
            std::env::set_var(GVPROXY_ENV, gvproxy_path);
        }
        Self {
            _lock: lock,
            previous_krunkit,
            previous_gvproxy,
        }
    }
}

#[cfg(test)]
impl Drop for MachineHelperEnvGuard {
    fn drop(&mut self) {
        match &self.previous_krunkit {
            Some(path) => unsafe { std::env::set_var(KRUNKIT_ENV, path) },
            None => unsafe { std::env::remove_var(KRUNKIT_ENV) },
        }
        match &self.previous_gvproxy {
            Some(path) => unsafe { std::env::set_var(GVPROXY_ENV, path) },
            None => unsafe { std::env::remove_var(GVPROXY_ENV) },
        }
    }
}

#[cfg(test)]
fn write_helper_stub(path: &Path, helper_name: &str) {
    fs::write(path, "#!/bin/sh\n").unwrap_or_else(|error| {
        panic!("{helper_name} stub should write: {error}");
    });
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap_or_else(|error| {
            panic!("{helper_name} stub should be executable: {error}");
        });
    }
}

fn resolve_bootable_image_path(
    paths: &MachinePaths,
    image_source: &MachineImageSource,
) -> Result<PathBuf, Error> {
    match image_source {
        MachineImageSource::LocalDisk { path } => {
            if !path.is_file() {
                return Err(Error::InvalidInput(format!(
                    "machine guest image {} does not exist",
                    path.display()
                )));
            }
            Ok(path.clone())
        }
        MachineImageSource::OciReference { reference } => {
            if paths.materialized_image_path.is_file() {
                return Ok(paths.materialized_image_path.clone());
            }
            materialize_oci_image(paths, reference)
        }
        MachineImageSource::HttpUrl { url } => {
            if paths.materialized_image_path.is_file() {
                return Ok(paths.materialized_image_path.clone());
            }
            materialize_http_image(paths, url)
        }
    }
}

fn materialize_http_image(paths: &MachinePaths, url: &str) -> Result<PathBuf, Error> {
    fs::create_dir_all(&paths.image_cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine image cache directory {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;

    let download = NamedTempFile::new_in(&paths.image_cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to allocate temporary download file under {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;
    let client = Client::builder()
        .timeout(HTTP_IMAGE_TIMEOUT)
        .build()
        .map_err(|error| Error::Internal(format!("failed to build HTTP client: {error}")))?;
    let mut response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to download machine guest image from {url}: {error}"
            ))
        })?;

    let mut writer = download.reopen().map_err(|error| {
        Error::Internal(format!(
            "failed to reopen temporary download file under {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;
    io::copy(&mut response, &mut writer).map_err(|error| {
        Error::Internal(format!(
            "failed to write downloaded machine image from {url} into {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;
    writer.flush().map_err(|error| {
        Error::Internal(format!(
            "failed to flush downloaded machine image for {url}: {error}"
        ))
    })?;
    drop(writer);

    let temp_output = NamedTempFile::new_in(&paths.image_cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to allocate temporary materialization file under {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;

    if url.ends_with(".gz") {
        let input = download.reopen().map_err(|error| {
            Error::Internal(format!(
                "failed to reopen temporary download file for gzip decode: {error}"
            ))
        })?;
        let mut decoder = GzDecoder::new(BufReader::new(input));
        let mut output = temp_output.reopen().map_err(|error| {
            Error::Internal(format!(
                "failed to reopen temporary materialization file for gzip decode: {error}"
            ))
        })?;
        io::copy(&mut decoder, &mut output).map_err(|error| {
            Error::Internal(format!(
                "failed to decompress gzip machine image from {url}: {error}"
            ))
        })?;
        output.flush().map_err(|error| {
            Error::Internal(format!(
                "failed to flush decompressed machine image for {url}: {error}"
            ))
        })?;
    } else {
        fs::copy(download.path(), temp_output.path()).map_err(|error| {
            Error::Internal(format!(
                "failed to stage downloaded machine image from {url}: {error}"
            ))
        })?;
    }

    temp_output
        .persist(&paths.materialized_image_path)
        .map_err(|error| {
            Error::Internal(format!(
                "failed to persist machine image from {url} into {}: {}",
                paths.materialized_image_path.display(),
                error.error
            ))
        })?;

    Ok(paths.materialized_image_path.clone())
}

fn materialize_oci_image(paths: &MachinePaths, reference: &str) -> Result<PathBuf, Error> {
    fs::create_dir_all(&paths.image_cache_dir).map_err(|error| {
        Error::Internal(format!(
            "failed to create machine image cache directory {}: {error}",
            paths.image_cache_dir.display()
        ))
    })?;

    let cache_dir = paths.image_cache_dir.clone();
    let reference = reference.to_owned();
    let source_label = format!("published OCI artifact '{reference}'");
    let reference_for_pull = reference.clone();
    let cached_blob_path = run_async_in_thread(move || async move {
        pull_oci_artifact_to_cache(cache_dir, reference_for_pull).await
    })?;

    materialize_cached_disk(
        &cached_blob_path,
        &paths.materialized_image_path,
        &source_label,
    )?;
    Ok(paths.materialized_image_path.clone())
}

async fn pull_oci_artifact_to_cache(
    image_cache_dir: PathBuf,
    reference: String,
) -> Result<PathBuf, Error> {
    let stripped_reference = strip_docker_reference_prefix(&reference);
    let registry_reference = Reference::try_from(stripped_reference.as_str()).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to parse machine guest OCI reference '{reference}': {error}"
        ))
    })?;
    let client = build_oci_client(&stripped_reference)?;
    let auth = RegistryAuth::Anonymous;
    let accepted_media_types = vec![
        OCI_IMAGE_INDEX_MEDIA_TYPE,
        IMAGE_MANIFEST_LIST_MEDIA_TYPE,
        OCI_IMAGE_MEDIA_TYPE,
        IMAGE_MANIFEST_MEDIA_TYPE,
    ];
    let (top_manifest_bytes, _) = client
        .pull_manifest_raw(&registry_reference, &auth, &accepted_media_types)
        .await
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to resolve machine guest OCI reference '{reference}': {error}"
            ))
        })?;

    let selected_artifact =
        select_oci_artifact_layer(&reference, &top_manifest_bytes, &client, &auth).await?;
    let cache_path = image_cache_dir.join(cached_oci_blob_file_name(&selected_artifact.layer));
    if cache_path.is_file() {
        return Ok(cache_path);
    }

    let download_path = image_cache_dir.join(format!(
        "{}.download",
        digest_hex(&selected_artifact.layer.digest)?
    ));
    if download_path.exists() {
        fs::remove_file(&download_path).map_err(|error| {
            Error::Internal(format!(
                "failed to remove stale machine image download {}: {error}",
                download_path.display()
            ))
        })?;
    }

    let mut output = tokio::fs::File::create(&download_path)
        .await
        .map_err(|error| {
            Error::Internal(format!(
                "failed to create temporary machine image download {}: {error}",
                download_path.display()
            ))
        })?;
    let layer = to_oci_descriptor(&selected_artifact.layer);
    client
        .pull_blob(&selected_artifact.child_reference, &layer, &mut output)
        .await
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to download machine guest OCI artifact '{}': {error}",
                reference
            ))
        })?;
    output.flush().await.map_err(|error| {
        Error::Internal(format!(
            "failed to flush downloaded machine guest OCI artifact '{}': {error}",
            reference
        ))
    })?;
    output.shutdown().await.map_err(|error| {
        Error::Internal(format!(
            "failed to close downloaded machine guest OCI artifact '{}': {error}",
            reference
        ))
    })?;
    drop(output);

    verify_downloaded_oci_blob(&download_path, &selected_artifact.layer)?;
    log_machine_artifact_metadata(&reference, &selected_artifact.metadata);
    check_build_attestation(
        &reference,
        &selected_artifact.layer.digest,
        selected_artifact.metadata.attestation_repository.as_deref(),
    );
    fs::rename(&download_path, &cache_path).map_err(|error| {
        Error::Internal(format!(
            "failed to persist machine guest OCI artifact cache {}: {error}",
            cache_path.display()
        ))
    })?;

    Ok(cache_path)
}

async fn select_oci_artifact_layer(
    reference: &str,
    top_manifest_bytes: &[u8],
    client: &OciClient,
    auth: &RegistryAuth,
) -> Result<SelectedMachineArtifact, Error> {
    if let Ok(index) = serde_json::from_slice::<RegistryImageIndex>(top_manifest_bytes) {
        let manifest_descriptor =
            select_oci_manifest_descriptor(reference, &index.manifests)?.clone();
        let child_reference = build_digest_reference(reference, &manifest_descriptor.digest)?;
        let (child_manifest_bytes, _) = client
            .pull_manifest_raw(
                &child_reference,
                auth,
                &[OCI_IMAGE_MEDIA_TYPE, IMAGE_MANIFEST_MEDIA_TYPE],
            )
            .await
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "failed to pull machine guest OCI child manifest '{}': {error}",
                    manifest_descriptor.digest
                ))
            })?;
        let child_manifest = serde_json::from_slice::<RegistryImageManifest>(&child_manifest_bytes)
            .map_err(|error| {
                Error::Internal(format!(
                    "failed to parse machine guest OCI child manifest '{}': {error}",
                    manifest_descriptor.digest
                ))
            })?;
        let layer = select_machine_layer(reference, &child_manifest.layers)?;
        return Ok(SelectedMachineArtifact {
            child_reference,
            layer: layer.clone(),
            metadata: machine_artifact_metadata_from_annotations(
                Some(&manifest_descriptor.annotations),
                Some(&child_manifest.annotations),
            ),
        });
    }

    let image_manifest = serde_json::from_slice::<RegistryImageManifest>(top_manifest_bytes)
        .map_err(|error| {
            Error::Internal(format!(
                "failed to parse machine guest OCI manifest '{}': {error}",
                reference
            ))
        })?;
    let layer = select_machine_layer(reference, &image_manifest.layers)?;
    let registry_reference = Reference::try_from(strip_docker_reference_prefix(reference).as_str())
        .map_err(|error| {
            Error::InvalidInput(format!(
                "failed to parse machine guest OCI reference '{reference}': {error}"
            ))
        })?;
    Ok(SelectedMachineArtifact {
        child_reference: registry_reference,
        layer: layer.clone(),
        metadata: machine_artifact_metadata_from_annotations(
            Some(&image_manifest.annotations),
            None,
        ),
    })
}

fn build_oci_client(reference: &str) -> Result<OciClient, Error> {
    let mut config = OciClientConfig::default();
    if is_loopback_registry(reference) {
        config.protocol = ClientProtocol::Http;
    }
    OciClient::try_from(config).map_err(|error| {
        Error::Internal(format!(
            "failed to initialize OCI client for machine image '{reference}': {error}"
        ))
    })
}

fn is_loopback_registry(reference: &str) -> bool {
    let stripped_reference = strip_docker_reference_prefix(reference);
    let host = stripped_reference.split('/').next().unwrap_or_default();
    host.starts_with("localhost") || host.starts_with("127.0.0.1") || host.starts_with("[::1]")
}

fn strip_docker_reference_prefix(reference: &str) -> String {
    reference
        .strip_prefix("docker://")
        .unwrap_or(reference)
        .to_owned()
}

fn select_oci_manifest_descriptor<'a>(
    reference: &str,
    manifests: &'a [RegistryManifestDescriptor],
) -> Result<&'a RegistryManifestDescriptor, Error> {
    manifests
        .iter()
        .find(|descriptor| {
            let Some(platform) = descriptor.platform.as_ref() else {
                return false;
            };
            platform.os == OCI_MACHINE_OS
                && current_machine_oci_architectures()
                    .iter()
                    .any(|arch| platform.architecture == *arch)
                && descriptor
                    .annotations
                    .get("disktype")
                    .map(|value| value == OCI_MACHINE_DISK_TYPE)
                    .unwrap_or(false)
        })
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "machine guest OCI reference '{}' does not contain a linux/{:?} '{}' disk artifact",
                reference,
                current_machine_oci_architectures(),
                OCI_MACHINE_DISK_TYPE
            ))
        })
}

fn select_machine_layer<'a>(
    reference: &str,
    layers: &'a [RegistryLayerDescriptor],
) -> Result<&'a RegistryLayerDescriptor, Error> {
    match layers {
        [layer] => Ok(layer),
        [] => Err(Error::InvalidInput(format!(
            "machine guest OCI reference '{}' has no disk layers",
            reference
        ))),
        _ => Err(Error::InvalidInput(format!(
            "machine guest OCI reference '{}' has {} disk layers; expected exactly 1",
            reference,
            layers.len()
        ))),
    }
}

fn current_machine_oci_architectures() -> &'static [&'static str] {
    #[cfg(target_arch = "aarch64")]
    {
        &["aarch64", "arm64"]
    }
    #[cfg(target_arch = "x86_64")]
    {
        &["x86_64", "amd64"]
    }
}

fn build_digest_reference(reference: &str, digest: &str) -> Result<Reference, Error> {
    let reference = strip_docker_reference_prefix(reference);
    let repository = reference
        .split_once('@')
        .map(|(value, _)| value.to_owned())
        .unwrap_or_else(|| {
            let last_slash = reference.rfind('/');
            let last_colon = reference.rfind(':');
            match (last_slash, last_colon) {
                (_, None) => reference.clone(),
                (Some(slash), Some(colon)) if colon > slash => reference[..colon].to_owned(),
                (None, Some(colon)) if !reference[..colon].contains('/') => reference.clone(),
                _ => reference.clone(),
            }
        });
    Reference::try_from(format!("{repository}@{digest}")).map_err(|error| {
        Error::InvalidInput(format!(
            "failed to build machine guest OCI digest reference '{repository}@{digest}': {error}"
        ))
    })
}

fn cached_oci_blob_file_name(layer: &RegistryLayerDescriptor) -> String {
    let digest = digest_hex(&layer.digest).unwrap_or_else(|_| "machine-image".to_owned());
    let suffix = layer
        .annotations
        .get(OCI_ANNOTATION_TITLE)
        .and_then(|title| oci_artifact_suffix(title))
        .unwrap_or(".blob");
    format!("{digest}{suffix}")
}

fn oci_artifact_suffix(title: &str) -> Option<&str> {
    [
        ".raw.zst",
        ".raw.gz",
        ".raw",
        ".qcow2.xz",
        ".qcow2.gz",
        ".qcow2",
    ]
    .into_iter()
    .find(|suffix| title.ends_with(suffix))
}

fn verify_downloaded_oci_blob(path: &Path, layer: &RegistryLayerDescriptor) -> Result<(), Error> {
    let metadata = fs::metadata(path).map_err(|error| {
        Error::Internal(format!(
            "failed to stat downloaded machine guest OCI artifact {}: {error}",
            path.display()
        ))
    })?;
    if metadata.len() != layer.size as u64 {
        return Err(Error::InvalidInput(format!(
            "downloaded machine guest OCI artifact {} has size {}, expected {}",
            path.display(),
            metadata.len(),
            layer.size
        )));
    }
    let digest = compute_sha256(path)?;
    let expected = digest_hex(&layer.digest)?;
    if digest != expected {
        return Err(Error::InvalidInput(format!(
            "downloaded machine guest OCI artifact {} has sha256 {}, expected {}",
            path.display(),
            digest,
            expected
        )));
    }
    Ok(())
}

fn machine_artifact_metadata_from_annotations(
    primary: Option<&BTreeMap<String, String>>,
    fallback: Option<&BTreeMap<String, String>>,
) -> MachineArtifactMetadata {
    MachineArtifactMetadata {
        attestation_repository: annotation_value(
            primary,
            fallback,
            OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY,
        ),
        source_repository_url: annotation_value(primary, fallback, OCI_ANNOTATION_SOURCE),
        neovex_version: annotation_value(primary, fallback, OCI_ANNOTATION_MACHINE_NEOVEX_VERSION),
    }
}

fn annotation_value(
    primary: Option<&BTreeMap<String, String>>,
    fallback: Option<&BTreeMap<String, String>>,
    key: &str,
) -> Option<String> {
    primary
        .and_then(|annotations| annotations.get(key))
        .or_else(|| fallback.and_then(|annotations| annotations.get(key)))
        .filter(|value| !value.is_empty())
        .cloned()
}

fn log_machine_artifact_metadata(reference: &str, metadata: &MachineArtifactMetadata) {
    if let Some(neovex_version) = metadata.neovex_version.as_deref() {
        eprintln!("info: machine image '{reference}' embeds neovex {neovex_version}");
    }
    if let Some(source_repository_url) = metadata.source_repository_url.as_deref() {
        eprintln!("info: machine image '{reference}' source={source_repository_url}");
    }
}

/// The neovex source repo, used as a fallback for legacy machine images that
/// were published before OCI metadata recorded the attestation repository.
const NEOVEX_SOURCE_REPO: &str = "agentstation/neovex";

/// Query the GitHub Attestations API for a signed build provenance attestation
/// matching the downloaded artifact digest. Prefer the explicit attestation
/// repository published in the OCI metadata; fall back to the historical
/// dual-repo lookup only for older machine images. Advisory only — logs the
/// result but does not block the download.
fn check_build_attestation(
    reference: &str,
    subject_digest: &str,
    explicit_repository: Option<&str>,
) {
    let stripped = strip_docker_reference_prefix(reference);
    let Some(image_repo) = extract_ghcr_repo_path(&stripped) else {
        return;
    };

    let repos_to_check = attestation_repositories_for_reference(
        &image_repo,
        explicit_repository.filter(|repo| !repo.is_empty()),
    );

    let client = match Client::builder().timeout(Duration::from_secs(10)).build() {
        Ok(client) => client,
        Err(error) => {
            eprintln!("warning: attestation lookup failed: {error}");
            return;
        }
    };

    for repo in &repos_to_check {
        match query_attestations(&client, repo, subject_digest) {
            Ok(count) if count > 0 => {
                eprintln!(
                    "verified: {count} build attestation(s) found for {subject_digest} in {repo}"
                );
                return;
            }
            Ok(_) => {}
            Err(msg) => {
                eprintln!("warning: attestation lookup for {repo}: {msg}");
            }
        }
    }

    eprintln!("warning: no build attestations found for {subject_digest}");
}

/// Query the GitHub Attestations API for a specific repo and digest.
/// Returns the number of attestations found, or an error message.
fn query_attestations(client: &Client, repo: &str, subject_digest: &str) -> Result<usize, String> {
    let url = format!("https://api.github.com/repos/{repo}/attestations/{subject_digest}");

    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .header("User-Agent", "neovex-machine-manager")
        .send()
        .map_err(|e| format!("{e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let body: serde_json::Value = response.json().map_err(|e| format!("{e}"))?;

    Ok(body
        .get("attestations")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0))
}

fn attestation_repositories_for_reference(
    image_repo: &str,
    explicit_repository: Option<&str>,
) -> Vec<String> {
    if let Some(explicit_repository) = explicit_repository {
        return vec![explicit_repository.to_owned()];
    }

    if image_repo == NEOVEX_SOURCE_REPO {
        vec![image_repo.to_owned()]
    } else {
        vec![image_repo.to_owned(), NEOVEX_SOURCE_REPO.to_owned()]
    }
}

/// Extract the GitHub repository path (owner/repo) from a ghcr.io image
/// reference. Returns None for non-GHCR references.
fn extract_ghcr_repo_path(reference: &str) -> Option<String> {
    let without_host = reference.strip_prefix("ghcr.io/")?;
    let without_tag = without_host
        .split_once('@')
        .map(|(r, _)| r)
        .unwrap_or(without_host);
    let without_tag = without_tag
        .split_once(':')
        .map(|(r, _)| r)
        .unwrap_or(without_tag);
    let parts: Vec<&str> = without_tag.splitn(3, '/').collect();
    if parts.len() >= 2 {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

fn compute_sha256(path: &Path) -> Result<String, Error> {
    let mut reader = BufReader::new(fs::File::open(path).map_err(|error| {
        Error::Internal(format!(
            "failed to open {} for sha256 verification: {error}",
            path.display()
        ))
    })?);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer).map_err(|error| {
            Error::Internal(format!(
                "failed to read {} for sha256 verification: {error}",
                path.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn digest_hex(digest: &str) -> Result<String, Error> {
    let (algorithm, hex) = digest.split_once(':').ok_or_else(|| {
        Error::InvalidInput(format!(
            "invalid OCI digest '{}': missing algorithm prefix",
            digest
        ))
    })?;
    if algorithm != "sha256" {
        return Err(Error::InvalidInput(format!(
            "unsupported OCI digest algorithm '{}'; expected sha256",
            algorithm
        )));
    }
    Ok(hex.to_owned())
}

fn materialize_cached_disk(
    source_path: &Path,
    output_path: &Path,
    source_label: &str,
) -> Result<(), Error> {
    let temp_output = NamedTempFile::new_in(output_path.parent().ok_or_else(|| {
        Error::Internal(format!("{} has no parent directory", output_path.display()))
    })?)
    .map_err(|error| {
        Error::Internal(format!(
            "failed to allocate temporary materialization file for {}: {error}",
            source_label
        ))
    })?;

    let compression = detect_disk_compression(source_path)?;
    match compression {
        DiskCompression::None => {
            fs::copy(source_path, temp_output.path()).map_err(|error| {
                Error::Internal(format!(
                    "failed to stage {} into {}: {error}",
                    source_label,
                    temp_output.path().display()
                ))
            })?;
        }
        DiskCompression::Gzip => {
            let input = fs::File::open(source_path).map_err(|error| {
                Error::Internal(format!(
                    "failed to open {} for gzip decode: {error}",
                    source_path.display()
                ))
            })?;
            let mut decoder = GzDecoder::new(BufReader::new(input));
            let mut output = temp_output.reopen().map_err(|error| {
                Error::Internal(format!(
                    "failed to reopen {} for gzip decode: {error}",
                    temp_output.path().display()
                ))
            })?;
            io::copy(&mut decoder, &mut output).map_err(|error| {
                Error::Internal(format!(
                    "failed to decompress gzip {}: {error}",
                    source_label
                ))
            })?;
            output.flush().map_err(|error| {
                Error::Internal(format!(
                    "failed to flush decompressed {}: {error}",
                    source_label
                ))
            })?;
        }
        DiskCompression::Zstd => {
            let input = fs::File::open(source_path).map_err(|error| {
                Error::Internal(format!(
                    "failed to open {} for zstd decode: {error}",
                    source_path.display()
                ))
            })?;
            let mut output = temp_output.reopen().map_err(|error| {
                Error::Internal(format!(
                    "failed to reopen {} for zstd decode: {error}",
                    temp_output.path().display()
                ))
            })?;
            zstd::stream::copy_decode(BufReader::new(input), &mut output).map_err(|error| {
                Error::Internal(format!(
                    "failed to decompress zstd {}: {error}",
                    source_label
                ))
            })?;
            output.flush().map_err(|error| {
                Error::Internal(format!(
                    "failed to flush decompressed {}: {error}",
                    source_label
                ))
            })?;
        }
    }

    temp_output.persist(output_path).map_err(|error| {
        Error::Internal(format!(
            "failed to persist materialized machine image {}: {}",
            output_path.display(),
            error.error
        ))
    })?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiskCompression {
    None,
    Gzip,
    Zstd,
}

fn detect_disk_compression(path: &Path) -> Result<DiskCompression, Error> {
    let mut file = fs::File::open(path).map_err(|error| {
        Error::Internal(format!(
            "failed to open machine image {} for compression detection: {error}",
            path.display()
        ))
    })?;
    let mut header = [0_u8; 4];
    let read = file.read(&mut header).map_err(|error| {
        Error::Internal(format!(
            "failed to read machine image {} for compression detection: {error}",
            path.display()
        ))
    })?;
    if read >= 2 && header[..2] == [0x1f, 0x8b] {
        return Ok(DiskCompression::Gzip);
    }
    if read >= 4 && header == [0x28, 0xb5, 0x2f, 0xfd] {
        return Ok(DiskCompression::Zstd);
    }
    Ok(DiskCompression::None)
}

fn to_oci_descriptor(layer: &RegistryLayerDescriptor) -> OciDescriptor {
    OciDescriptor {
        digest: layer.digest.clone(),
        media_type: layer.media_type.clone(),
        size: layer.size,
        annotations: if layer.annotations.is_empty() {
            None
        } else {
            Some(layer.annotations.clone())
        },
        ..Default::default()
    }
}

fn run_async_in_thread<F, Fut, T>(build: F) -> Result<T, Error>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, Error>> + Send + 'static,
    T: Send + 'static,
{
    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| {
                Error::Internal(format!("failed to build machine async runtime: {error}"))
            })?
            .block_on(build())
    })
    .join()
    .map_err(|_| Error::Internal("machine async worker panicked".to_owned()))?
}

fn allocate_local_port() -> io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.local_addr().map(|addr| addr.port())
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

fn resolve_ready_wait_timeout() -> Duration {
    let seconds =
        env_parse_u64(READY_WAIT_TIMEOUT_ENV).unwrap_or(DEFAULT_READY_WAIT_TIMEOUT.as_secs());
    Duration::from_secs(seconds.max(1))
}

fn resolve_ssh_ready_wait_timeout() -> Duration {
    let seconds = env_parse_u64(SSH_READY_WAIT_TIMEOUT_ENV)
        .unwrap_or(DEFAULT_SSH_READY_WAIT_TIMEOUT.as_secs());
    Duration::from_secs(seconds.max(1))
}

fn env_parse_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}

pub(super) fn mount_tag(target: &Path) -> String {
    let digest = Sha256::digest(target.as_os_str().as_encoded_bytes());
    format!("{digest:x}")[..36].to_owned()
}

#[cfg(test)]
mod tests {
    use std::io::{Read as _, Write as _};
    use std::net::TcpListener;
    use std::thread;

    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tempfile::TempDir;

    use super::*;
    use crate::machine::{
        MachineGuestConfig, MachineImageSource, MachineProvider, MachineResources,
        MachineRootLayout,
    };

    fn sample_config(image: &Path) -> MachineConfigRecord {
        MachineConfigRecord {
            name: "default".to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::LocalDisk {
                    path: image.to_path_buf(),
                },
                ssh_user: "core".to_owned(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: 2,
                memory_mib: 2048,
                disk_gib: 20,
            },
            volumes: vec![MachineVolume {
                source: PathBuf::from("/Users"),
                target: PathBuf::from("/Users"),
            }],
            roots: MachineRootLayout::new(
                PathBuf::from("/tmp/config-root"),
                PathBuf::from("/tmp/state-root"),
                PathBuf::from("/tmp/runtime-root"),
            ),
        }
    }

    #[test]
    fn launch_plan_requires_bootable_local_disk_image() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let config = sample_config(&image_path);
        let paths = config.roots.paths("default");
        let state = MachineStateRecord::initialized();
        let plan =
            MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");

        assert!(
            plan.krunkit_command
                .args
                .iter()
                .any(|arg| arg.contains("virtio-blk,path="))
        );
        assert!(
            plan.krunkit_command
                .args
                .iter()
                .any(|arg| arg.contains("virtio-net,type=unixgram"))
        );
        assert!(
            plan.krunkit_command
                .args
                .iter()
                .any(|arg| arg.contains("virtio-vsock,port=1025"))
        );
        assert!(
            plan.krunkit_command
                .args
                .iter()
                .any(|arg| arg.contains("virtio-vsock,port=1024"))
        );
        assert!(
            !plan
                .gvproxy_command
                .args
                .iter()
                .any(|arg| arg == "-forward-sock")
        );
        assert_eq!(
            plan.ignition_file_path,
            Some(paths.generated_ignition_path.clone())
        );
    }

    #[test]
    fn launch_plan_adds_gvproxy_machine_api_forwarding_when_ssh_identity_exists() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let image_path = temp_dir.path().join("disk.raw");
        let ssh_identity_path = temp_dir.path().join("machine-key");
        let ssh_public_key_path = temp_dir.path().join("machine-key.pub");
        fs::write(&image_path, []).expect("image should write");
        fs::write(&ssh_identity_path, "fake key").expect("identity should write");
        fs::write(
            &ssh_public_key_path,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey jack@example",
        )
        .expect("public key should write");

        let mut config = sample_config(&image_path);
        config.guest.ssh_identity_path = Some(ssh_identity_path.clone());

        let paths = config.roots.paths("default");
        let state = MachineStateRecord::initialized();
        let plan =
            MachineLaunchPlan::build(&paths, &config, &state).expect("launch plan should build");

        assert!(plan.gvproxy_command.args.windows(2).any(|pair| {
            pair[0] == "-forward-sock" && pair[1] == paths.api_socket_path.display().to_string()
        }));
        assert!(
            plan.gvproxy_command
                .args
                .windows(2)
                .any(|pair| { pair[0] == "-forward-dest" && pair[1] == GUEST_NEOVEX_SOCKET })
        );
        assert!(
            plan.gvproxy_command
                .args
                .windows(2)
                .any(|pair| { pair[0] == "-forward-user" && pair[1] == MACHINE_API_FORWARD_USER })
        );
        assert!(plan.gvproxy_command.args.windows(2).any(|pair| {
            pair[0] == "-forward-identity" && pair[1] == ssh_identity_path.display().to_string()
        }));
    }

    #[test]
    fn registry_image_reference_materializes_raw_disk_from_oci_registry() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("default");
        let raw_payload = b"raw-disk-oci-bytes".to_vec();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&raw_payload)
            .expect("gzip payload should write");
        let gzip_payload = encoder.finish().expect("gzip payload should finish");
        let reference = serve_fake_oci_registry(gzip_payload);

        let materialized =
            resolve_bootable_image_path(&paths, &MachineImageSource::OciReference { reference })
                .expect("registry image should materialize");

        assert_eq!(materialized, paths.materialized_image_path);
        assert_eq!(
            fs::read(&paths.materialized_image_path).expect("materialized image should read"),
            raw_payload
        );
    }

    #[test]
    fn registry_image_reference_reuses_materialized_disk_when_present() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("default");
        fs::create_dir_all(&paths.image_cache_dir).expect("image cache dir should exist");
        fs::write(&paths.materialized_image_path, []).expect("materialized image should write");

        let config = MachineConfigRecord {
            name: "default".to_owned(),
            provider: MachineProvider::Krunkit,
            guest: MachineGuestConfig {
                image_source: MachineImageSource::OciReference {
                    reference: "docker://ghcr.io/agentstation/neovex-machine-os:v0.1.0".to_owned(),
                },
                ssh_user: "core".to_owned(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: MachineResources {
                cpus: 2,
                memory_mib: 2048,
                disk_gib: 20,
            },
            volumes: Vec::new(),
            roots: layout.clone(),
        };

        let plan = MachineLaunchPlan::build(&paths, &config, &MachineStateRecord::initialized())
            .expect("materialized disk should satisfy launch plan");

        assert_eq!(plan.runtime.image_path, paths.materialized_image_path);
        assert!(
            plan.krunkit_command
                .args
                .iter()
                .any(|arg| arg.contains(&format!(
                    "virtio-blk,path={}",
                    paths.materialized_image_path.display()
                )))
        );
    }

    #[test]
    fn http_image_source_materializes_raw_disk_into_reserved_path() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("default");
        let payload = b"raw-disk-bytes".to_vec();
        let url = serve_single_http_response(payload.clone(), None);

        let materialized =
            resolve_bootable_image_path(&paths, &MachineImageSource::HttpUrl { url: url.clone() })
                .expect("http source should materialize");

        assert_eq!(materialized, paths.materialized_image_path);
        assert_eq!(
            fs::read(&paths.materialized_image_path).expect("materialized image should read"),
            payload
        );
    }

    #[test]
    fn cached_zstd_machine_image_materializes_into_reserved_path() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let source_path = temp_dir.path().join("disk.raw.zst");
        let output_path = temp_dir.path().join("disk.raw");
        let payload = b"raw-disk-zstd-bytes".to_vec();
        let compressed = zstd::stream::encode_all(std::io::Cursor::new(&payload), 1)
            .expect("zstd payload should encode");
        fs::write(&source_path, compressed).expect("compressed source should write");

        materialize_cached_disk(&source_path, &output_path, "test zstd image")
            .expect("zstd image should materialize");

        assert_eq!(
            fs::read(&output_path).expect("materialized image should read"),
            payload
        );
    }

    #[test]
    fn http_gzip_image_source_materializes_decompressed_disk_into_reserved_path() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("default");
        let payload = b"raw-disk-gzip-bytes".to_vec();
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&payload)
            .expect("gzip payload should write");
        let gzip_payload = encoder.finish().expect("gzip payload should finish");
        let url = serve_single_http_response(gzip_payload, Some("/disk.raw.gz"));

        let materialized =
            resolve_bootable_image_path(&paths, &MachineImageSource::HttpUrl { url: url.clone() })
                .expect("gzip http source should materialize");

        assert_eq!(materialized, paths.materialized_image_path);
        assert_eq!(
            fs::read(&paths.materialized_image_path).expect("materialized image should read"),
            payload
        );
    }

    #[test]
    fn helper_resolution_honors_environment_overrides() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let krunkit_path = temp_dir.path().join("krunkit");
        let gvproxy_path = temp_dir.path().join("gvproxy");
        let _guard = MachineHelperEnvGuard::install_stub_binaries(temp_dir.path());
        let resolved =
            MachineHelperBinaryPaths::resolve().expect("helper binaries should resolve via env");

        assert_eq!(resolved.krunkit, krunkit_path);
        assert_eq!(resolved.gvproxy, gvproxy_path);
    }

    #[cfg(unix)]
    #[test]
    fn machine_command_spawn_detaches_helpers_into_new_session() {
        let command = MachineCommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), "sleep 30".to_owned()],
        };
        let mut child = command.spawn().expect("helper process should spawn");
        let child_pid = child.id() as i32;
        let parent_sid = unsafe { libc::getsid(0) };
        let child_sid = unsafe { libc::getsid(child_pid) };

        assert!(parent_sid > 0, "parent sid should resolve");
        assert_eq!(child_sid, child_pid, "child should lead its own session");
        assert_ne!(
            child_sid, parent_sid,
            "child session should differ from parent"
        );

        cleanup_process(&mut child).expect("child should clean up");
    }

    #[test]
    fn ssh_port_is_listening_detects_local_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener address should resolve")
            .port();

        assert!(ssh_port_is_listening(port));
    }

    #[test]
    fn wait_for_ssh_ready_accepts_listening_port_without_identity_probe() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let port = listener
            .local_addr()
            .expect("listener address should resolve")
            .port();
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let config = sample_config(&image_path);
        let mut gvproxy_child = MachineCommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), "sleep 30".to_owned()],
        }
        .spawn()
        .expect("gvproxy probe child should spawn");
        let mut krunkit_child = MachineCommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), "sleep 30".to_owned()],
        }
        .spawn()
        .expect("krunkit probe child should spawn");

        let result = wait_for_ssh_ready(
            &config,
            port,
            Duration::from_secs(1),
            &mut krunkit_child,
            &mut gvproxy_child,
        );

        cleanup_process(&mut krunkit_child).expect("krunkit probe child should clean up");
        cleanup_process(&mut gvproxy_child).expect("gvproxy probe child should clean up");
        drop(listener);

        assert!(result.is_ok(), "listener-backed SSH readiness should pass");
    }

    #[test]
    fn refresh_machine_state_marks_missing_pids_as_stale() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let layout = MachineRootLayout::new(
            temp_dir.path().join("config"),
            temp_dir.path().join("state"),
            temp_dir.path().join("runtime"),
        );
        let paths = layout.paths("default");
        paths
            .ensure_runtime_directories()
            .expect("runtime directories should exist");

        let mut state = MachineStateRecord::initialized();
        state.lifecycle = MachineLifecycle::Running;
        state.manager = MachineManagerState::Ready;
        state.runtime = Some(MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
                gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
            },
            image_path: PathBuf::from("/tmp/disk.raw"),
            efi_variable_store_path: paths.efi_variable_store_path.clone(),
            ssh_port: 2222,
            rest_uri: format!("unix://{}", paths.krunkit_endpoint_path.display()),
            ready_vsock_port: READY_VSOCK_PORT,
        });

        refresh_machine_state(&paths, &mut state).expect("refresh should succeed");

        assert_eq!(state.lifecycle, MachineLifecycle::Failed);
        assert_eq!(state.manager, MachineManagerState::Stale);
        assert!(
            state
                .last_error
                .expect("stale error should be present")
                .contains("krunkit_alive=false")
        );
    }

    #[test]
    fn ssh_command_requires_running_machine_and_identity() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        fs::write(&image_path, []).expect("image should write");
        let mut config = sample_config(&image_path);
        config.guest.ssh_identity_path = None;

        let mut state = MachineStateRecord::initialized();
        state.lifecycle = MachineLifecycle::Running;
        state.runtime = Some(MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
                gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
            },
            image_path,
            efi_variable_store_path: PathBuf::from("/tmp/efi"),
            ssh_port: 2222,
            rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
            ready_vsock_port: READY_VSOCK_PORT,
        });

        let error = build_ssh_command(&config, &state).expect_err("missing identity should fail");
        assert!(error.to_string().contains("no SSH identity configured"));
    }

    #[test]
    fn ssh_command_applies_localhost_machine_safety_options() {
        let temp_dir = TempDir::new().expect("temp dir should exist");
        let image_path = temp_dir.path().join("disk.raw");
        let identity_path = temp_dir.path().join("machine");
        fs::write(&image_path, []).expect("image should write");
        fs::write(&identity_path, "fake-private-key").expect("identity should write");

        let mut config = sample_config(&image_path);
        config.guest.ssh_identity_path = Some(identity_path.clone());

        let mut state = MachineStateRecord::initialized();
        state.lifecycle = MachineLifecycle::Running;
        state.runtime = Some(MachineRuntimeState {
            helper_binaries: MachineHelperBinaryPaths {
                krunkit: PathBuf::from("/opt/homebrew/bin/krunkit"),
                gvproxy: PathBuf::from("/opt/homebrew/bin/gvproxy"),
            },
            image_path,
            efi_variable_store_path: PathBuf::from("/tmp/efi"),
            ssh_port: 2222,
            rest_uri: "unix:///tmp/krunkit.sock".to_owned(),
            ready_vsock_port: READY_VSOCK_PORT,
        });

        let command = build_ssh_command(&config, &state).expect("ssh command should build");
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(
            args.windows(2)
                .any(|window| window == ["-o", "BatchMode=yes"])
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["-o", "StrictHostKeyChecking=no"])
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["-o", "UserKnownHostsFile=/dev/null"])
        );
        assert!(
            args.windows(2)
                .any(|window| window == ["-i", identity_path.to_string_lossy().as_ref()])
        );
        assert!(args.windows(2).any(|window| window == ["-p", "2222"]));
        assert_eq!(args.last().map(String::as_str), Some("core@127.0.0.1"));
    }

    fn serve_single_http_response(body: Vec<u8>, path: Option<&str>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener address should resolve");
        let request_path = path.unwrap_or("/disk.raw").to_owned();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("server should accept one request");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("response header should write");
            stream.write_all(&body).expect("response body should write");
        });
        format!("http://{}:{}{}", address.ip(), address.port(), request_path)
    }

    fn serve_fake_oci_registry(layer_body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener address should resolve");
        let repository = "example/neovex-machine-os";
        let tag = "test";
        let layer_digest = format!("sha256:{:x}", Sha256::digest(&layer_body));
        let current_arch = current_machine_oci_architectures()[0];
        let child_manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": OCI_IMAGE_MEDIA_TYPE,
            "config": {
                "mediaType": "application/vnd.oci.empty.v1+json",
                "size": 2,
                "digest": "sha256:44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
            },
            "layers": [{
                "mediaType": "application/vnd.neovex.machine.disk.layer.v1.tar+gzip",
                "size": layer_body.len(),
                "digest": layer_digest,
                "annotations": {
                    "org.opencontainers.image.title": "neovex-machine-os.raw.gz"
                }
            }]
        });
        let child_manifest_bytes =
            serde_json::to_vec(&child_manifest).expect("child manifest should serialize");
        let child_manifest_digest = format!("sha256:{:x}", Sha256::digest(&child_manifest_bytes));
        let index_manifest = serde_json::json!({
            "schemaVersion": 2,
            "mediaType": OCI_IMAGE_INDEX_MEDIA_TYPE,
            "manifests": [{
                "mediaType": OCI_IMAGE_MEDIA_TYPE,
                "size": child_manifest_bytes.len(),
                "digest": child_manifest_digest,
                "platform": {
                    "architecture": current_arch,
                    "os": OCI_MACHINE_OS
                },
                "annotations": {
                    "disktype": OCI_MACHINE_DISK_TYPE,
                    "org.opencontainers.image.source": "https://github.com/agentstation/neovex-machine-os",
                    "io.neovex.machine.attestation.repository": "agentstation/neovex-machine-os",
                    "io.neovex.machine.neovex.version": "v1.2.3"
                }
            }]
        });
        let index_manifest_bytes =
            serde_json::to_vec(&index_manifest).expect("index manifest should serialize");

        thread::spawn(move || {
            for _ in 0..8 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buffer = [0_u8; 4096];
                let read = stream.read(&mut buffer).expect("request should read");
                let request = String::from_utf8_lossy(&buffer[..read]);
                let mut parts = request
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .split_whitespace();
                let method = parts.next().unwrap_or("GET");
                let path = parts.next().unwrap_or("/");
                let (status, content_type, body) = match path {
                    "/v2/" | "/v2" => (200, "text/plain", Vec::new()),
                    _ if path == format!("/v2/{repository}/manifests/{tag}") => (
                        200,
                        OCI_IMAGE_INDEX_MEDIA_TYPE,
                        index_manifest_bytes.clone(),
                    ),
                    _ if path == format!("/v2/{repository}/manifests/{child_manifest_digest}") => {
                        (200, OCI_IMAGE_MEDIA_TYPE, child_manifest_bytes.clone())
                    }
                    _ if path == format!("/v2/{repository}/blobs/{layer_digest}") => {
                        (200, "application/octet-stream", layer_body.clone())
                    }
                    _ => (404, "text/plain", b"not found".to_vec()),
                };
                let status_line = if status == 200 {
                    "HTTP/1.1 200 OK"
                } else {
                    "HTTP/1.1 404 Not Found"
                };
                let response = format!(
                    "{status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("response header should write");
                if method != "HEAD" {
                    stream.write_all(&body).expect("response body should write");
                }
            }
        });

        format!("docker://127.0.0.1:{}/{repository}:{tag}", address.port())
    }

    #[test]
    fn attestation_repository_prefers_explicit_metadata() {
        assert_eq!(
            attestation_repositories_for_reference(
                "agentstation/neovex-machine-os",
                Some("agentstation/neovex")
            ),
            vec!["agentstation/neovex".to_owned()]
        );
    }

    #[test]
    fn attestation_repository_falls_back_to_known_repo_order() {
        assert_eq!(
            attestation_repositories_for_reference("agentstation/neovex-machine-os", None),
            vec![
                "agentstation/neovex-machine-os".to_owned(),
                "agentstation/neovex".to_owned()
            ]
        );
    }

    #[test]
    fn machine_artifact_metadata_uses_primary_then_fallback_annotations() {
        let mut primary = BTreeMap::new();
        primary.insert(
            OCI_ANNOTATION_MACHINE_ATTESTATION_REPOSITORY.to_owned(),
            "agentstation/neovex".to_owned(),
        );
        let mut fallback = BTreeMap::new();
        fallback.insert(
            OCI_ANNOTATION_SOURCE.to_owned(),
            "https://github.com/agentstation/neovex-machine-os".to_owned(),
        );
        fallback.insert(
            OCI_ANNOTATION_MACHINE_NEOVEX_VERSION.to_owned(),
            "v1.2.3".to_owned(),
        );

        let metadata = machine_artifact_metadata_from_annotations(Some(&primary), Some(&fallback));

        assert_eq!(
            metadata.attestation_repository.as_deref(),
            Some("agentstation/neovex")
        );
        assert_eq!(
            metadata.source_repository_url.as_deref(),
            Some("https://github.com/agentstation/neovex-machine-os")
        );
        assert_eq!(metadata.neovex_version.as_deref(), Some("v1.2.3"));
    }
}
