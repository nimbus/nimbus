//! Narrow deterministic Container fixtures for upper-crate substitution tests.

use std::path::Path;

use nimbus_sandbox::{
    SandboxExecutionTeardownCommand, SandboxNetworkTeardownCommand, SandboxProvisionNetworkPlan,
};

use crate::{ContainerSandboxBackend, ContainerSandboxBackendConfig};

/// Durable Container fixture that can create independent backend instances.
#[doc(hidden)]
pub struct PreparedContainerNetworkTeardown {
    config: ContainerSandboxBackendConfig,
}

impl PreparedContainerNetworkTeardown {
    /// Prepare one exact attached workload with durable `ExecutionStopped` evidence.
    pub fn new(
        root: &Path,
        stopped: &SandboxExecutionTeardownCommand,
        detached: &SandboxNetworkTeardownCommand,
        plan: SandboxProvisionNetworkPlan,
        pep_port: u16,
        release_pep_reservation: impl FnOnce(),
    ) -> nimbus_sandbox::Result<Self> {
        super::prepare_network_teardown_fixture(
            root,
            stopped,
            detached,
            plan,
            pep_port,
            release_pep_reservation,
        )
        .map(|config| Self { config })
    }

    /// Reopen only from the fixture's durable roots.
    pub fn reopen(&self) -> ContainerSandboxBackend {
        super::reopen_network_teardown_fixture(&self.config)
    }
}
