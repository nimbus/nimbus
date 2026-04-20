pub(super) use super::*;

use std::collections::BTreeMap;

use neovex_core::TenantId;

pub(super) use std::path::PathBuf;

pub(super) use crate::backend::SandboxBackendKind;
use crate::backends::oci::buildah::{
    OciExposedPort, OciExposedPortProtocol, OciImageLaunchDefaults,
};
use crate::backends::oci::materializer::MaterializedImageRootfs;
use crate::backends::oci::network::OciMachinePortForwarderConfig;
pub(super) use crate::instance::{SandboxId, SandboxStatus};
pub(super) use crate::spec::{
    SandboxFilesystemSpec, SandboxPortBinding, SandboxProcessSpec, SandboxSpec,
};

pub(super) fn sample_spec() -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new("svc-demo").expect("tenant should parse"),
        "db",
        SandboxBackendKind::Container,
        SandboxFilesystemSpec::new(PathBuf::from("/tmp/rootfs")),
        SandboxProcessSpec::new(["/bin/sh", "-c", "sleep 60"]),
    )
}

pub(super) fn sample_launch_defaults(rootfs_path: PathBuf) -> OciImageLaunchDefaults {
    OciImageLaunchDefaults {
        filesystem: SandboxFilesystemSpec::new(rootfs_path),
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
        filesystem: SandboxFilesystemSpec::new(rootfs_path),
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
    OciMachinePortForwarderConfig {
        host: "127.0.0.1".to_owned(),
        port,
        path_prefix: "/services/forwarder".to_owned(),
    }
}

pub(super) fn sample_plan_only_backend(root: &std::path::Path) -> ContainerSandboxBackend {
    ContainerSandboxBackend::new(ContainerSandboxBackendConfig {
        launch_mode: ContainerLaunchMode::PlanOnly,
        ..ContainerSandboxBackendConfig::under_root(root)
    })
}

pub(super) fn sandbox_id() -> SandboxId {
    SandboxId::new("db-01")
}
