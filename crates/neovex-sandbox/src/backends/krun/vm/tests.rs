use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use flate2::{Compression, write::GzEncoder};
use futures::executor::block_on;
use serde_json::json;
use sha2::{Digest, Sha256};
use tar::Builder;
use tempfile::TempDir;

use neovex_core::TenantId;

use super::{
    GUEST_USER_GID_ENV, GUEST_USER_HELPER_GUEST_PATH, GUEST_USER_UID_ENV, GuestUserIds,
    KrunImageMetadata, KrunLaunchMode, KrunSandboxBackend, KrunSandboxBackendConfig,
    KrunSandboxManifest, ReadinessProbeTarget, configured_stop_signal, configured_stop_timeout,
    desired_krun_vm_config, krun_vm_config_path, parse_guest_user, probe_target_ready,
    readiness_probe_target, restart_backoff_delay, restart_policy_allows_restart, running_status,
    slugify, visible_published_endpoints,
};
use crate::backend::{SandboxBackend, SandboxBackendKind};
use crate::backends::oci::buildah::{
    ImageHealthcheck, OciExposedPort, OciExposedPortProtocol, OciImageLaunchDefaults,
};
use crate::endpoint::PublishedEndpointProtocol;
use crate::instance::{SandboxId, SandboxStatus};
use crate::spec::{
    SandboxBuildLaunchSpec, SandboxFilesystemSpec, SandboxImageLaunchSpec,
    SandboxImageProcessOverrides, SandboxPortBinding, SandboxProcessSpec, SandboxResourceLimits,
    SandboxRestartPolicy, SandboxSpec,
};

#[test]
fn plan_only_backend_lowers_through_generic_trait_surface() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend: Box<dyn SandboxBackend> = Box::new(KrunSandboxBackend::new(
        KrunSandboxBackendConfig::plan_only(
            temp_dir.path().join("bundles"),
            temp_dir.path().join("state"),
        ),
    ));
    let spec = sample_spec();

    let handle = block_on(backend.start(spec)).expect("plan-only start should succeed");
    assert_eq!(handle.backend, SandboxBackendKind::Krun);
    assert_eq!(handle.status, crate::instance::SandboxStatus::Starting);
    assert_eq!(handle.published_endpoints.len(), 2);

    let inspected = block_on(backend.inspect(&handle.id))
        .expect("inspect should succeed")
        .expect("plan-only sandbox should persist a manifest");
    assert_eq!(inspected.id, handle.id);

    block_on(backend.stop(&handle.id)).expect("stop should succeed in plan-only mode");
    let stopped = block_on(backend.inspect(&handle.id))
        .expect("inspect after stop should succeed")
        .expect("stopped sandbox should still have a manifest");
    assert_eq!(stopped.status, crate::instance::SandboxStatus::Stopped);
}

#[test]
fn plan_only_backend_lowers_image_launch_through_generic_trait_surface() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let image_reference = sample_registry_image_reference();
    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.use_buildah_unshare = false;
    let backend: Box<dyn SandboxBackend> = Box::new(KrunSandboxBackend::new(config));

    let handle = block_on(backend.start_from_image(SandboxImageLaunchSpec::new(
        sparse_image_spec("image-trait"),
        &image_reference,
    )))
    .expect("plan-only image-backed start should succeed through the trait");

    assert_eq!(handle.backend, SandboxBackendKind::Krun);
    assert_eq!(handle.status, crate::instance::SandboxStatus::Starting);

    let inspected = block_on(backend.inspect(&handle.id))
        .expect("inspect should succeed")
        .expect("plan-only image-backed sandbox should persist a manifest");
    assert_eq!(inspected.id, handle.id);
}

#[test]
fn plan_only_backend_lowers_build_launch_through_generic_trait_surface() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace directory should exist");
    let dockerfile_path = workspace.join("Dockerfile");
    fs::write(&dockerfile_path, "FROM scratch\nCMD [\"/bin/true\"]\n")
        .expect("dockerfile should be written");

    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.use_buildah_unshare = false;
    let backend: Box<dyn SandboxBackend> = Box::new(KrunSandboxBackend::new(config));

    let handle = block_on(backend.start_from_build(SandboxBuildLaunchSpec::new(
        sparse_image_spec("build-trait"),
        "neovex-api",
        &dockerfile_path,
        &workspace,
    )))
    .expect("plan-only build-backed start should succeed through the trait");

    assert_eq!(handle.backend, SandboxBackendKind::Krun);
    assert_eq!(handle.status, crate::instance::SandboxStatus::Starting);

    let inspected = block_on(backend.inspect(&handle.id))
        .expect("inspect should succeed")
        .expect("plan-only build-backed sandbox should persist a manifest");
    assert_eq!(inspected.id, handle.id);
    let manifest_path = temp_dir
        .path()
        .join("state")
        .join("containers")
        .join(handle.id.as_str())
        .join("manifest.json");
    let manifest = fs::read_to_string(&manifest_path).expect("manifest should be readable");
    assert!(
        manifest.contains("\"Rootfs\""),
        "build-backed plan should persist a materialized rootfs launch artifact: {manifest}"
    );
    assert!(
        !manifest.contains("\"MountedRootfs\""),
        "build-backed plan should no longer depend on mounted buildah rootfs sessions: {manifest}"
    );
}

#[test]
fn plan_start_writes_bundle_and_manifest_under_backend_roots() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let spec = sample_spec();

    let handle = block_on(backend.start(spec)).expect("plan-only start should succeed");
    let manifest_dir = temp_dir
        .path()
        .join("state")
        .join("containers")
        .join(handle.id.as_str());
    let manifest_path = manifest_dir.join("manifest.json");
    let bundle_path = temp_dir
        .path()
        .join("bundles")
        .join(handle.id.as_str())
        .join("config.json");

    assert!(manifest_path.exists(), "sandbox manifest should be written");
    assert!(bundle_path.exists(), "bundle config should be written");

    let rendered_bundle =
        fs::read_to_string(bundle_path).expect("bundle config should be readable");
    assert!(
        rendered_bundle.contains("\"krun.port_map\": \"15432:5432,18080:8080\""),
        "bundle config should preserve the host:guest TSI mapping"
    );
}

#[test]
fn plan_only_start_writes_krun_vm_config_for_explicit_resource_limits() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let rootfs = temp_dir.path().join("rootfs");
    fs::create_dir_all(&rootfs).expect("rootfs directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let spec = sample_spec_with_rootfs(&rootfs).with_resource_limits(
        SandboxResourceLimits::default()
            .with_cpu_count(2)
            .with_memory_limit_bytes(256 * 1024 * 1024),
    );

    let handle = block_on(backend.start(spec)).expect("plan-only start should succeed");
    let vm_config_path = krun_vm_config_path(&rootfs);
    let vm_config =
        fs::read_to_string(&vm_config_path).expect("krun vm config should be materialized");
    let bundle = fs::read_to_string(
        temp_dir
            .path()
            .join("bundles")
            .join(handle.id.as_str())
            .join("config.json"),
    )
    .expect("bundle config should be readable");

    assert!(vm_config.contains("\"cpus\": 2"));
    assert!(vm_config.contains("\"ram_mib\": 256"));
    assert!(bundle.contains("\"limit\": 268435456"));
}

#[test]
fn plan_only_start_removes_stale_krun_vm_config_when_cpu_limit_is_unset() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let rootfs = temp_dir.path().join("rootfs");
    fs::create_dir_all(&rootfs).expect("rootfs directory should exist");
    let stale_vm_config = krun_vm_config_path(&rootfs);
    fs::write(&stale_vm_config, "{\"cpus\":4,\"ram_mib\":512}")
        .expect("stale krun vm config should be seeded");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let spec = sample_spec_with_rootfs(&rootfs).with_memory_limit_bytes(256 * 1024 * 1024);

    block_on(backend.start(spec)).expect("plan-only start should succeed");

    assert!(
        !stale_vm_config.exists(),
        "memory-only starts should remove stale krun vm config so crun uses the OCI memory limit path"
    );
}

#[test]
fn slugify_normalizes_operator_facing_names() {
    assert_eq!(slugify("Postgres Primary"), "postgres-primary");
    assert_eq!(slugify("db__1"), "db-1");
    assert_eq!(slugify("api@edge"), "api-edge");
}

#[test]
fn plan_start_with_launch_defaults_materializes_sparse_spec_from_image_defaults() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "api",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(PathBuf::new()),
        SandboxProcessSpec::new(Vec::<String>::new()),
    );

    let launch_plan = backend
        .plan_start_with_launch_defaults(&spec, Some(&sample_launch_defaults()))
        .expect("launch defaults should materialize the sparse spec");

    assert_eq!(
        launch_plan.manifest.spec.filesystem.rootfs,
        PathBuf::from("/image/rootfs")
    );
    assert_eq!(
        launch_plan.manifest.spec.process.args,
        vec![
            GUEST_USER_HELPER_GUEST_PATH.to_owned(),
            "/usr/local/bin/service".to_owned(),
            "serve".to_owned(),
        ]
    );
    assert_eq!(
        launch_plan.manifest.spec.process.env,
        vec![
            "PATH=/usr/local/bin:/usr/bin".to_owned(),
            "SERVICE_MODE=prod".to_owned(),
            format!("{GUEST_USER_UID_ENV}=1000"),
            format!("{GUEST_USER_GID_ENV}=1000"),
        ]
    );
    assert_eq!(
        launch_plan.manifest.spec.process.cwd,
        PathBuf::from("/srv/service")
    );
    assert_eq!(
        launch_plan.manifest.image_metadata.stop_signal,
        Some("SIGTERM".to_owned())
    );
    assert_eq!(
        launch_plan.manifest.image_metadata.exposed_ports,
        vec![
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
        ]
    );

    let rendered_bundle = fs::read_to_string(&launch_plan.manifest.bundle_layout.config_path)
        .expect("bundle config should be readable");
    assert!(
        rendered_bundle.contains(&format!("\"{GUEST_USER_HELPER_GUEST_PATH}\"")),
        "bundle config should wrap the image-default command with the guest user helper"
    );
    // krun bundles always use root for the VMM process (needs /dev/kvm).
    // The image user is stored in the manifest, not the bundle.
    assert!(
        rendered_bundle.contains("\"uid\": 0"),
        "krun bundle should use root uid for VMM /dev/kvm access"
    );
    assert!(
        rendered_bundle.contains("\"gid\": 0"),
        "krun bundle should use root gid for VMM /dev/kvm access"
    );
    assert!(
        rendered_bundle.contains("\"destination\": \"/.neovex\""),
        "bundle config should mount the guest helper root when image USER is set"
    );
}

#[test]
fn plan_start_with_launch_defaults_preserves_explicit_operator_overrides() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "api",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new("/operator/rootfs").read_only(true),
        SandboxProcessSpec::new(["/bin/sh", "-lc", "exec custom-api"])
            .with_env(["PATH=/custom/bin", "APP_MODE=dev"])
            .with_cwd("/workspace"),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080));

    let launch_plan = backend
        .plan_start_with_launch_defaults(&spec, Some(&sample_launch_defaults()))
        .expect("explicit operator overrides should coexist with image defaults");

    assert_eq!(
        launch_plan.manifest.spec.filesystem.rootfs,
        PathBuf::from("/operator/rootfs")
    );
    assert!(launch_plan.manifest.spec.filesystem.readonly);
    assert_eq!(
        launch_plan.manifest.spec.process.args,
        vec![
            GUEST_USER_HELPER_GUEST_PATH.to_owned(),
            "/bin/sh".to_owned(),
            "-lc".to_owned(),
            "exec custom-api".to_owned(),
        ]
    );
    assert_eq!(
        launch_plan.manifest.spec.process.env,
        vec![
            "PATH=/custom/bin".to_owned(),
            "SERVICE_MODE=prod".to_owned(),
            "APP_MODE=dev".to_owned(),
            format!("{GUEST_USER_UID_ENV}=1000"),
            format!("{GUEST_USER_GID_ENV}=1000"),
        ]
    );
    assert_eq!(
        launch_plan.manifest.spec.process.cwd,
        PathBuf::from("/workspace")
    );
    assert!(!launch_plan.manifest.spec.process.terminal);
    assert_eq!(launch_plan.manifest.spec.port_bindings.len(), 1);
    assert_eq!(
        launch_plan.manifest.image_metadata.healthcheck,
        Some(ImageHealthcheck {
            test: vec![
                "CMD-SHELL".to_owned(),
                "curl -f http://localhost/health".to_owned()
            ],
            interval: Some(15_000_000_000),
            timeout: Some(3_000_000_000),
            start_period: Some(20_000_000_000),
            retries: Some(5),
        })
    );
}

#[test]
fn start_from_image_plan_only_persists_and_then_cleans_up_materialized_rootfs() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let image_reference = sample_registry_image_reference();

    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.use_buildah_unshare = false;

    let backend = KrunSandboxBackend::new(config);
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "image-backed-api",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(PathBuf::new()),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080));

    let handle = block_on(
        backend.start_from_image(
            SandboxImageLaunchSpec::new(spec, &image_reference)
                .with_process_overrides(SandboxImageProcessOverrides::default()),
        ),
    )
    .expect("plan-only image-backed start should succeed");

    let manifest_path = temp_dir
        .path()
        .join("state")
        .join("containers")
        .join(handle.id.as_str())
        .join("manifest.json");
    let manifest_before_stop =
        fs::read_to_string(&manifest_path).expect("manifest should be readable before stop");
    assert!(
        manifest_before_stop.contains("\"launch_artifact\""),
        "manifest should retain launch-artifact metadata while running"
    );
    let rootfs_path = temp_dir
        .path()
        .join("state")
        .join("materialized-rootfs")
        .join(handle.id.as_str());
    assert!(
        rootfs_path.exists(),
        "image-backed plan should materialize a rootfs under the krun state root"
    );

    block_on(backend.stop(&handle.id)).expect("plan-only stop should succeed");

    let manifest_after_stop =
        fs::read_to_string(&manifest_path).expect("manifest should be readable after stop");
    assert!(
        manifest_after_stop.contains("\"launch_artifact\": null"),
        "stop should clear launch-artifact metadata after cleanup"
    );
    assert!(
        !rootfs_path.exists(),
        "stop should remove the materialized rootfs after cleanup"
    );
}

#[test]
fn start_from_image_plan_only_skips_krun_vm_config_prelude_for_materialized_rootfs() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let image_reference = sample_registry_image_reference();

    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.use_buildah_unshare = true;

    let backend = KrunSandboxBackend::new(config);
    let spec = sparse_image_spec("image-with-limits").with_resource_limits(
        SandboxResourceLimits::default()
            .with_cpu_count(2)
            .with_memory_limit_bytes(256 * 1024 * 1024),
    );

    let launch_plan = backend
        .plan_start_from_image(
            &spec,
            &image_reference,
            &SandboxImageProcessOverrides::default(),
        )
        .expect("image-backed plan should succeed");

    let script = launch_plan
        .manifest
        .conmon_launch
        .create_command
        .args
        .join(" ");
    assert!(
        !script.contains(".krun_vm.json"),
        "materialized rootfs launches should write krun vm config directly, not via a buildah unshare prelude: {script}"
    );
}

#[test]
fn start_from_image_plan_only_auto_assigns_exposed_ports_and_reuses_released_ports() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let image_reference = sample_registry_image_reference();

    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.use_buildah_unshare = false;
    config.published_port_range = 15000..=15001;

    let backend = KrunSandboxBackend::new(config);

    let first = block_on(backend.start_from_image(SandboxImageLaunchSpec::new(
        sparse_image_spec("first"),
        &image_reference,
    )))
    .expect("first plan-only image-backed start should succeed");
    let first_inspected = block_on(backend.inspect(&first.id))
        .expect("inspect should succeed")
        .expect("first sandbox should be persisted");
    assert_eq!(first_inspected.published_endpoints.len(), 1);
    assert_eq!(first_inspected.published_endpoints[0].address.port(), 15000);

    let second = block_on(backend.start_from_image(SandboxImageLaunchSpec::new(
        sparse_image_spec("second"),
        &image_reference,
    )))
    .expect("second plan-only image-backed start should succeed");
    let second_inspected = block_on(backend.inspect(&second.id))
        .expect("inspect should succeed")
        .expect("second sandbox should be persisted");
    assert_eq!(second_inspected.published_endpoints.len(), 1);
    assert_eq!(
        second_inspected.published_endpoints[0].address.port(),
        15001
    );

    block_on(backend.stop(&first.id)).expect("stopping the first sandbox should succeed");

    let third = block_on(backend.start_from_image(SandboxImageLaunchSpec::new(
        sparse_image_spec("third"),
        &image_reference,
    )))
    .expect("third plan-only image-backed start should succeed");
    let third_inspected = block_on(backend.inspect(&third.id))
        .expect("inspect should succeed")
        .expect("third sandbox should be persisted");
    assert_eq!(third_inspected.published_endpoints.len(), 1);
    assert_eq!(third_inspected.published_endpoints[0].address.port(), 15000);

    let third_bundle = fs::read_to_string(
        temp_dir
            .path()
            .join("bundles")
            .join(third.id.as_str())
            .join("config.json"),
    )
    .expect("third bundle config should be readable");
    assert!(
        third_bundle.contains("\"krun.port_map\": \"15000:8080\""),
        "auto-assigned bindings should rewrite the krun port map annotation"
    );
}

#[test]
fn configured_stop_signal_prefers_image_metadata_and_falls_back_to_term() {
    assert_eq!(
        configured_stop_signal(&sample_image_metadata().with_stop_signal("SIGQUIT")),
        "SIGQUIT"
    );
    assert_eq!(
        configured_stop_signal(&sample_image_metadata().with_stop_signal("  ")),
        "TERM"
    );
    assert_eq!(
        configured_stop_signal(&KrunImageMetadata::default()),
        "TERM"
    );
}

#[test]
fn configured_stop_timeout_prefers_sandbox_lifecycle_and_falls_back_to_backend_default() {
    let backend_default = KrunSandboxBackendConfig {
        stop_timeout: Duration::from_secs(5),
        ..KrunSandboxBackendConfig::default()
    };
    assert_eq!(
        configured_stop_timeout(
            &sample_spec().with_stop_timeout(Duration::from_secs(30)),
            &backend_default,
        ),
        Duration::from_secs(30)
    );
    assert_eq!(
        configured_stop_timeout(&sample_spec(), &backend_default),
        Duration::from_secs(5)
    );
}

#[test]
fn parse_guest_user_accepts_numeric_uid_and_uid_gid() {
    assert_eq!(
        parse_guest_user(Some("1234")).expect("uid should parse"),
        Some(GuestUserIds { uid: 1234, gid: 0 })
    );
    assert_eq!(
        parse_guest_user(Some("1234:5678")).expect("uid:gid should parse"),
        Some(GuestUserIds {
            uid: 1234,
            gid: 5678
        })
    );
    assert_eq!(
        parse_guest_user(Some(" ")).expect("blank user should be ignored"),
        None
    );
}

#[test]
fn parse_guest_user_rejects_non_numeric_components() {
    let error = parse_guest_user(Some("postgres:postgres"))
        .expect_err("guest user switching should require numeric ids by this stage");
    assert!(
        error.to_string().contains("requires a numeric image user"),
        "expected actionable numeric-user error, got: {error}"
    );
}

#[test]
fn readiness_probe_target_prefers_http_endpoints() {
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "api",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new("/srv/rootfs"),
        SandboxProcessSpec::new(["/bin/service"]),
    )
    .with_port_bindings([
        SandboxPortBinding::tcp("postgres", 15432, 5432),
        SandboxPortBinding::new("http", PublishedEndpointProtocol::Http, 18080, 8080),
    ]);
    let manifest = sample_manifest(spec, KrunLaunchMode::Execute);

    assert_eq!(
        readiness_probe_target(&manifest),
        Some(ReadinessProbeTarget::Http(SocketAddr::from((
            [127, 0, 0, 1],
            18080
        ))))
    );
}

#[test]
fn probe_target_ready_succeeds_for_http_listener() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should report local addr");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("listener should accept");
        let mut request = [0_u8; 256];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"HTTP/1.0 204 No Content\r\nContent-Length: 0\r\n\r\n")
            .expect("server should write response");
    });

    assert!(
        probe_target_ready(ReadinessProbeTarget::Http(address), Duration::from_secs(1)),
        "expected HTTP readiness probe to pass against local listener"
    );
    server.join().expect("server thread should join");
}

#[test]
fn running_status_stays_starting_until_probe_passes() {
    let unused_listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
    let address = unused_listener
        .local_addr()
        .expect("listener should report local addr");
    drop(unused_listener);

    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "tcp-service",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new("/srv/rootfs"),
        SandboxProcessSpec::new(["/bin/service"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("tcp", address.port(), 8080));
    let manifest = sample_manifest(spec, KrunLaunchMode::Execute);

    assert_eq!(running_status(&manifest), SandboxStatus::Starting);
}

#[test]
fn running_status_degrades_ready_sandboxes_to_not_ready_on_probe_failure() {
    let unused_listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
    let address = unused_listener
        .local_addr()
        .expect("listener should report local addr");
    drop(unused_listener);

    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "http-service",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new("/srv/rootfs"),
        SandboxProcessSpec::new(["/bin/service"]),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        address.port(),
        8080,
    ));
    let mut manifest = sample_manifest(spec, KrunLaunchMode::Execute);
    manifest.status = SandboxStatus::Ready;
    manifest.handle.status = SandboxStatus::Ready;
    manifest.handle.published_endpoints = visible_published_endpoints(
        KrunLaunchMode::Execute,
        &manifest.spec,
        SandboxStatus::Ready,
    );

    assert_eq!(running_status(&manifest), SandboxStatus::NotReady);
}

#[test]
fn running_status_recovers_not_ready_sandboxes_when_probe_returns() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should report local addr");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("listener should accept");
        let mut request = [0_u8; 256];
        let _ = stream.read(&mut request);
        stream
            .write_all(b"HTTP/1.0 204 No Content\r\nContent-Length: 0\r\n\r\n")
            .expect("server should write response");
    });

    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "http-service",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new("/srv/rootfs"),
        SandboxProcessSpec::new(["/bin/service"]),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        PublishedEndpointProtocol::Http,
        address.port(),
        8080,
    ));
    let mut manifest = sample_manifest(spec, KrunLaunchMode::Execute);
    manifest.status = SandboxStatus::NotReady;
    manifest.handle.status = SandboxStatus::NotReady;

    assert_eq!(running_status(&manifest), SandboxStatus::Ready);
    server.join().expect("server thread should join");
}

#[test]
fn detect_runtime_status_marks_stale_pidfiles_as_failed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let mut manifest = backend
        .plan_start_with_id(&sample_spec(), &SandboxId::new("db-01"), None, None)
        .expect("plan should lower")
        .manifest;
    let state_stub = temp_dir.path().join("krun-state");
    fs::write(&state_stub, "#!/bin/sh\nexit 1\n").expect("state stub should write");
    let mut permissions = fs::metadata(&state_stub)
        .expect("state stub metadata should resolve")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&state_stub, permissions).expect("state stub permissions should update");
    manifest.conmon_launch.state_command.program = state_stub;
    fs::write(&manifest.conmon_layout.pidfile, "999999\n").expect("pidfile should write");

    assert_eq!(
        backend
            .detect_runtime_status(&manifest)
            .expect("status should resolve"),
        SandboxStatus::Failed
    );
}

#[test]
fn visible_published_endpoints_hide_execute_mode_endpoints_until_ready() {
    let spec = sample_spec();

    assert!(
        visible_published_endpoints(KrunLaunchMode::Execute, &spec, SandboxStatus::Starting)
            .is_empty(),
        "execute-mode sandboxes should not publish endpoints before readiness succeeds"
    );
    assert_eq!(
        visible_published_endpoints(KrunLaunchMode::Execute, &spec, SandboxStatus::Ready).len(),
        2
    );
    assert!(
        visible_published_endpoints(KrunLaunchMode::Execute, &spec, SandboxStatus::NotReady)
            .is_empty(),
        "execute-mode sandboxes should withdraw endpoints when liveness probes regress"
    );
    assert_eq!(
        visible_published_endpoints(KrunLaunchMode::PlanOnly, &spec, SandboxStatus::Starting).len(),
        2,
        "plan-only starts should retain published endpoints for deterministic tests"
    );
}

#[test]
fn restart_policy_allows_expected_restart_shapes() {
    assert!(
        !restart_policy_allows_restart(SandboxRestartPolicy::Never, 42, 0),
        "never policy should not restart"
    );
    assert!(
        restart_policy_allows_restart(SandboxRestartPolicy::OnFailure { max_restarts: 1 }, 42, 0),
        "on-failure should restart non-zero exits within budget"
    );
    assert!(
        !restart_policy_allows_restart(SandboxRestartPolicy::OnFailure { max_restarts: 1 }, 0, 0),
        "on-failure should not restart clean exits"
    );
    assert!(
        !restart_policy_allows_restart(SandboxRestartPolicy::Always { max_restarts: 1 }, 42, 1),
        "restart budget should cap repeated restarts"
    );
}

#[test]
fn restart_backoff_delay_grows_and_caps() {
    assert_eq!(restart_backoff_delay(0), Duration::from_secs(1));
    assert_eq!(restart_backoff_delay(1), Duration::from_secs(2));
    assert_eq!(restart_backoff_delay(2), Duration::from_secs(4));
    assert_eq!(restart_backoff_delay(6), Duration::from_secs(60));
    assert_eq!(restart_backoff_delay(12), Duration::from_secs(60));
}

#[test]
fn manifest_deserialization_defaults_restart_fields_for_pre_restart_manifests() {
    let manifest: KrunSandboxManifest = serde_json::from_value(json!({
        "handle": {
            "id": "sandbox-01",
            "name": "legacy",
            "backend": "krun",
            "status": "starting",
            "published_endpoints": [],
        },
        "spec": {
            "tenant_id": "tenant",
            "name": "legacy",
            "backend": "krun",
            "filesystem": {
                "rootfs": "/srv/rootfs",
                "readonly": false,
            },
            "process": {
                "args": ["/bin/service"],
                "env": ["PATH=/usr/bin"],
                "cwd": "/",
                "terminal": false,
            },
            "resources": {
                "cpu_count": null,
                "memory_limit_bytes": null,
            },
            "port_bindings": [],
        },
        "image_metadata": {},
        "launch_artifact": null,
        "bundle_layout": {
            "bundle_dir": "/tmp/bundle",
            "config_path": "/tmp/bundle/config.json",
        },
        "conmon_layout": {
            "state_root": "/tmp/state",
            "container_state_dir": "/tmp/state/containers/sandbox-01",
            "exit_dir": "/tmp/state/exits",
            "persist_dir": "/tmp/state/persist/sandbox-01",
            "ctr_log": "/tmp/state/containers/sandbox-01/ctr.log",
            "oci_log": "/tmp/state/containers/sandbox-01/oci.log",
            "pidfile": "/tmp/state/containers/sandbox-01/pidfile",
            "conmon_pidfile": "/tmp/state/containers/sandbox-01/conmon.pid",
            "exit_status_file": "/tmp/state/exits/sandbox-01",
            "manifest_path": "/tmp/state/containers/sandbox-01/manifest.json",
        },
        "conmon_launch": {
            "create_command": {
                "program": "/usr/bin/conmon",
                "args": [],
            },
            "state_command": {
                "program": "/usr/libexec/neovex/crun",
                "args": ["state", "sandbox-01"],
            },
            "start_command": {
                "program": "/usr/libexec/neovex/crun",
                "args": ["start", "sandbox-01"],
            },
        },
        "last_exit_code": null,
        "launch_mode": "execute",
        "shutdown_requested": false,
        "status": "starting",
    }))
    .expect("legacy manifest should deserialize with new defaults");

    assert_eq!(manifest.restart_count, 0);
    assert_eq!(
        manifest.spec.lifecycle.restart_policy,
        SandboxRestartPolicy::Never
    );
    assert_eq!(manifest.spec.lifecycle.stop_timeout, None);
    assert!(
        manifest
            .conmon_launch
            .delete_command
            .program
            .as_os_str()
            .is_empty(),
        "legacy manifests should default the delete command instead of failing to deserialize"
    );
}

fn sample_spec() -> SandboxSpec {
    sample_spec_with_rootfs(Path::new("/srv/rootfs"))
}

fn sample_spec_with_rootfs(rootfs: &Path) -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        "postgres-primary",
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(rootfs),
        SandboxProcessSpec::new(["/usr/bin/postgres", "-D", "/var/lib/postgresql/data"])
            .with_env(["PATH=/usr/bin", "PGDATA=/var/lib/postgresql/data"]),
    )
    .with_port_bindings([
        SandboxPortBinding::tcp("postgres", 15432, 5432),
        SandboxPortBinding::tcp("health", 18080, 8080),
    ])
}

fn sparse_image_spec(name: &str) -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        name,
        SandboxBackendKind::Krun,
        SandboxFilesystemSpec::new(PathBuf::new()),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
}

fn sample_launch_defaults() -> OciImageLaunchDefaults {
    OciImageLaunchDefaults {
        filesystem: SandboxFilesystemSpec::new("/image/rootfs"),
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

fn sample_image_metadata() -> KrunImageMetadata {
    KrunImageMetadata::default()
}

fn sample_manifest(spec: SandboxSpec, launch_mode: KrunLaunchMode) -> KrunSandboxManifest {
    let endpoints = visible_published_endpoints(launch_mode, &spec, SandboxStatus::Starting);
    KrunSandboxManifest {
        handle: crate::instance::SandboxHandle::new(
            crate::instance::SandboxId::new("sandbox-01"),
            spec.name.clone(),
            SandboxBackendKind::Krun,
            SandboxStatus::Starting,
            endpoints,
        ),
        spec,
        image_metadata: KrunImageMetadata::default(),
        launch_artifact: None,
        bundle_layout: super::KrunBundleLayout::new("/tmp/bundle"),
        conmon_layout: super::OciConmonLayout::new(
            "/tmp/state",
            &crate::instance::SandboxId::new("sandbox-01"),
        ),
        conmon_launch: super::OciConmonLaunchPlan {
            create_command: super::CommandSpec::new("/bin/true"),
            state_command: super::CommandSpec::new("/bin/true"),
            start_command: super::CommandSpec::new("/bin/true"),
            delete_command: super::CommandSpec::new("/bin/true"),
        },
        last_exit_code: None,
        restart_count: 0,
        next_restart_at_millis: None,
        launch_mode,
        shutdown_requested: false,
        status: SandboxStatus::Starting,
    }
}

fn sample_registry_image_reference() -> String {
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

#[test]
fn desired_krun_vm_config_requires_memory_when_cpu_count_is_requested() {
    let error = desired_krun_vm_config(
        &sample_spec().with_resource_limits(SandboxResourceLimits::default().with_cpu_count(2)),
    )
    .expect_err("cpu-only krun resource requests should be rejected");

    assert!(
        error
            .to_string()
            .contains("cpu_count requires memory_limit_bytes"),
        "expected actionable validation error, got: {error}"
    );
}

trait ImageMetadataTestExt {
    fn with_stop_signal(self, stop_signal: &str) -> Self;
}

impl ImageMetadataTestExt for KrunImageMetadata {
    fn with_stop_signal(mut self, stop_signal: &str) -> Self {
        self.stop_signal = Some(stop_signal.to_owned());
        self
    }
}
