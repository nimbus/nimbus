use super::support::*;

use std::sync::Arc;

use crate::backends::oci::network::{
    OciMachinePortForwarderConfig, OciNetworkDirectEgress, OciSegmentAllocator,
    RecordingSegmentAllocator, SegmentAllocatorOperation,
};
use nimbus_egress::{EGRESS_CA_BUNDLE_ENV, EGRESS_NODE_EXTRA_CA_CERTS_ENV, EGRESS_PROXY_URL_ENV};
use tempfile::TempDir;

#[test]
fn plan_only_backend_persists_a_container_manifest() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());

    let handle = backend
        .start_sync(sample_spec().with_port_binding(SandboxPortBinding::tcp("db", 5432, 5432)))
        .expect("container plan should start");

    assert_eq!(handle.backend, SandboxBackendKind::Container);
    let manifest_path = crate::artifact_paths::manifest_path(
        &temp_dir.path().join("state"),
        &sample_spec().tenant_id,
        &handle.id,
    );
    assert!(manifest_path.is_file(), "manifest should be written");
}

#[test]
fn container_launch_network_config_denies_direct_egress_for_supervised_processes() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());

    let tenant = nimbus_core::TenantId::new("egress-tenant").expect("tenant should parse");
    let network_config = backend
        .network_config(&tenant)
        .expect("network config should resolve");

    assert_eq!(
        network_config.direct_egress,
        OciNetworkDirectEgress::Deny,
        "process-capable container launches must not keep ambient bridge egress"
    );
}

#[test]
fn container_backend_consumes_the_injected_portable_segment_allocator() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let tenant = nimbus_core::TenantId::new("injected-container").expect("tenant should parse");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        tenant.clone(),
        "10.73.0.0/24",
        73,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = ContainerSandboxBackend::with_segment_allocator(
        ContainerSandboxBackendConfig::under_root(temp_dir.path()),
        injected,
    );

    let network = backend
        .network_config(&tenant)
        .expect("injected allocator should resolve the network");

    assert_eq!(network.network_subnet, "10.73.0.0/24");
    assert_eq!(network.network_name, "nimbus-t-73");
    assert_eq!(network.network_interface, "nb-73");
    assert_eq!(
        recorder.operations(),
        [
            SegmentAllocatorOperation::Reconcile(Default::default()),
            SegmentAllocatorOperation::SegmentFor(tenant),
        ],
        "the container backend must use only the injected capability; startup reconciliation and resolution must not reconstruct or downcast a concrete allocator"
    );
}

#[test]
fn execute_plan_assigns_bridge_reachable_egress_proxy_and_injects_proxy_env() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.published_port_range = 15000..=15002;
    let backend = ContainerSandboxBackend::new(config);

    let plan = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("execute plan should lower");

    let egress_proxy = plan
        .manifest
        .egress_proxy
        .as_ref()
        .expect("execute launch should assign an egress proxy");
    // First tenant on the node super-net gets 10.0.0.0/24, gateway 10.0.0.1.
    assert_eq!(egress_proxy.host, "10.0.0.1");
    assert_eq!(egress_proxy.port, 15000);
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan.manifest.bundle_layout.config_path).unwrap())
            .expect("bundle config should parse");
    let env = config["process"]["env"]
        .as_array()
        .expect("env should be an array")
        .iter()
        .map(|value| value.as_str().expect("env entries should be strings"))
        .collect::<Vec<_>>();
    assert!(
        env.contains(&"HTTP_PROXY=http://10.0.0.1:15000")
            && env.contains(&"http_proxy=http://10.0.0.1:15000")
            && env.contains(&"NO_PROXY=")
            && env.contains(&"no_proxy="),
        "execute bundle should steer proxy-aware tools through the egress proxy: {env:?}"
    );
    assert!(
        env.contains(&format!("{EGRESS_PROXY_URL_ENV}=http://10.0.0.1:15000").as_str()),
        "execute bundle should expose Nimbus egress proxy metadata: {env:?}"
    );
    for expected in [
        format!("{EGRESS_CA_BUNDLE_ENV}=/run/nimbus/egress/ca.pem"),
        format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}=/run/nimbus/egress/ca.pem"),
    ] {
        assert!(
            env.contains(&expected.as_str()),
            "execute bundle should expose the workload-scoped trust anchor: {env:?}"
        );
    }
    let trust_anchor_path = temp_dir
        .path()
        .join("state")
        .join("egress-trust-anchors")
        .join("svc-demo")
        .join("db-01.pem");
    assert!(
        trust_anchor_path.is_file(),
        "execute planning must materialize the deterministic trust-anchor mount source"
    );
    assert!(
        std::fs::read_to_string(&trust_anchor_path)
            .expect("trust-anchor placeholder should read")
            .contains("placeholder"),
        "the planner writes a placeholder that the live PEP overwrites before launch"
    );
    let mounts = config["mounts"]
        .as_array()
        .expect("mounts should be an array");
    let trust_mount = mounts
        .iter()
        .find(|mount| mount["destination"] == "/run/nimbus/egress/ca.pem")
        .expect("execute bundle should mount the trust anchor");
    assert_eq!(trust_mount["type"], "bind");
    assert_eq!(
        trust_mount["source"].as_str(),
        Some(trust_anchor_path.to_string_lossy().as_ref())
    );
}

#[test]
fn execute_plan_allocates_egress_proxy_port_after_existing_guest_bindings() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.published_port_range = 15000..=15002;
    let backend = ContainerSandboxBackend::new(config);

    let plan = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 15000, 8080)),
            &sandbox_id(),
            None,
            None,
        )
        .expect("execute plan should lower");

    assert_eq!(
        plan.manifest.egress_proxy.as_ref().map(|proxy| proxy.port),
        Some(15001),
        "egress proxy must not collide with published service ports"
    );
}

#[test]
fn plan_only_launches_do_not_materialize_live_proxy_env() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());

    let plan = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("plan-only launch should lower");

    assert!(
        plan.manifest.egress_proxy.is_none(),
        "plan-only launches should not claim a live egress proxy"
    );
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan.manifest.bundle_layout.config_path).unwrap())
            .expect("bundle config should parse");
    let env = config["process"]["env"]
        .as_array()
        .expect("env should be an array")
        .iter()
        .map(|value| value.as_str().expect("env entries should be strings"))
        .collect::<Vec<_>>();
    assert!(
        env.iter().all(|entry| !entry.starts_with("HTTP_PROXY=")
            && !entry.starts_with("http_proxy=")
            && !entry.starts_with(&format!("{EGRESS_CA_BUNDLE_ENV}="))
            && !entry.starts_with(&format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}="))),
        "plan-only bundles should keep live proxy env absent: {env:?}"
    );
}

#[test]
fn plan_only_service_workload_prepares_runner_manifest_pointer_and_proxy_env() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.published_port_range = 15000..=15002;
    config.netavark_path = "/usr/libexec/podman/netavark".into();
    config.aardvark_dns_path = "/usr/libexec/podman/aardvark-dns".into();
    config.machine_port_forwarder = Some(OciMachinePortForwarderConfig::gvproxy_default());
    let backend = ContainerSandboxBackend::new(config);

    let prepared = backend
        .prepare_plan_only_service_workload(sample_spec())
        .expect("service workload should prepare");

    let manifest_path = crate::artifact_paths::manifest_path(
        &temp_dir.path().join("state"),
        &sample_spec().tenant_id,
        &prepared.handle.id,
    );
    let pointer_path = prepared.bundle_dir.join(RUNNER_MANIFEST_POINTER_FILE);
    assert_eq!(
        std::fs::read_to_string(&pointer_path)
            .expect("runner manifest pointer should be readable")
            .trim(),
        manifest_path.to_string_lossy(),
        "runner should receive an exact manifest pointer scoped to the prepared bundle"
    );

    let manifest_bytes = std::fs::read(&manifest_path).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).expect("manifest should parse");
    assert_eq!(manifest["start_mode"], "plan_only");
    assert_eq!(manifest["egress_proxy"]["host"], "10.0.0.1");
    assert_eq!(manifest["egress_proxy"]["port"], 15000);
    assert_eq!(
        manifest["runner_config"]["netavark_path"],
        "/usr/libexec/podman/netavark"
    );
    assert_eq!(
        manifest["runner_config"]["aardvark_dns_path"],
        "/usr/libexec/podman/aardvark-dns"
    );
    assert_eq!(
        manifest["runner_config"]["machine_port_forwarder"]["path_prefix"],
        "/services/forwarder"
    );
    let typed_manifest: ContainerSandboxManifest =
        serde_json::from_slice(&manifest_bytes).expect("typed manifest should parse");
    let runner_config = typed_manifest.runner_config.to_backend_config();
    assert_eq!(
        runner_config.netavark_path,
        PathBuf::from("/usr/libexec/podman/netavark")
    );
    assert_eq!(
        runner_config.aardvark_dns_path,
        PathBuf::from("/usr/libexec/podman/aardvark-dns")
    );
    assert_eq!(
        runner_config
            .machine_port_forwarder
            .expect("machine forwarder should survive runner reconstruction")
            .path_prefix,
        "/services/forwarder"
    );

    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(prepared.bundle_dir.join("config.json")).unwrap())
            .expect("bundle config should parse");
    let env = config["process"]["env"]
        .as_array()
        .expect("env should be an array")
        .iter()
        .map(|value| value.as_str().expect("env entries should be strings"))
        .collect::<Vec<_>>();
    assert!(
        env.contains(&"HTTP_PROXY=http://10.0.0.1:15000")
            && env.contains(&"http_proxy=http://10.0.0.1:15000")
            && env.contains(&format!("{EGRESS_PROXY_URL_ENV}=http://10.0.0.1:15000").as_str())
            && env.contains(&format!("{EGRESS_CA_BUNDLE_ENV}=/run/nimbus/egress/ca.pem").as_str())
            && env.contains(
                &format!("{EGRESS_NODE_EXTRA_CA_CERTS_ENV}=/run/nimbus/egress/ca.pem").as_str()
            ),
        "service bundle should route proxy-aware tools through the runner-owned egress proxy: {env:?}"
    );
    let trust_anchor_path = temp_dir
        .path()
        .join("state")
        .join("egress-trust-anchors")
        .join("svc-demo")
        .join(format!("{}.pem", prepared.handle.id.as_str()));
    assert!(
        trust_anchor_path.is_file(),
        "service workload planning must materialize the runner-owned trust-anchor mount source"
    );
}

#[test]
fn plan_only_backend_scopes_container_artifacts_by_tenant_for_same_service_name() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());
    let tenant_a = sample_spec_for_tenant("tenant-a", "api");
    let tenant_b = sample_spec_for_tenant("tenant-b", "api");

    let handle_a = backend
        .start_sync(tenant_a.clone())
        .expect("tenant-a container plan should start");
    let handle_b = backend
        .start_sync(tenant_b.clone())
        .expect("tenant-b container plan should start");

    let manifest_a = crate::artifact_paths::manifest_path(
        &temp_dir.path().join("state"),
        &tenant_a.tenant_id,
        &handle_a.id,
    );
    let manifest_b = crate::artifact_paths::manifest_path(
        &temp_dir.path().join("state"),
        &tenant_b.tenant_id,
        &handle_b.id,
    );
    let bundle_a = crate::artifact_paths::bundle_dir(
        &temp_dir.path().join("bundles"),
        &tenant_a.tenant_id,
        &handle_a.id,
    )
    .join("config.json");
    let bundle_b = crate::artifact_paths::bundle_dir(
        &temp_dir.path().join("bundles"),
        &tenant_b.tenant_id,
        &handle_b.id,
    )
    .join("config.json");

    assert_ne!(
        manifest_a, manifest_b,
        "tenant-scoped container manifests must not collide for the same service name"
    );
    assert_ne!(
        bundle_a, bundle_b,
        "tenant-scoped container bundles must not collide for the same service name"
    );
    assert!(manifest_a.is_file(), "tenant-a manifest should be written");
    assert!(manifest_b.is_file(), "tenant-b manifest should be written");
    assert!(bundle_a.is_file(), "tenant-a bundle should be written");
    assert!(bundle_b.is_file(), "tenant-b bundle should be written");

    futures::executor::block_on(crate::backend::SandboxBackend::remove_tenant_artifacts(
        &backend,
        tenant_a.tenant_id.clone(),
    ))
    .expect("tenant-a artifacts should be removed");

    assert!(
        !manifest_a.exists(),
        "tenant-a manifest should be removed by tenant cleanup"
    );
    assert!(
        !bundle_a.exists(),
        "tenant-a bundle should be removed by tenant cleanup"
    );
    assert!(manifest_b.exists(), "tenant-b manifest should remain");
    assert!(bundle_b.exists(), "tenant-b bundle should remain");
}

#[test]
fn plan_only_backend_lowers_tenant_volume_mounts_under_tenant_state_root() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());
    let tenant_a = sample_spec_for_tenant("tenant-a", "api")
        .with_mount(SandboxMountSpec::tenant_volume("shared", "/var/lib/app").read_only(false));
    let tenant_b = sample_spec_for_tenant("tenant-b", "api")
        .with_mount(SandboxMountSpec::tenant_volume("shared", "/var/lib/app").read_only(true));

    let handle_a = backend
        .start_sync(tenant_a.clone())
        .expect("tenant-a container plan should start");
    let handle_b = backend
        .start_sync(tenant_b.clone())
        .expect("tenant-b container plan should start");

    let volume_a = temp_dir
        .path()
        .join("state")
        .join("tenants")
        .join("tenant-a")
        .join("volumes")
        .join("shared");
    let volume_b = temp_dir
        .path()
        .join("state")
        .join("tenants")
        .join("tenant-b")
        .join("volumes")
        .join("shared");
    assert!(volume_a.is_dir(), "tenant-a volume directory should exist");
    assert!(volume_b.is_dir(), "tenant-b volume directory should exist");
    assert_ne!(
        volume_a, volume_b,
        "same named volume in different tenants must not share host storage"
    );

    let bundle_a = crate::artifact_paths::bundle_dir(
        &temp_dir.path().join("bundles"),
        &tenant_a.tenant_id,
        &handle_a.id,
    )
    .join("config.json");
    let bundle_b = crate::artifact_paths::bundle_dir(
        &temp_dir.path().join("bundles"),
        &tenant_b.tenant_id,
        &handle_b.id,
    )
    .join("config.json");
    let rendered_a =
        std::fs::read_to_string(&bundle_a).expect("tenant-a bundle should be readable");
    let rendered_b =
        std::fs::read_to_string(&bundle_b).expect("tenant-b bundle should be readable");
    assert!(
        rendered_a.contains(&volume_a.to_string_lossy().to_string()),
        "tenant-a bundle should bind only the tenant-a volume path: {rendered_a}"
    );
    assert!(
        rendered_b.contains(&volume_b.to_string_lossy().to_string()),
        "tenant-b bundle should bind only the tenant-b volume path: {rendered_b}"
    );

    futures::executor::block_on(crate::backend::SandboxBackend::remove_tenant_artifacts(
        &backend,
        tenant_a.tenant_id.clone(),
    ))
    .expect("tenant-a artifacts should be removed");

    assert!(
        !volume_a.exists(),
        "tenant-a volume should be removed by tenant cleanup"
    );
    assert!(
        volume_b.exists(),
        "tenant-b volume should remain after tenant-a cleanup"
    );
}

#[test]
fn plan_only_backend_scopes_network_state_by_tenant_for_same_sandbox_id() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());
    let sandbox_id = SandboxId::new("api-01");

    let tenant_a_plan = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("tenant-a", "api"),
            &sandbox_id,
            None,
            None,
        )
        .expect("tenant-a plan should lower");
    let tenant_b_plan = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("tenant-b", "api"),
            &sandbox_id,
            None,
            None,
        )
        .expect("tenant-b plan should lower");

    assert_ne!(
        tenant_a_plan.manifest.network_layout.netns_path,
        tenant_b_plan.manifest.network_layout.netns_path,
        "same sandbox id in different tenants must not share network namespaces"
    );
    assert_eq!(
        tenant_a_plan.manifest.network_layout.state_root,
        temp_dir.path().join("state")
    );
    assert_eq!(
        tenant_b_plan.manifest.network_layout.state_root,
        temp_dir.path().join("state"),
        "all network resources on one node share one authority root"
    );
    assert_ne!(
        tenant_a_plan.manifest.network_layout.tenant_id,
        tenant_b_plan.manifest.network_layout.tenant_id,
        "tenant IPAM payloads remain distinct typed partitions"
    );
}

#[test]
fn plan_only_backend_auto_assigns_exposed_ports_from_published_range() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    config.published_port_range = 15000..=15001;
    let backend = ContainerSandboxBackend::new(config);

    let plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &sandbox_id(),
            Some(&exposed_port_launch_defaults(PathBuf::from("/tmp/rootfs"))),
            None,
        )
        .expect("plan should lower image-exposed ports");

    assert_eq!(plan.manifest.spec.port_bindings.len(), 1);
    let binding = &plan.manifest.spec.port_bindings[0];
    assert_eq!(binding.name, "tcp-8080");
    assert_eq!(binding.host_port, 15000);
    assert_eq!(binding.guest_port, 8080);
}

#[test]
fn plan_only_backend_rejects_same_tenant_port_quota_for_image_exposed_ports() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    config.published_port_range = 15000..=15005;
    config.max_published_ports_per_tenant = Some(1);
    let backend = ContainerSandboxBackend::new(config);

    backend
        .start_sync(sample_spec().with_port_binding(SandboxPortBinding::tcp("db", 15432, 5432)))
        .expect("first same-tenant service should consume the single tenant port");

    let error = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("api-01"),
            Some(&exposed_port_launch_defaults(PathBuf::from("/tmp/rootfs"))),
            None,
        )
        .expect_err("image-exposed port should exceed the tenant port quota");

    assert!(
        error.to_string().contains("published port quota exceeded")
            && error.to_string().contains("svc-demo")
            && error.to_string().contains("limit 1"),
        "expected tenant port quota error, got: {error}"
    );
}

#[test]
fn plan_only_backend_rejects_same_tenant_resource_quota_exhaustion() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    // Disk is left unlimited: sandbox specs can no longer carry an (unenforceable)
    // disk_limit_bytes, so each sandbox is charged the default per-sandbox disk
    // accounting. This test isolates the vCPU constraint, so disk must not bind first.
    config.resource_quota_policy = SandboxResourceQuotaPolicy::default()
        .with_max_active_sandboxes_per_tenant(Some(2))
        .with_max_vcpus_per_tenant(Some(2))
        .with_max_memory_bytes_per_tenant(Some(1024))
        .with_max_disk_bytes_per_tenant(None)
        .with_max_log_bytes_per_tenant(Some(512));
    let backend = ContainerSandboxBackend::new(config);

    backend
        .start_sync(
            sample_spec_for_tenant("tenant-a", "db").with_resource_limits(
                SandboxResourceLimits::default()
                    .with_cpu_count(1)
                    .with_memory_limit_bytes(512)
                    .with_log_limit_bytes(256),
            ),
        )
        .expect("first sandbox should fit within tenant quota");

    let error = backend
        .start_sync(
            sample_spec_for_tenant("tenant-a", "api").with_resource_limits(
                SandboxResourceLimits::default()
                    .with_cpu_count(2)
                    .with_memory_limit_bytes(512)
                    .with_log_limit_bytes(256),
            ),
        )
        .expect_err("second sandbox should exceed same-tenant vCPU quota");

    assert!(
        error.to_string().contains("sandbox vCPU quota exceeded")
            && error.to_string().contains("tenant-a"),
        "expected tenant vCPU quota error, got: {error}"
    );

    backend
        .start_sync(
            sample_spec_for_tenant("tenant-b", "api").with_resource_limits(
                SandboxResourceLimits::default()
                    .with_cpu_count(2)
                    .with_memory_limit_bytes(512)
                    .with_log_limit_bytes(256),
            ),
        )
        .expect("other tenant should have an independent quota bucket");
}

#[test]
fn image_backed_plan_uses_direct_conmon_launch_for_materialized_rootfs() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let rootfs_path = temp_dir.path().join("materialized-rootfs");

    let plan = backend
        .plan_start_with_id(
            &sample_spec(),
            &sandbox_id(),
            Some(&sample_launch_defaults(rootfs_path.clone())),
            Some(sample_rootfs_artifact(rootfs_path)),
        )
        .expect("image-backed plan should lower");

    assert_eq!(
        plan.manifest.conmon_launch.create_command.program,
        PathBuf::from("conmon")
    );
    assert_eq!(
        plan.manifest.conmon_launch.start_command.program,
        PathBuf::from("crun")
    );
    assert!(
        plan.manifest
            .conmon_launch
            .create_command
            .args
            .first()
            .map(String::as_str)
            != Some("unshare"),
        "materialized rootfs launches should not be wrapped in buildah unshare"
    );
}

#[test]
fn image_backed_plan_merges_image_env_with_process_overrides() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());
    let rootfs_path = temp_dir.path().join("materialized-rootfs");
    let mut defaults = sample_launch_defaults(rootfs_path.clone());
    defaults.process = SandboxProcessSpec::new(["/bin/app"]).with_env([
        "PATH=/image/bin",
        "IMAGE_ONLY=yes",
        "OVERRIDE_ME=image",
    ]);
    let mut spec = sample_spec();
    spec.process = SandboxProcessSpec::new(Vec::<String>::new())
        .with_env(["OVERRIDE_ME=compose", "APP_ENV=dev"]);

    let plan = backend
        .plan_start_with_id(
            &spec,
            &sandbox_id(),
            Some(&defaults),
            Some(sample_rootfs_artifact(rootfs_path)),
        )
        .expect("image-backed plan should lower with merged process env");

    assert_eq!(
        plan.manifest.spec.process.env,
        vec![
            "PATH=/image/bin".to_owned(),
            "IMAGE_ONLY=yes".to_owned(),
            "OVERRIDE_ME=compose".to_owned(),
            "APP_ENV=dev".to_owned(),
        ],
        "container image env should survive while process env keeps override precedence"
    );

    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan.manifest.bundle_layout.config_path).unwrap())
            .expect("bundle config should parse");
    let env = config["process"]["env"]
        .as_array()
        .expect("env should be an array")
        .iter()
        .map(|value| value.as_str().expect("env entries should be strings"))
        .collect::<Vec<_>>();
    for expected in [
        "PATH=/image/bin",
        "IMAGE_ONLY=yes",
        "OVERRIDE_ME=compose",
        "APP_ENV=dev",
    ] {
        assert!(
            env.contains(&expected),
            "bundle env should include merged entry {expected:?}: {env:?}"
        );
    }
    assert!(
        !env.contains(&"OVERRIDE_ME=image"),
        "bundle env should replace image env with process override: {env:?}"
    );
}

#[test]
fn rootfs_plan_resolves_entrypoint_command_and_user_without_image_defaults() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());
    let mut spec = sample_spec();
    spec.process = SandboxProcessSpec::new(Vec::<String>::new())
        .with_entrypoint(["/bin/sh", "-lc"])
        .with_command(["exec app"])
        .with_user("1001:1002");

    let plan = backend
        .plan_start_with_id(&spec, &sandbox_id(), None, None)
        .expect("rootfs plan should lower entrypoint/command without image defaults");

    assert_eq!(
        plan.manifest.spec.process.args,
        vec!["/bin/sh", "-lc", "exec app"],
        "rootfs entrypoint and command must become runtime process args"
    );

    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&plan.manifest.bundle_layout.config_path).unwrap())
            .expect("bundle config should parse");
    assert_eq!(
        config["process"]["args"],
        serde_json::json!(["/bin/sh", "-lc", "exec app"])
    );
    assert_eq!(config["process"]["user"]["uid"], serde_json::json!(1001));
    assert_eq!(config["process"]["user"]["gid"], serde_json::json!(1002));
}
