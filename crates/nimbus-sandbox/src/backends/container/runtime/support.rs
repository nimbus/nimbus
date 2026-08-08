pub(super) use super::*;

use std::collections::BTreeMap;

use nimbus_core::TenantId;

pub(super) use std::path::PathBuf;

pub(super) use crate::backend::SandboxBackendKind;
use crate::backends::oci::buildah::{
    OciExposedPort, OciExposedPortProtocol, OciImageLaunchDefaults,
};
use crate::backends::oci::materializer::MaterializedImageRootfs;
use crate::backends::oci::network::OciMachinePortForwarderConfig;
pub(super) use crate::instance::{SandboxId, SandboxStatus};
pub(super) use crate::provision::test_support::sandbox_provision_network_plan_fixture as sample_provision_network_plan;
pub(super) use crate::spec::{
    SandboxMountSpec, SandboxOwnerSpec, SandboxPortBinding, SandboxProcessSpec,
    SandboxRestartPolicy, SandboxRootSpec, SandboxRootfsSpec, SandboxSpec,
};
pub(super) use crate::spec::{SandboxResourceLimits, SandboxResourceQuotaPolicy};

pub(super) fn sample_spec() -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new("svc-demo").expect("tenant should parse"),
        SandboxOwnerSpec::service("db"),
        SandboxBackendKind::Container,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new(PathBuf::from("/tmp/rootfs"))),
        SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
}

pub(super) fn sample_spec_for_tenant(tenant_id: &str, name: &str) -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new(tenant_id).expect("tenant should parse"),
        SandboxOwnerSpec::service(name),
        SandboxBackendKind::Container,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new(PathBuf::from("/tmp/rootfs"))),
        SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
}

pub(super) fn sample_launch_defaults(rootfs_path: PathBuf) -> OciImageLaunchDefaults {
    OciImageLaunchDefaults {
        rootfs: SandboxRootfsSpec::new(rootfs_path),
        process: SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
        exposed_ports: Vec::new(),
        user: None,
        stop_signal: None,
        healthcheck: None,
        labels: BTreeMap::new(),
    }
}

pub(super) fn exposed_port_launch_defaults(rootfs_path: PathBuf) -> OciImageLaunchDefaults {
    OciImageLaunchDefaults {
        rootfs: SandboxRootfsSpec::new(rootfs_path),
        process: SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
        exposed_ports: vec![OciExposedPort {
            port: 8080,
            protocol: OciExposedPortProtocol::Tcp,
            raw: "8080/tcp".to_owned(),
        }],
        user: None,
        stop_signal: None,
        healthcheck: None,
        labels: BTreeMap::new(),
    }
}

pub(super) fn sample_rootfs_artifact(rootfs_path: PathBuf) -> ContainerLaunchArtifact {
    ContainerLaunchArtifact::Rootfs(MaterializedImageRootfs {
        image_reference: "docker.io/library/demo:latest".to_owned(),
        rootfs_path,
    })
}

pub(super) fn sample_forwarder(port: u16) -> OciMachinePortForwarderConfig {
    OciMachinePortForwarderConfig::for_provider_instance(
        "127.0.0.1",
        port,
        "/services/forwarder",
        format!("test-forwarder:{port}"),
        nimbus_network::NetworkResourceGeneration::new(1),
    )
    .expect("test machine-forwarder identity should validate")
}

pub(super) fn sample_plan_only_backend(root: &std::path::Path) -> ContainerSandboxBackend {
    ContainerSandboxBackend::new(ContainerSandboxBackendConfig {
        start_mode: ContainerStartMode::PlanOnly,
        ..ContainerSandboxBackendConfig::under_root(root)
    })
}

pub(super) fn sample_execution_attempt_id(
    sandbox_id: &SandboxId,
) -> crate::SandboxExecutionAttemptId {
    crate::SandboxExecutionAttemptId::new(format!("test-execution-attempt:{sandbox_id}"))
        .expect("test execution attempt should validate")
}

/// Drive the two non-effectful PlanOnly provision phases in tests without
/// recreating the deleted production coarse-start authority.
pub(super) fn reserve_and_prepare_plan_only_fixture(
    backend: &ContainerSandboxBackend,
    spec: SandboxSpec,
    label: &str,
) -> Result<SandboxHandle> {
    let sandbox_id = SandboxId::new(format!("plan-only-{label}"));
    let network_plan = sample_provision_network_plan(&spec, &sandbox_id, label);
    let execution_attempt_id = sample_execution_attempt_id(&sandbox_id);
    backend.reserve_provision_network(
        spec,
        sandbox_id.clone(),
        execution_attempt_id.clone(),
        network_plan,
    )?;
    backend.prepare_provision_workload(&sandbox_id, &execution_attempt_id)
}

pub(super) fn mark_runtime_absent_for_cleanup(manifest: &mut ContainerSandboxManifest) {
    manifest.creator_handoff = ContainerCreatorHandoffState::Quiesced {
        proof: crate::backends::conmon::creator::CreatorQuiescenceProof::never_spawned(
            "test-confirmed-no-creator",
        ),
    };
    manifest.conmon_launch.delete_command =
        crate::backends::oci::command::CommandSpec::new("/usr/bin/true");
    manifest.conmon_launch.state_command =
        crate::backends::oci::command::CommandSpec::new("/bin/sh").args([
            "-c".to_owned(),
            format!(
                "printf '%s\\n' 'container `{0}` does not exist: open \
                 `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
                manifest.handle.id
            ),
        ]);
}

pub(super) fn publish_present_runner_lifecycle(
    manifest: &ContainerSandboxManifest,
    handoff: &super::runner::RunnerHandoffGuard,
) {
    super::runner::record_runner_effect_outcome(
        manifest,
        super::runner::RunnerEffectOutcome::Present,
        handoff,
    )
    .expect("runner fixture should publish its exact present-effect receipt");
    super::runner::publish_runner_lifecycle_ownership(manifest, handoff)
        .expect("ordinary lifecycle ownership should publish");
}

pub(super) fn sandbox_id() -> SandboxId {
    SandboxId::new("db-01")
}
