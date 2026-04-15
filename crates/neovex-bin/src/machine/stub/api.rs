use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use neovex::{Error, SandboxBackend};
use neovex_sandbox::backends::container::OciMachinePortForwarderConfig;

use super::{MachineApiCommand, MachineRootLayout};

#[derive(Clone)]
pub(crate) struct MachineApiState {
    pub(crate) control_data_dir: PathBuf,
    pub(crate) listen_mode: MachineApiListenMode,
    pub(crate) binary_lookup_path: Option<OsString>,
    pub(crate) service_backend: Option<Arc<dyn SandboxBackend>>,
    pub(crate) machine_port_forwarder: Option<OciMachinePortForwarderConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MachineApiListenMode {
    DirectSocket,
    SystemdSocketActivation,
}

impl MachineApiListenMode {
    #[allow(dead_code)]
    fn as_str(self) -> &'static str {
        match self {
            Self::DirectSocket => "direct-socket",
            Self::SystemdSocketActivation => "systemd-socket-activation",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StubMachineApiListener {
    #[allow(dead_code)]
    socket_path: PathBuf,
}

pub(super) async fn run_machine_api_command(
    _command: MachineApiCommand,
    _roots: &MachineRootLayout,
) -> Result<(), Error> {
    Err(unsupported_machine_api_error())
}

pub(crate) async fn serve_machine_api<F>(
    _listener: StubMachineApiListener,
    _state: MachineApiState,
    _shutdown: F,
) -> Result<(), Error>
where
    F: Future<Output = ()> + Send + 'static,
{
    Err(unsupported_machine_api_error())
}

pub(crate) fn bind_direct_listener(path: &Path) -> Result<StubMachineApiListener, Error> {
    Ok(StubMachineApiListener {
        socket_path: path.to_path_buf(),
    })
}

fn unsupported_machine_api_error() -> Error {
    Error::InvalidInput(
        "neovex machine API is only available on unix hosts; Windows builds keep the CLI surface but do not provide machine control"
            .to_owned(),
    )
}
