//! Deterministic cross-crate fixture for the real Krun network adapter.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::backends::oci::network::{AttachmentAttachAuthority, FixedOciEgressPinProvider};
use crate::{
    SandboxBackendKind, SandboxExecutionTeardownCommand, SandboxNetworkTeardownCommand,
    SandboxOwnerSpec, SandboxProcessSpec, SandboxProvisionNetworkPlan, SandboxRootSpec,
    SandboxSpec, SandboxStatus,
};

use super::teardown::state::KrunStopProgress;
use super::{KrunLaunchAuthority, KrunSandboxBackend, KrunSandboxBackendConfig};

pub(crate) fn prepare_network_teardown_fixture(
    root: &Path,
    stopped: &SandboxExecutionTeardownCommand,
    detached: &SandboxNetworkTeardownCommand,
    plan: SandboxProvisionNetworkPlan,
    pep_port: u16,
    release_pep_reservation: impl FnOnce(),
) -> crate::Result<KrunSandboxBackendConfig> {
    crate::backends::test_hooks::validate_network_teardown_fixture(
        SandboxBackendKind::Krun,
        "nimbus-sandbox.krun-execution",
        crate::backends::KRUN_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY,
        stopped,
        detached,
        &plan,
    )?;
    let mut config = KrunSandboxBackendConfig::under_root(root);
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = reopen_network_teardown_fixture(&config);
    let rootfs = root.join("fixture-rootfs");
    std::fs::create_dir_all(&rootfs).map_err(fixture_io)?;
    let spec = SandboxSpec::new(
        detached.tenant_id().clone(),
        SandboxOwnerSpec::standalone_named("compute-network-substitution"),
        SandboxBackendKind::Krun,
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

    let mut manifest = backend
        .read_manifest(detached.sandbox_id())?
        .ok_or_else(|| crate::SandboxError::NotFound {
            sandbox_id: detached.sandbox_id().as_str().to_owned(),
        })?;
    let reservation_claim = manifest.require_reserved_claim()?.clone();
    manifest.mark_adopting()?;
    backend.persist_effect_barrier(&manifest, "compute fixture Krun adoption intent")?;
    let network_config = manifest.require_network_config()?.clone();
    backend.segment_allocator.adopt_reserved_attachment(
        detached.tenant_id(),
        detached.attachment_id(),
        &reservation_claim,
    )?;
    manifest.mark_adopted()?;
    backend.persist_effect_barrier(&manifest, "compute fixture Krun adoption result")?;
    {
        let ports = backend.port_lease_coordinator();
        let hostname = super::start::hostname_for(&manifest.spec);
        backend
            .non_routable_attachment_adapter(&manifest, &network_config, &hostname)
            .attach_with_test_host(
                &backend.attachment_lifecycle(&ports),
                AttachmentAttachAuthority::FreshLaunch(&reservation_claim),
                |_| {
                    backend.egress_pin_provider.apply(
                        &manifest.network_layout,
                        manifest.egress_proxy.as_ref().ok_or_else(|| {
                            crate::SandboxError::OperationFailed {
                                message: "Krun fixture omitted its planned PEP".to_owned(),
                            }
                        })?,
                    )
                },
            )?;
    }
    backend.start_planned_provision_pep(&manifest, &reservation_claim)?;

    let mut attached = backend
        .read_manifest(detached.sandbox_id())?
        .ok_or_else(|| crate::SandboxError::NotFound {
            sandbox_id: detached.sandbox_id().as_str().to_owned(),
        })?;
    attached.launch_authority = KrunLaunchAuthority::ProviderOwned;
    attached.status = SandboxStatus::Ready;
    attached.handle.status = SandboxStatus::Ready;
    attached
        .execution_teardown
        .set_stop(KrunStopProgress::ExecutionStopped {
            fence: stopped.provider_claim().clone(),
            evidence: b"deterministic exact ExecutionStopped fixture".to_vec(),
        });
    backend.persist_effect_barrier(&attached, "compute fixture Krun execution stopped")?;
    Ok(config)
}

pub(crate) fn reopen_network_teardown_fixture(
    config: &KrunSandboxBackendConfig,
) -> KrunSandboxBackend {
    KrunSandboxBackend::new(config.clone())
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()))
}

fn fixture_io(error: std::io::Error) -> crate::SandboxError {
    crate::SandboxError::OperationFailed {
        message: format!("Krun network fixture I/O failed: {error}"),
    }
}
