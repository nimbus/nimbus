use neovex::{
    SandboxBackend, SandboxBackendKind, SandboxBuildLaunchSpec, SandboxError, SandboxHandle,
    SandboxId, SandboxImageLaunchSpec, SandboxSpec,
};
use neovex_sandbox::SandboxFuture;

use super::client::MachineApiClient;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForwardedMachineApiSandboxBackend {
    #[allow(dead_code)]
    client: MachineApiClient,
}

impl ForwardedMachineApiSandboxBackend {
    pub(crate) fn new(client: MachineApiClient) -> Self {
        Self { client }
    }
}

impl SandboxBackend for ForwardedMachineApiSandboxBackend {
    fn kind(&self) -> SandboxBackendKind {
        SandboxBackendKind::Container
    }

    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle> {
        let message = format!(
            "forwarded machine API backend requires image/build launches, not bare spec {}",
            spec.name
        );
        Box::pin(async move { Err(SandboxError::InvalidSpec { message }) })
    }

    fn start_from_image(&self, _launch: SandboxImageLaunchSpec) -> SandboxFuture<SandboxHandle> {
        unsupported_machine_api_backend()
    }

    fn start_from_build(&self, _launch: SandboxBuildLaunchSpec) -> SandboxFuture<SandboxHandle> {
        unsupported_machine_api_backend()
    }

    fn inspect(&self, _id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>> {
        Box::pin(async move {
            Err(SandboxError::BackendUnavailable {
                message: unsupported_machine_api_backend_message(),
            })
        })
    }

    fn stop(&self, _id: &SandboxId) -> SandboxFuture<()> {
        Box::pin(async move {
            Err(SandboxError::BackendUnavailable {
                message: unsupported_machine_api_backend_message(),
            })
        })
    }
}

fn unsupported_machine_api_backend() -> SandboxFuture<SandboxHandle> {
    Box::pin(async move {
        Err(SandboxError::BackendUnavailable {
            message: unsupported_machine_api_backend_message(),
        })
    })
}

fn unsupported_machine_api_backend_message() -> String {
    "forwarded machine API backend is only available on unix hosts; Windows builds do not provide machine-backed guest execution"
        .to_owned()
}
