pub(super) use std::collections::BTreeMap;
pub(super) use std::fs;
pub(super) use std::io::{Read, Write};
pub(super) use std::net::{SocketAddr, TcpListener};
pub(super) use std::os::unix::fs::PermissionsExt;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::thread;
pub(super) use std::time::Duration;

pub(super) use flate2::{Compression, write::GzEncoder};
pub(super) use futures::executor::block_on;
pub(super) use serde_json::json;
pub(super) use sha2::{Digest, Sha256};
pub(super) use tar::Builder;
pub(super) use tempfile::TempDir;

pub(super) use nimbus_core::TenantId;
pub(super) use nimbus_egress::EgressPolicy;
pub(super) use nimbus_proxy::{WorkloadPep, WorkloadPepConfig};

pub(super) use super::super::{
    GUEST_USER_GID_ENV, GUEST_USER_HELPER_GUEST_PATH, GUEST_USER_UID_ENV, GuestUserIds,
    KrunCreatorHandoffState, KrunEffectBarrierFailureStage, KrunEffectBarrierTestProbe,
    KrunImageMetadata, KrunLaunchAuthority, KrunLifecycleLockTestProbe,
    KrunProviderFailureCleanupState, KrunRuntimeAbsenceProof, KrunSandboxBackend,
    KrunSandboxBackendConfig, KrunSandboxManifest, KrunStartMode, KrunStartPlan,
    configured_stop_signal, configured_stop_timeout, desired_krun_vm_config, krun_vm_config_path,
    parse_guest_user, published_endpoints, running_status, slugify, visible_published_endpoints,
};
pub(super) use crate::backend::{SandboxBackend, SandboxBackendKind};
pub(super) use crate::backends::conmon::lifecycle::RestartLaunchTestProbe;
pub(super) use crate::backends::oci::buildah::{
    ImageHealthcheck, OciExposedPort, OciExposedPortProtocol, OciImageLaunchDefaults,
};
pub(super) use crate::backends::oci::command::CommandSpec;
pub(super) use crate::backends::oci::materializer::{
    MaterializedImageRootfs, PreparedMaterializedImageLaunch,
};
pub(super) use crate::backends::readiness_probe::{
    FixedReadinessProbeProvider, ReadinessProbeObservation, ReadinessProbeTarget,
    readiness_probe_target,
};
pub(super) use crate::inspection::{
    SandboxCleanupObservation, SandboxExecutionObservation, SandboxRestartAssessment,
    SandboxRestartBlocker, SandboxRestartIneligibility,
};
pub(super) use crate::instance::{SandboxId, SandboxStatus};
use crate::provision::test_support::legacy_start_attachment_network_plan_fixture;
pub(super) use crate::provision::test_support::sandbox_provision_network_plan_fixture as sample_provision_network_plan;
pub(super) use crate::spec::{
    SandboxMountSpec, SandboxOciBuildSpec, SandboxOciImageSource, SandboxOwnerSpec,
    SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits, SandboxResourceQuotaPolicy,
    SandboxRestartPolicy, SandboxRootSpec, SandboxRootfsSpec, SandboxSpec,
};
pub(super) use nimbus_network::EndpointProtocol;

pub(super) fn sample_spec() -> SandboxSpec {
    sample_spec_with_rootfs(Path::new("/srv/rootfs"))
}

/// Preserve plan-only lowering coverage after removal of the production
/// coarse-start authority. This fixture deliberately exercises only the
/// test-only planner and provider-local artifact materialization; it does not
/// attach, activate, publish, or stand in for the compute provision saga.
pub(super) fn materialize_plan_only_fixture(
    backend: &KrunSandboxBackend,
    spec: SandboxSpec,
) -> crate::Result<crate::SandboxHandle> {
    let launch_plan = backend.plan_start(&spec)?;
    materialize_plan_only_plan_fixture(backend, launch_plan)
}

pub(super) fn materialize_plan_only_plan_fixture(
    backend: &KrunSandboxBackend,
    launch_plan: KrunStartPlan,
) -> crate::Result<crate::SandboxHandle> {
    backend.materialize_krun_vm_config(&launch_plan.manifest)?;
    let mut manifest = launch_plan.manifest;
    manifest.last_exit_code = None;
    manifest.shutdown_requested = false;
    backend.write_manifest(&manifest)?;
    Ok(manifest.handle)
}

/// Loopback bind address (ephemeral port) for starting test egress PEPs without
/// needing the bridge gateway interface.
pub(super) fn loopback_addr() -> SocketAddr {
    SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 0)
}

pub(super) fn sample_spec_with_rootfs(rootfs: &Path) -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        SandboxOwnerSpec::service("postgres-primary"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new(rootfs)),
        SandboxProcessSpec::new(["/usr/bin/postgres", "-D", "/var/lib/postgresql/data"])
            .with_env(["PATH=/usr/bin", "PGDATA=/var/lib/postgresql/data"]),
    )
    .with_port_bindings([
        SandboxPortBinding::tcp("postgres", 15432, 5432),
        SandboxPortBinding::tcp("health", 18080, 8080),
    ])
}

pub(super) fn sparse_image_spec(name: &str) -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        SandboxOwnerSpec::service(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image_reference(
            "registry.example.com/acme/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
}

pub(super) fn sparse_build_spec(
    name: &str,
    image_name: impl Into<String>,
    dockerfile_path: impl Into<PathBuf>,
    context_path: impl Into<PathBuf>,
) -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        SandboxOwnerSpec::service(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::oci_image(SandboxOciImageSource::Build(SandboxOciBuildSpec::new(
            image_name,
            dockerfile_path,
            context_path,
        ))),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
}

pub(super) fn sample_spec_for_tenant(tenant_id: &str, name: &str) -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new(tenant_id).expect("tenant id should be valid"),
        SandboxOwnerSpec::service(name),
        SandboxBackendKind::Krun,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/srv/rootfs")),
        SandboxProcessSpec::new(["/usr/bin/service"]),
    )
}

pub(super) fn manifest_path(root: &Path, spec: &SandboxSpec, sandbox_id: &SandboxId) -> PathBuf {
    crate::artifact_paths::manifest_path(&root.join("state"), &spec.tenant_id, sandbox_id)
}

pub(super) fn bundle_config_path(
    root: &Path,
    spec: &SandboxSpec,
    sandbox_id: &SandboxId,
) -> PathBuf {
    crate::artifact_paths::bundle_dir(&root.join("bundles"), &spec.tenant_id, sandbox_id)
        .join("config.json")
}

pub(super) fn rootfs_artifact_path(
    root: &Path,
    spec: &SandboxSpec,
    sandbox_id: &SandboxId,
) -> PathBuf {
    crate::artifact_paths::rootfs_root(&root.join("state"), &spec.tenant_id, sandbox_id)
        .join(sandbox_id.as_str())
}

pub(super) fn sample_launch_defaults() -> OciImageLaunchDefaults {
    OciImageLaunchDefaults {
        rootfs: SandboxRootfsSpec::new("/image/rootfs"),
        process: SandboxProcessSpec::new(["/usr/local/bin/service", "serve"])
            .with_env(["PATH=/usr/local/bin:/usr/bin", "SERVICE_MODE=prod"])
            .with_cwd("/srv/service"),
        exposed_ports: vec![
            OciExposedPort {
                port: 8080,
                protocol: OciExposedPortProtocol::Tcp,
                raw: "8080/tcp".to_owned(),
            },
            OciExposedPort {
                port: 8443,
                protocol: OciExposedPortProtocol::Tcp,
                raw: "8443/tcp".to_owned(),
            },
        ],
        user: Some("1000:1000".to_owned()),
        stop_signal: Some("SIGTERM".to_owned()),
        healthcheck: Some(ImageHealthcheck {
            test: vec![
                "CMD-SHELL".to_owned(),
                "curl -f http://localhost/health".to_owned(),
            ],
            interval: Some(15_000_000_000),
            timeout: Some(3_000_000_000),
            start_period: Some(20_000_000_000),
            retries: Some(5),
        }),
        labels: BTreeMap::from([("com.example.service".to_owned(), "edge".to_owned())]),
    }
}

pub(super) fn sample_image_metadata() -> KrunImageMetadata {
    KrunImageMetadata::default()
}

pub(super) fn sample_manifest(spec: SandboxSpec, start_mode: KrunStartMode) -> KrunSandboxManifest {
    let endpoints = visible_published_endpoints(start_mode, &spec, SandboxStatus::Starting);
    let sandbox_id = crate::instance::SandboxId::new("sandbox-01");
    let attachment_network_plan = (start_mode == KrunStartMode::Execute).then(|| {
        legacy_start_attachment_network_plan_fixture(&spec, &sandbox_id, "krun-sample-manifest")
    });
    let network_config = attachment_network_plan.as_ref().map(|plan| {
        let mut config = super::super::OciNetworkConfig::default();
        config.attachment_id =
            crate::backends::oci::network::default_network_attachment_id(&sandbox_id);
        config.network_plan = Some(plan.clone());
        config
    });
    let network_layout =
        super::super::OciNetworkLayout::under_root("/tmp/state", &spec.tenant_id, &sandbox_id);
    KrunSandboxManifest {
        handle: crate::instance::SandboxHandle::new(
            spec.tenant_id.clone(),
            sandbox_id.clone(),
            spec.display_name().to_owned(),
            SandboxBackendKind::Krun,
            SandboxStatus::Starting,
            endpoints,
        ),
        execution_attempt_id: crate::SandboxExecutionAttemptId::new("wea_test").unwrap(),
        spec,
        image_metadata: KrunImageMetadata::default(),
        launch_artifact: None,
        provision_prepared: true,
        bundle_layout: super::super::KrunBundleLayout::new("/tmp/bundle"),
        conmon_layout: super::super::OciConmonLayout::new("/tmp/state", &sandbox_id),
        network_layout,
        provision_network_plan: None,
        network_config,
        port_leases: Vec::new(),
        launch_authority: match start_mode {
            KrunStartMode::PlanOnly => KrunLaunchAuthority::PlanOnly,
            KrunStartMode::Execute => KrunLaunchAuthority::ProviderOwned,
        },
        creator_handoff: match start_mode {
            KrunStartMode::PlanOnly => KrunCreatorHandoffState::NotSpawned,
            KrunStartMode::Execute => KrunCreatorHandoffState::RuntimeObserved {
                receipt: crate::backends::conmon::creator::CreatorAttemptReceipt::for_test(
                    "test-runtime-observed",
                ),
            },
        },
        provider_failure_cleanup: KrunProviderFailureCleanupState::Inactive,
        execution_teardown: Default::default(),
        network_teardown: Default::default(),
        egress_proxy: None,
        conmon_launch: super::super::OciConmonLaunchPlan {
            create_command: CommandSpec::new("/bin/true"),
            state_command: CommandSpec::new("/bin/true"),
            start_command: CommandSpec::new("/bin/true"),
            delete_command: CommandSpec::new("/bin/true"),
        },
        last_exit_code: None,
        start_mode,
        shutdown_requested: false,
        status: SandboxStatus::Starting,
    }
}

pub(super) fn sample_registry_image_reference() -> String {
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("fake OCI registry listener should bind");
    let address = listener
        .local_addr()
        .expect("fake OCI registry address should resolve");

    let mut layer_archive = Vec::new();
    {
        let mut encoder = GzEncoder::new(&mut layer_archive, Compression::default());
        {
            let mut tar = Builder::new(&mut encoder);
            let file_contents = b"#!/bin/sh\necho hello from demo\n";
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o755);
            header.set_size(file_contents.len() as u64);
            header.set_cksum();
            tar.append_data(&mut header, "usr/local/bin/demo", &file_contents[..])
                .expect("fake OCI layer file should append");

            let passwd_contents = b"demo:x:1000:1000:Demo:/workspace:/bin/sh\n";
            let mut passwd_header = tar::Header::new_gnu();
            passwd_header.set_mode(0o644);
            passwd_header.set_size(passwd_contents.len() as u64);
            passwd_header.set_cksum();
            tar.append_data(&mut passwd_header, "etc/passwd", &passwd_contents[..])
                .expect("fake OCI passwd should append");

            let group_contents = b"demo:x:1000:\n";
            let mut group_header = tar::Header::new_gnu();
            group_header.set_mode(0o644);
            group_header.set_size(group_contents.len() as u64);
            group_header.set_cksum();
            tar.append_data(&mut group_header, "etc/group", &group_contents[..])
                .expect("fake OCI group should append");
            tar.finish().expect("fake OCI tar archive should finish");
        }
        encoder
            .finish()
            .expect("fake OCI gzip archive should finish");
    }

    let config = serde_json::json!({
        "architecture": "amd64",
        "os": "linux",
        "config": {
            "Entrypoint": ["/usr/local/bin/demo"],
            "Cmd": ["serve"],
            "Env": ["PATH=/usr/local/bin:/usr/bin", "SERVICE_MODE=prod"],
            "User": "demo",
            "WorkingDir": "/workspace",
            "ExposedPorts": {
                "8080/tcp": {}
            },
            "Labels": {
                "app": "demo"
            }
        }
    });
    let config_bytes = serde_json::to_vec(&config).expect("fake OCI config should serialize");
    let config_digest = format!("sha256:{:x}", Sha256::digest(&config_bytes));
    let layer_digest = format!("sha256:{:x}", Sha256::digest(&layer_archive));
    let child_manifest = serde_json::json!({
        "schemaVersion": 2,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "size": config_bytes.len(),
            "digest": config_digest
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar+gzip",
            "size": layer_archive.len(),
            "digest": layer_digest
        }]
    });
    let child_manifest_bytes =
        serde_json::to_vec(&child_manifest).expect("fake OCI child manifest should serialize");
    let child_manifest_digest = format!("sha256:{:x}", Sha256::digest(&child_manifest_bytes));
    let index_manifest = serde_json::json!({
        "schemaVersion": 2,
        "manifests": [{
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "size": child_manifest_bytes.len(),
            "digest": child_manifest_digest,
            "platform": {
                "architecture": if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" },
                "os": "linux"
            }
        }]
    });
    let index_manifest_bytes =
        serde_json::to_vec(&index_manifest).expect("fake OCI index manifest should serialize");

    thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.expect("fake OCI registry connection should accept");
            let mut buffer = [0_u8; 4096];
            let read = stream
                .read(&mut buffer)
                .expect("fake OCI registry request should read");
            let request = String::from_utf8_lossy(&buffer[..read]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");

            let (status, body) = match path {
                "/v2/" => (200, Vec::new()),
                "/v2/library/demo/manifests/latest" => (200, index_manifest_bytes.clone()),
                _ if path == format!("/v2/library/demo/manifests/{child_manifest_digest}") => {
                    (200, child_manifest_bytes.clone())
                }
                _ if path == format!("/v2/library/demo/blobs/{config_digest}") => {
                    (200, config_bytes.clone())
                }
                _ if path == format!("/v2/library/demo/blobs/{layer_digest}") => {
                    (200, layer_archive.clone())
                }
                _ => (404, Vec::new()),
            };

            let response = format!(
                "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                if status == 200 { "OK" } else { "Not Found" },
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .expect("fake OCI registry response head should write");
            stream
                .write_all(&body)
                .expect("fake OCI registry response body should write");
        }
    });

    format!("docker://localhost:{}/library/demo:latest", address.port())
}

pub(super) trait ImageMetadataTestExt {
    fn with_stop_signal(self, stop_signal: &str) -> Self;
}

impl ImageMetadataTestExt for KrunImageMetadata {
    fn with_stop_signal(mut self, stop_signal: &str) -> Self {
        self.stop_signal = Some(stop_signal.to_owned());
        self
    }
}
