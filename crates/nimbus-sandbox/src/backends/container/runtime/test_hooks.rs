//! Deterministic cross-crate fixture for the real Container network adapter.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backends::oci::network::FixedOciEgressPinProvider;
use crate::{
    SandboxBackendKind, SandboxExecutionTeardownCommand, SandboxNetworkTeardownCommand,
    SandboxOwnerSpec, SandboxProcessSpec, SandboxProvisionNetworkPlan, SandboxRootSpec,
    SandboxSpec,
};

use super::teardown::state::ContainerStopProgress;
use super::{ContainerSandboxBackend, ContainerSandboxBackendConfig};

pub(crate) fn prepare_network_teardown_fixture(
    root: &Path,
    stopped: &SandboxExecutionTeardownCommand,
    detached: &SandboxNetworkTeardownCommand,
    plan: SandboxProvisionNetworkPlan,
    pep_port: u16,
    release_pep_reservation: impl FnOnce(),
) -> crate::Result<ContainerSandboxBackendConfig> {
    crate::backends::test_hooks::validate_network_teardown_fixture(
        SandboxBackendKind::Container,
        "nimbus-sandbox.container-execution",
        crate::backends::CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
        stopped,
        detached,
        &plan,
    )?;
    let mut config = ContainerSandboxBackendConfig::under_root(root);
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = reopen_network_teardown_fixture(&config);
    let rootfs = root.join("fixture-rootfs");
    std::fs::create_dir_all(&rootfs).map_err(fixture_io)?;
    let spec = SandboxSpec::new(
        detached.tenant_id().clone(),
        SandboxOwnerSpec::standalone_named("compute-network-substitution"),
        SandboxBackendKind::Container,
        SandboxRootSpec::rootfs(rootfs),
        SandboxProcessSpec::new(["/bin/true"]),
    );
    release_pep_reservation();
    backend.reserve_provision_network(
        spec,
        detached.sandbox_id().clone(),
        detached.execution_attempt_id().clone(),
        plan,
    )?;
    backend.prepare_provision_workload(detached.sandbox_id(), detached.execution_attempt_id())?;
    backend.attach_provision_network_with_test_host(
        detached.sandbox_id(),
        detached.execution_attempt_id(),
    )?;

    let mut manifest = backend
        .read_manifest(detached.sandbox_id())?
        .ok_or_else(|| crate::SandboxError::NotFound {
            sandbox_id: detached.sandbox_id().as_str().to_owned(),
        })?;
    manifest
        .execution_teardown
        .set_stop(ContainerStopProgress::ExecutionStopped {
            fence: stopped.provider_claim().clone(),
            evidence: b"deterministic exact ExecutionStopped fixture".to_vec(),
        });
    backend.write_existing_workload_manifest(&manifest)?;
    Ok(config)
}

pub(crate) fn reopen_network_teardown_fixture(
    config: &ContainerSandboxBackendConfig,
) -> ContainerSandboxBackend {
    ContainerSandboxBackend::new(config.clone())
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()))
}

fn fixture_io(error: std::io::Error) -> crate::SandboxError {
    crate::SandboxError::OperationFailed {
        message: format!("Container network fixture I/O failed: {error}"),
    }
}
