mod attachment_authority;
mod creator_recovery;
mod egress_readiness;
mod endpoint_projection;
mod explicit_stop;
mod generation_fencing;
mod launch_compensation;
mod legacy_stop_config;
mod lifecycle_locking;
mod manifest_durability;
mod manifest_schema;
mod natural_exit;
mod network_composition;
mod provider_failure_recovery;
mod provision_phases;
mod restart_phases;
mod root_ownership;
mod startup_fencing;
mod support;
use support::*;

use crate::backends::oci::network::AttachmentAttachAuthority;
use nimbus_network::LocalPortLeaseAuthority;

pub(super) fn env_from_config(config: &serde_json::Value) -> Vec<&str> {
    config["process"]["env"]
        .as_array()
        .expect("env should be an array")
        .iter()
        .map(|value| value.as_str().expect("env entries should be strings"))
        .collect()
}

fn sample_execution_attempt_id(id: &SandboxId) -> crate::SandboxExecutionAttemptId {
    crate::SandboxExecutionAttemptId::new(format!("test-execution-attempt:{id}"))
        .expect("test execution attempt should validate")
}

// KME4 readiness gate: the execute path no longer fails closed unconditionally;
// it is gated immediately before the VMM launches and permits the launch IFF the
// deny-by-default netns is installed AND the per-sandbox egress PEP is running
// with an active policy generation AND the host platform supports enforcement.
// The two tests below are the converted descendants of the original
// always-fail-closed pair: one proves a NOT-READY PEP still denies, the other
// proves a fully-ready setup permits. Each gate arm gets its own negative test.

/// Converted from the original always-fail-closed execute test: a running but
/// NOT-READY PEP (no active policy generation) must still deny the launch.
#[test]
fn execute_launch_denies_when_egress_pep_is_not_ready() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let id = SandboxId::new("kme4-not-ready");
    let tenant = TenantId::new("tenant-kme4-not-ready").expect("tenant id should be valid");
    let netns_path = temp_dir.path().join("netns-installed");
    fs::write(&netns_path, b"netns").expect("netns marker should write");

    // A PEP is running, but it was started without a policy, so it reports
    // not-ready (no active policy generation).
    let policyless = WorkloadPep::start(WorkloadPepConfig::without_active_policy())
        .expect("a policy-less PEP should still bind and start");
    backend
        .egress_proxies
        .insert_running_for_test(&tenant, &id, policyless)
        .expect("test PEP should register");
    let readiness = backend
        .egress_proxies
        .readiness(&tenant, &id)
        .expect("readiness should resolve")
        .expect("a PEP is registered");
    assert!(
        !readiness.is_ready() && readiness.policy_generation().is_none(),
        "precondition: the registered PEP must be not-ready, got: {readiness:?}"
    );

    let error = backend
        .ensure_execute_egress_preconditions(&tenant, &id, &netns_path)
        .expect_err("a not-ready PEP must deny the launch fail-closed");
    assert!(
        error.to_string().contains("not ready")
            && error.to_string().contains("active policy generation"),
        "expected not-ready deny, got: {error}"
    );
}

/// Converted from the original always-fail-closed image-launch test: with the
/// netns installed AND a ready PEP (active policy generation), the gate permits
/// the launch. Runs on the Mac without `/dev/kvm` because it exercises the gate
/// decision, not a real VMM.
#[test]
fn execute_launch_permits_when_netns_installed_and_pep_ready() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let id = SandboxId::new("kme4-ready");
    let netns_path = temp_dir.path().join("netns-installed");
    fs::write(&netns_path, b"netns").expect("netns marker should write");
    let tenant = TenantId::new("tenant-kme4-ready").expect("tenant id should be valid");

    backend
        .egress_proxies
        .ensure_running(&tenant, &id, &EgressPolicy::deny_all(), loopback_addr())
        .expect("a ready PEP should start with a compiled policy");
    let readiness = backend
        .egress_proxies
        .readiness(&tenant, &id)
        .expect("readiness should resolve")
        .expect("a PEP is registered");
    assert!(
        readiness.is_ready() && readiness.policy_generation().is_some(),
        "precondition: the registered PEP must be ready, got: {readiness:?}"
    );

    backend
        .ensure_execute_egress_preconditions(&tenant, &id, &netns_path)
        .expect("all preconditions satisfied must permit the launch");
}

/// NNC0.6 regression for NNCF6. This captures the exact unsafe boundary after
/// the persistent namespace path exists but before Netavark has emitted status
/// and before an attachment phase can prove the egress pin. It consumes the
/// platform-independent half of the production complete-readiness gate so the
/// same proof runs on Linux and non-Linux hosts.
#[test]
fn nnc0_6_krun_rejects_netns_path_without_complete_attachment_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let manifest = sample_manifest(
        sample_spec_for_tenant("tenant-nnc0-6", "partial-attachment"),
        KrunStartMode::Execute,
    );
    fs::create_dir_all(
        manifest
            .network_layout
            .netns_path
            .parent()
            .expect("netns path should have a parent"),
    )
    .expect("netns parent should create");
    fs::write(&manifest.network_layout.netns_path, b"netns")
        .expect("netns-created boundary should persist");
    assert!(
        !manifest.network_layout.status_path.exists(),
        "precondition: Netavark status must still be absent at this partial boundary"
    );
    backend
        .egress_proxies
        .ensure_running(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &EgressPolicy::deny_all(),
            loopback_addr(),
        )
        .expect("ready PEP isolates the missing attachment-evidence condition");

    let readiness = backend.ensure_complete_host_managed_attachment_readiness_for_test(&manifest);

    assert!(
        readiness.is_err(),
        "NNCF6: a netns path plus ready PEP cannot prove Netavark setup or egress pin; \
         partial same-generation attachment must deny launch"
    );
}

/// NETNS-ABSENT arm: even with a ready PEP, a missing deny-by-default netns must
/// deny the launch fail-closed.
#[test]
fn execute_launch_denies_when_network_namespace_is_not_installed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let id = SandboxId::new("kme4-no-netns");
    let missing_netns = temp_dir.path().join("netns").join("never-created");
    let tenant = TenantId::new("tenant-kme4-no-netns").expect("tenant id should be valid");
    backend
        .egress_proxies
        .ensure_running(&tenant, &id, &EgressPolicy::deny_all(), loopback_addr())
        .expect("a ready PEP should start");

    let error = backend
        .ensure_execute_egress_preconditions(&tenant, &id, &missing_netns)
        .expect_err("a missing netns must deny the launch fail-closed");
    assert!(
        error.to_string().contains("network namespace")
            && error.to_string().contains("not installed"),
        "expected netns-not-installed deny, got: {error}"
    );
}

/// PEP-ABSENT arm: the netns is installed but no PEP is registered for the
/// sandbox, so the gate must deny fail-closed.
#[test]
fn execute_launch_denies_when_egress_pep_is_absent() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let id = SandboxId::new("kme4-no-pep");
    let tenant = TenantId::new("tenant-kme4-no-pep").expect("tenant id should be valid");
    let netns_path = temp_dir.path().join("netns-installed");
    fs::write(&netns_path, b"netns").expect("netns marker should write");

    let error = backend
        .ensure_execute_egress_preconditions(&tenant, &id, &netns_path)
        .expect_err("an absent PEP must deny the launch fail-closed");
    assert!(
        error
            .to_string()
            .contains("no egress policy-enforcement proxy is running"),
        "expected no-PEP deny, got: {error}"
    );
}

/// PLATFORM arm: krun execute is Linux-KVM only. On a non-Linux host the full
/// execute path denies before any VMM launch and registers no egress proxy, so
/// no enforcement state leaks. (Only meaningful off-Linux; vacuous on Linux CI.)
#[cfg(not(target_os = "linux"))]
#[test]
fn execute_start_denies_fail_closed_off_linux() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));

    let spec = sample_spec();
    let activation_id = SandboxId::new("kme4-off-linux-activation");
    let network_plan = sample_provision_network_plan(&spec, &activation_id, "off-linux-activation");
    backend
        .reserve_provision_network(
            spec,
            activation_id.clone(),
            sample_execution_attempt_id(&activation_id),
            network_plan,
        )
        .expect("non-Linux reservation must not launch the VMM");
    backend
        .prepare_provision_workload(&activation_id, &sample_execution_attempt_id(&activation_id))
        .expect("non-Linux preparation must not launch the VMM");
    let error = backend
        .activate_provision_workload(&activation_id, &sample_execution_attempt_id(&activation_id))
        .expect_err("krun activation must fail closed on a non-Linux host");
    assert!(
        matches!(error, crate::error::SandboxError::BackendUnavailable { .. })
            && error.to_string().contains("requires a Linux host"),
        "expected non-Linux platform deny, got: {error}"
    );

    // The full gate (`ensure_execute_egress_enforced`) also denies off-Linux even
    // when the netns + a ready PEP are present, proving the platform check is part
    // of the gate itself, not only the earlier `execute_start` guard.
    let id = SandboxId::new("kme4-off-linux");
    let netns_path = temp_dir.path().join("netns-installed");
    fs::write(&netns_path, b"netns").expect("netns marker should write");
    let tenant = TenantId::new("tenant-kme4-off-linux").expect("tenant id should be valid");
    backend
        .egress_proxies
        .ensure_running(&tenant, &id, &EgressPolicy::deny_all(), loopback_addr())
        .expect("a ready PEP should start");
    let mut manifest = sample_manifest(sample_spec(), KrunStartMode::Execute);
    manifest.handle.id = id;
    manifest.network_layout.netns_path = netns_path;
    let gate_error = backend
        .ensure_execute_egress_enforced(&manifest)
        .expect_err("the full gate must deny on a non-Linux host");
    assert!(
        matches!(
            gate_error,
            crate::error::SandboxError::BackendUnavailable { .. }
        ),
        "expected platform deny from the full gate, got: {gate_error}"
    );
}

#[test]
fn plan_only_backend_lowers_before_generic_lifecycle_inspection() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let spec = sample_spec();

    let handle = materialize_plan_only_fixture(&backend, spec)
        .expect("plan-only lowering should materialize its fixture");
    assert_eq!(handle.backend, SandboxBackendKind::Krun);
    assert_eq!(handle.status, crate::instance::SandboxStatus::Starting);
    assert_eq!(handle.published_endpoints.len(), 2);

    let inspected = block_on(backend.inspect(&handle.id))
        .expect("inspect should succeed")
        .expect("plan-only sandbox should persist a manifest");
    assert_eq!(inspected.handle.id, handle.id);
}

#[test]
fn plan_only_backend_lowers_image_launch_before_lifecycle_inspection() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let image_reference = sample_registry_image_reference();
    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.use_buildah_unshare = false;
    let backend = KrunSandboxBackend::new(config);

    let mut spec = sparse_image_spec("image-trait");
    spec.root = SandboxRootSpec::oci_image_reference(image_reference);
    let handle = materialize_plan_only_fixture(&backend, spec.clone())
        .expect("plan-only image-backed lowering should materialize");

    assert_eq!(handle.backend, SandboxBackendKind::Krun);
    assert_eq!(handle.status, crate::instance::SandboxStatus::Starting);

    let inspected = block_on(backend.inspect(&handle.id))
        .expect("inspect should succeed")
        .expect("plan-only image-backed sandbox should persist a manifest");
    assert_eq!(inspected.handle.id, handle.id);
}

#[test]
fn plan_only_backend_lowers_build_launch_before_lifecycle_inspection() {
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
    let backend = KrunSandboxBackend::new(config);

    let spec = sparse_build_spec("build-trait", "nimbus-api", &dockerfile_path, &workspace);
    let handle = materialize_plan_only_fixture(&backend, spec.clone())
        .expect("plan-only build-backed lowering should materialize");

    assert_eq!(handle.backend, SandboxBackendKind::Krun);
    assert_eq!(handle.status, crate::instance::SandboxStatus::Starting);

    let inspected = block_on(backend.inspect(&handle.id))
        .expect("inspect should succeed")
        .expect("plan-only build-backed sandbox should persist a manifest");
    assert_eq!(inspected.handle.id, handle.id);
    let manifest_path = manifest_path(temp_dir.path(), &spec, &handle.id);
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

    let handle = materialize_plan_only_fixture(&backend, spec.clone())
        .expect("plan-only lowering should materialize");
    let manifest_path = manifest_path(temp_dir.path(), &spec, &handle.id);
    let bundle_path = bundle_config_path(temp_dir.path(), &spec, &handle.id);

    assert!(manifest_path.exists(), "sandbox manifest should be written");
    assert!(bundle_path.exists(), "bundle config should be written");

    let rendered_bundle =
        fs::read_to_string(bundle_path).expect("bundle config should be readable");
    assert!(
        rendered_bundle
            .contains("\"krun.port_map\": \"127.0.0.1:15432:5432,127.0.0.1:18080:8080\""),
        "bundle config should preserve the address:host:guest TSI mapping"
    );
}

#[test]
fn plan_start_scopes_artifacts_by_tenant_for_same_service_name() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let tenant_a = sample_spec_for_tenant("tenant-a", "api");
    let tenant_b = sample_spec_for_tenant("tenant-b", "api");

    let handle_a = materialize_plan_only_fixture(&backend, tenant_a.clone())
        .expect("tenant-a plan should persist");
    let handle_b = materialize_plan_only_fixture(&backend, tenant_b.clone())
        .expect("tenant-b plan should persist");

    let manifest_a = manifest_path(temp_dir.path(), &tenant_a, &handle_a.id);
    let manifest_b = manifest_path(temp_dir.path(), &tenant_b, &handle_b.id);
    let bundle_a = bundle_config_path(temp_dir.path(), &tenant_a, &handle_a.id);
    let bundle_b = bundle_config_path(temp_dir.path(), &tenant_b, &handle_b.id);

    assert_ne!(
        manifest_a, manifest_b,
        "tenant-scoped manifests must not collide for the same service name"
    );
    assert_ne!(
        bundle_a, bundle_b,
        "tenant-scoped bundles must not collide for the same service name"
    );
    assert!(manifest_a.is_file(), "tenant-a manifest should be written");
    assert!(manifest_b.is_file(), "tenant-b manifest should be written");
    assert!(bundle_a.is_file(), "tenant-a bundle should be written");
    assert!(bundle_b.is_file(), "tenant-b bundle should be written");

    let tenant_a_manifest =
        fs::read_to_string(&manifest_a).expect("tenant-a manifest should be readable");
    let tenant_b_manifest =
        fs::read_to_string(&manifest_b).expect("tenant-b manifest should be readable");
    assert!(tenant_a_manifest.contains("\"tenant_id\": \"tenant-a\""));
    assert!(tenant_b_manifest.contains("\"tenant_id\": \"tenant-b\""));
}

#[test]
fn plan_start_lowers_tenant_volume_mounts_under_tenant_state_root() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let tenant_a = sample_spec_for_tenant("tenant-a", "api")
        .with_mount(SandboxMountSpec::tenant_volume("shared", "/var/lib/app").read_only(false));
    let tenant_b = sample_spec_for_tenant("tenant-b", "api")
        .with_mount(SandboxMountSpec::tenant_volume("shared", "/var/lib/app").read_only(true));

    let handle_a = materialize_plan_only_fixture(&backend, tenant_a.clone())
        .expect("tenant-a plan should persist");
    let handle_b = materialize_plan_only_fixture(&backend, tenant_b.clone())
        .expect("tenant-b plan should persist");

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

    let bundle_a = fs::read_to_string(bundle_config_path(temp_dir.path(), &tenant_a, &handle_a.id))
        .expect("tenant-a bundle should be readable");
    let bundle_b = fs::read_to_string(bundle_config_path(temp_dir.path(), &tenant_b, &handle_b.id))
        .expect("tenant-b bundle should be readable");
    assert!(
        bundle_a.contains(&volume_a.to_string_lossy().to_string()),
        "tenant-a bundle should bind only the tenant-a volume path: {bundle_a}"
    );
    assert!(
        bundle_b.contains(&volume_b.to_string_lossy().to_string()),
        "tenant-b bundle should bind only the tenant-b volume path: {bundle_b}"
    );
    assert!(
        bundle_a.contains("\"rw\""),
        "tenant-a writable mount should render rw options: {bundle_a}"
    );
    assert!(
        bundle_b.contains("\"ro\""),
        "tenant-b read-only mount should render ro options: {bundle_b}"
    );

    block_on(SandboxBackend::remove_tenant_artifacts(
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
fn remove_tenant_artifacts_deletes_only_matching_krun_tenant_roots() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let tenant_a = sample_spec_for_tenant("tenant-a", "api");
    let tenant_b = sample_spec_for_tenant("tenant-b", "api");
    let handle_a = materialize_plan_only_fixture(&backend, tenant_a.clone())
        .expect("tenant-a plan should persist");
    let handle_b = materialize_plan_only_fixture(&backend, tenant_b.clone())
        .expect("tenant-b plan should persist");
    let shared_cache = temp_dir
        .path()
        .join("state")
        .join("image-cache")
        .join("oci");
    fs::create_dir_all(&shared_cache).expect("shared image cache should be creatable");
    fs::write(shared_cache.join("digest"), "verified blob").expect("cache marker should write");

    block_on(backend.remove_tenant_artifacts(tenant_a.tenant_id.clone()))
        .expect("tenant-a artifacts should be removed");

    assert!(
        !manifest_path(temp_dir.path(), &tenant_a, &handle_a.id).exists(),
        "tenant-a manifest should be removed"
    );
    assert!(
        !bundle_config_path(temp_dir.path(), &tenant_a, &handle_a.id).exists(),
        "tenant-a bundle should be removed"
    );
    assert!(
        manifest_path(temp_dir.path(), &tenant_b, &handle_b.id).exists(),
        "tenant-b manifest should remain"
    );
    assert!(
        bundle_config_path(temp_dir.path(), &tenant_b, &handle_b.id).exists(),
        "tenant-b bundle should remain"
    );
    assert!(
        shared_cache.join("digest").exists(),
        "shared content-addressed image cache should not be removed with one tenant"
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

    let handle = materialize_plan_only_fixture(&backend, spec.clone())
        .expect("plan-only lowering should materialize");
    let vm_config_path = krun_vm_config_path(&rootfs);
    let vm_config =
        fs::read_to_string(&vm_config_path).expect("krun vm config should be materialized");
    let bundle = fs::read_to_string(bundle_config_path(temp_dir.path(), &spec, &handle.id))
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

    materialize_plan_only_fixture(&backend, spec).expect("plan-only lowering should materialize");

    assert!(
        !stale_vm_config.exists(),
        "memory-only starts should remove stale krun vm config so crun uses the OCI memory limit path"
    );
}

#[test]
fn rootfs_plan_resolves_entrypoint_command_and_user_without_image_defaults() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let rootfs = temp_dir.path().join("rootfs");
    fs::create_dir_all(&rootfs).expect("rootfs directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let mut spec = sample_spec_with_rootfs(&rootfs);
    spec.process = SandboxProcessSpec::new(Vec::<String>::new())
        .with_entrypoint(["/bin/sh", "-lc"])
        .with_command(["exec app"])
        .with_user("1001:1002");

    let handle = materialize_plan_only_fixture(&backend, spec.clone())
        .expect("rootfs krun plan should lower entrypoint/command without image defaults");
    let manifest_path = manifest_path(temp_dir.path(), &spec, &handle.id);
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(&manifest_path).expect("krun manifest should be readable"),
    )
    .expect("krun manifest should parse");

    assert_eq!(
        manifest["spec"]["process"]["args"],
        json!([GUEST_USER_HELPER_GUEST_PATH, "/bin/sh", "-lc", "exec app"]),
        "rootfs entrypoint and command must become runtime process args before guest user wrapping"
    );
    assert_eq!(
        manifest["image_metadata"]["user"],
        json!("1001:1002"),
        "rootfs process user should flow into krun guest-user handling"
    );
    assert_eq!(
        manifest["spec"]["process"]["env"]
            .as_array()
            .expect("env should be an array")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter(|entry| entry.starts_with("NIMBUS_GUEST_"))
            .collect::<Vec<_>>(),
        vec!["NIMBUS_GUEST_UID=1001", "NIMBUS_GUEST_GID=1002"]
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
        SandboxOwnerSpec::service("api"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new(PathBuf::new())),
        SandboxProcessSpec::new(Vec::<String>::new()),
    );

    let launch_plan = backend
        .plan_start_with_launch_defaults(&spec, Some(&sample_launch_defaults()))
        .expect("launch defaults should materialize the sparse spec");

    assert_eq!(
        launch_plan
            .manifest
            .spec
            .rootfs()
            .expect("launch defaults should resolve a rootfs")
            .rootfs,
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
        rendered_bundle.contains("\"destination\": \"/.nimbus\""),
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
        SandboxOwnerSpec::service("api"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/operator/rootfs").read_only(true)),
        SandboxProcessSpec::new(["/bin/sh", "-lc", "exec custom-api"])
            .with_env(["PATH=/custom/bin", "APP_MODE=dev"])
            .with_cwd("/workspace"),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080));

    let launch_plan = backend
        .plan_start_with_launch_defaults(&spec, Some(&sample_launch_defaults()))
        .expect("explicit operator overrides should coexist with image defaults");

    assert_eq!(
        launch_plan
            .manifest
            .spec
            .rootfs()
            .expect("operator override should preserve a rootfs")
            .rootfs,
        PathBuf::from("/operator/rootfs")
    );
    assert!(
        launch_plan
            .manifest
            .spec
            .rootfs()
            .expect("operator override should preserve rootfs options")
            .readonly
    );
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
    assert_eq!(
        launch_plan.manifest.spec.port_bindings,
        vec![
            SandboxPortBinding::tcp("http", 18080, 8080),
            SandboxPortBinding::tcp("tcp-8443", 15000, 8443),
        ],
        "the explicit mapping wins for 8080 while an inert preview is rendered for the remaining image port"
    );
    assert!(
        launch_plan.manifest.port_leases.is_empty(),
        "plan-only rendering must not claim durable host-port authority"
    );
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
fn oci_image_root_plan_only_persists_and_then_cleans_up_materialized_rootfs() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let image_reference = sample_registry_image_reference();

    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.use_buildah_unshare = false;

    let backend = KrunSandboxBackend::new(config);
    let spec = sparse_image_spec("image-backed-api")
        .with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080));

    let mut spec = spec;
    spec.root = SandboxRootSpec::oci_image_reference(image_reference);

    let handle = materialize_plan_only_fixture(&backend, spec.clone())
        .expect("plan-only image-backed lowering should materialize");

    let manifest_path = manifest_path(temp_dir.path(), &spec, &handle.id);
    let manifest_before_stop =
        fs::read_to_string(&manifest_path).expect("manifest should be readable before stop");
    assert!(
        manifest_before_stop.contains("\"launch_artifact\""),
        "manifest should retain launch-artifact metadata while running"
    );
    let rootfs_path = rootfs_artifact_path(temp_dir.path(), &spec, &handle.id);
    assert!(
        rootfs_path.exists(),
        "image-backed plan should materialize a rootfs under the krun state root"
    );
}

#[test]
fn oci_image_root_plan_only_skips_krun_vm_config_prelude_for_materialized_rootfs() {
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

    let mut spec = spec;
    spec.root = SandboxRootSpec::oci_image_reference(image_reference);

    let launch_plan = backend
        .plan_start(&spec)
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
fn oci_image_root_plan_only_previews_ports_without_reserving_them() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let image_reference = sample_registry_image_reference();

    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.use_buildah_unshare = false;
    config.published_port_range = 15000..=15001;

    let backend = KrunSandboxBackend::new(config);

    let mut first_spec = sparse_image_spec("first");
    first_spec.root = SandboxRootSpec::oci_image_reference(image_reference.clone());
    let first = materialize_plan_only_fixture(&backend, first_spec)
        .expect("first plan-only image-backed lowering should succeed");
    let first_inspected = block_on(backend.inspect(&first.id))
        .expect("inspect should succeed")
        .expect("first sandbox should be persisted");
    assert_eq!(first_inspected.handle.published_endpoints.len(), 1);
    assert_eq!(
        first_inspected.handle.published_endpoints[0].address.port(),
        15000
    );

    let mut second_spec = sparse_image_spec("second");
    second_spec.root = SandboxRootSpec::oci_image_reference(image_reference.clone());
    let second = materialize_plan_only_fixture(&backend, second_spec)
        .expect("second plan-only image-backed lowering should succeed");
    let second_inspected = block_on(backend.inspect(&second.id))
        .expect("inspect should succeed")
        .expect("second sandbox should be persisted");
    assert_eq!(second_inspected.handle.published_endpoints.len(), 1);
    assert_eq!(
        second_inspected.handle.published_endpoints[0]
            .address
            .port(),
        15000,
        "inert plan-only previews must not treat another manifest as allocation authority"
    );

    let mut third_spec = sparse_image_spec("third");
    third_spec.root = SandboxRootSpec::oci_image_reference(image_reference);
    let third = materialize_plan_only_fixture(&backend, third_spec.clone())
        .expect("third plan-only image-backed lowering should succeed");
    let third_inspected = block_on(backend.inspect(&third.id))
        .expect("inspect should succeed")
        .expect("third sandbox should be persisted");
    assert_eq!(third_inspected.handle.published_endpoints.len(), 1);
    assert_eq!(
        third_inspected.handle.published_endpoints[0].address.port(),
        15000
    );

    let third_bundle =
        fs::read_to_string(bundle_config_path(temp_dir.path(), &third_spec, &third.id))
            .expect("third bundle config should be readable");
    assert!(
        third_bundle.contains("\"krun.port_map\": \"127.0.0.1:15000:8080\""),
        "auto-assigned bindings should rewrite the krun port map annotation"
    );
}

#[test]
fn plan_only_range_exhaustion_creates_no_durable_segment_allocation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.published_port_range = 15000..=15000;
    let backend = KrunSandboxBackend::new(config);
    let spec = sample_spec().with_port_binding(SandboxPortBinding::tcp("explicit", 15000, 9090));

    let error = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("plan-only-range-exhaustion"),
            Some(&sample_launch_defaults()),
            None,
        )
        .expect_err("the image-derived ports must not fit in the exhausted preview range");
    assert!(
        error.to_string().contains("published port range")
            && error.to_string().contains("exhausted"),
        "the original preview failure must remain primary: {error}"
    );

    let segments = backend
        .segment_allocator
        .inspect_segments(&spec.tenant_id)
        .expect("segment authority should inspect")
        .unwrap_or_default();
    assert!(
        segments.is_empty(),
        "authority-free plan preview must fail before creating a segment allocation: {segments:?}"
    );
}

#[test]
fn successful_plan_only_previews_do_not_consume_the_node_segment_pool() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.node_network_supernet = "10.252.0.0/30".to_owned();
    config.node_tenant_subnet_prefix = 30;
    let backend = KrunSandboxBackend::new(config);

    for (tenant, sandbox) in [("preview-a", "app-a"), ("preview-b", "app-b")] {
        let spec = sample_spec_for_tenant(tenant, sandbox);
        backend
            .plan_start_with_id(&spec, &SandboxId::new(sandbox), None, None)
            .expect("an authority-free preview must not exhaust the one-slot node pool");
        assert!(
            backend
                .segment_allocator
                .inspect_segments(&spec.tenant_id)
                .expect("preview segment authority should inspect")
                .unwrap_or_default()
                .is_empty(),
            "a successful preview must not create attachment-less segment authority"
        );
    }
}

#[test]
fn execute_manifest_without_attachment_config_fails_before_network_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let mut manifest = sample_manifest(
        sample_spec_for_tenant("missing-network-authority", "app"),
        KrunStartMode::Execute,
    );
    manifest.network_config = None;
    let claim = crate::backends::oci::port_lease::new_launch_reservation_claim()
        .expect("missing-config test claim should mint");

    let error = backend
        .configure_network(
            &manifest,
            AttachmentAttachAuthority::FreshLaunch(&claim),
            true,
        )
        .expect_err("missing attachment config must fail before Netavark");
    assert!(
        error.to_string().contains("no reserved network attachment"),
        "the failure must name the missing attachment authority: {error}"
    );
    assert!(
        !manifest.network_layout.netns_path.exists(),
        "a manifest without attachment authority must not create a network namespace"
    );
}

#[test]
fn oci_image_root_plan_only_does_not_charge_manifest_only_port_previews() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let image_reference = sample_registry_image_reference();

    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.use_buildah_unshare = false;
    config.published_port_range = 15000..=15005;
    config.max_published_ports_per_tenant = Some(1);
    let state_root = config.network_state_root.clone();

    let backend = KrunSandboxBackend::new(config);

    let mut first_spec = sparse_image_spec("first");
    first_spec.root = SandboxRootSpec::oci_image_reference(image_reference.clone());
    materialize_plan_only_fixture(&backend, first_spec)
        .expect("first image-backed service plan should render");

    let mut second_spec = sparse_image_spec("second");
    second_spec.root = SandboxRootSpec::oci_image_reference(image_reference);
    let second = materialize_plan_only_fixture(&backend, second_spec)
        .expect("a manifest-only preview must not consume durable tenant quota");
    assert!(
        !second.published_endpoints.is_empty(),
        "the second plan should still render its image-derived endpoint"
    );
    let durable = LocalPortLeaseAuthority::open(&state_root)
        .expect("durable port authority should open")
        .list()
        .expect("durable port authority should list");
    assert!(
        durable.is_empty(),
        "manifest-only previews must not be charged as durable tenant-published leases: {durable:?}"
    );
}

#[test]
fn plan_only_backend_rejects_same_tenant_active_sandbox_quota_exhaustion() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    );
    config.resource_quota_policy = SandboxResourceQuotaPolicy::default()
        .with_max_active_sandboxes_per_tenant(Some(1))
        .with_max_vcpus_per_tenant(None)
        .with_max_memory_bytes_per_tenant(None)
        .with_max_disk_bytes_per_tenant(None)
        .with_max_log_bytes_per_tenant(None);
    let backend = KrunSandboxBackend::new(config);

    materialize_plan_only_fixture(&backend, sample_spec())
        .expect("first plan-only sandbox should consume the single active slot");

    let error = materialize_plan_only_fixture(&backend, sample_spec_for_tenant("tenant", "api"))
        .expect_err("second same-tenant sandbox should exceed active sandbox quota");

    assert!(
        error.to_string().contains("active sandbox quota exceeded")
            && error.to_string().contains("tenant")
            && error.to_string().contains("limit 1"),
        "expected active sandbox quota error, got: {error}"
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
        SandboxOwnerSpec::service("api"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/srv/rootfs")),
        SandboxProcessSpec::new(["/bin/service"]),
    )
    .with_port_bindings([
        SandboxPortBinding::tcp("postgres", 15432, 5432),
        SandboxPortBinding::new("http", EndpointProtocol::Http, 18080, 8080),
    ]);
    let manifest = sample_manifest(spec, KrunStartMode::Execute);

    assert_eq!(
        readiness_probe_target(&published_endpoints(&manifest.spec)),
        Some(ReadinessProbeTarget::Http(SocketAddr::from((
            [127, 0, 0, 1],
            18080
        ))))
    );
}

#[test]
fn running_status_passes_the_exact_http_target_and_image_timeout_to_the_provider() {
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        SandboxOwnerSpec::service("http-service"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/srv/rootfs")),
        SandboxProcessSpec::new(["/bin/service"]),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        EndpointProtocol::Http,
        18_080,
        8080,
    ));
    let mut manifest = sample_manifest(spec, KrunStartMode::Execute);
    manifest.image_metadata.healthcheck = Some(ImageHealthcheck {
        test: Vec::new(),
        interval: None,
        timeout: Some(37_000_000),
        start_period: None,
        retries: None,
    });
    let provider = FixedReadinessProbeProvider::ready();

    assert_eq!(running_status(&manifest, &provider), SandboxStatus::Ready);
    assert_eq!(
        provider.calls(),
        vec![(
            ReadinessProbeTarget::Http(SocketAddr::from(([127, 0, 0, 1], 18_080))),
            Duration::from_millis(37),
        )]
    );
}

#[test]
fn krun_composition_accepts_a_deterministic_readiness_provider() {
    let temp = TempDir::new().expect("temporary root should create");
    let fixed = std::sync::Arc::new(FixedReadinessProbeProvider::ready());
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp.path().join("bundles"),
        temp.path().join("state"),
    ))
    .with_readiness_probe_provider(fixed.clone());
    let target = ReadinessProbeTarget::Tcp(
        "127.0.0.1:18080"
            .parse::<SocketAddr>()
            .expect("target should parse"),
    );
    let timeout = Duration::from_millis(41);

    assert_eq!(
        backend.readiness_probe_provider.probe(target, timeout),
        ReadinessProbeObservation::Ready
    );
    assert_eq!(fixed.calls(), vec![(target, timeout)]);
}

#[test]
fn running_status_stays_starting_until_probe_passes() {
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        SandboxOwnerSpec::service("tcp-service"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/srv/rootfs")),
        SandboxProcessSpec::new(["/bin/service"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("tcp", 18_080, 8080));
    let manifest = sample_manifest(spec, KrunStartMode::Execute);
    let provider = FixedReadinessProbeProvider::not_ready("connection refused");

    assert_eq!(
        running_status(&manifest, &provider),
        SandboxStatus::Starting
    );
}

#[test]
fn running_status_degrades_ready_sandboxes_to_not_ready_on_probe_failure() {
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        SandboxOwnerSpec::service("http-service"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/srv/rootfs")),
        SandboxProcessSpec::new(["/bin/service"]),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        EndpointProtocol::Http,
        18_080,
        8080,
    ));
    let mut manifest = sample_manifest(spec, KrunStartMode::Execute);
    manifest.status = SandboxStatus::Ready;
    manifest.handle.status = SandboxStatus::Ready;
    manifest.handle.published_endpoints =
        visible_published_endpoints(KrunStartMode::Execute, &manifest.spec, SandboxStatus::Ready);
    let provider = FixedReadinessProbeProvider::unknown("inspection unavailable");

    assert_eq!(
        running_status(&manifest, &provider),
        SandboxStatus::NotReady
    );
}

#[test]
fn running_status_recovers_not_ready_sandboxes_when_probe_returns() {
    let spec = SandboxSpec::new(
        TenantId::new("tenant").expect("tenant id should be valid"),
        SandboxOwnerSpec::service("http-service"),
        SandboxBackendKind::Krun,
        SandboxRootSpec::Rootfs(SandboxRootfsSpec::new("/srv/rootfs")),
        SandboxProcessSpec::new(["/bin/service"]),
    )
    .with_port_binding(SandboxPortBinding::new(
        "http",
        EndpointProtocol::Http,
        18_080,
        8080,
    ));
    let mut manifest = sample_manifest(spec, KrunStartMode::Execute);
    manifest.status = SandboxStatus::NotReady;
    manifest.handle.status = SandboxStatus::NotReady;
    let provider = FixedReadinessProbeProvider::new(ReadinessProbeObservation::Ready);

    assert_eq!(running_status(&manifest, &provider), SandboxStatus::Ready);
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
    fs::write(
        &state_stub,
        "#!/bin/sh\nprintf '%s\\n' 'container `db-01` does not exist: open `/run/crun/db-01/status`: No such file or directory' >&2\nexit 1\n",
    )
    .expect("state stub should write");
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
        visible_published_endpoints(KrunStartMode::Execute, &spec, SandboxStatus::Starting)
            .is_empty(),
        "execute-mode sandboxes should not publish endpoints before readiness succeeds"
    );
    assert_eq!(
        visible_published_endpoints(KrunStartMode::Execute, &spec, SandboxStatus::Ready).len(),
        2
    );
    assert!(
        visible_published_endpoints(KrunStartMode::Execute, &spec, SandboxStatus::NotReady)
            .is_empty(),
        "execute-mode sandboxes should withdraw endpoints when liveness probes regress"
    );
    assert_eq!(
        visible_published_endpoints(KrunStartMode::PlanOnly, &spec, SandboxStatus::Starting).len(),
        2,
        "plan-only starts should retain published endpoints for deterministic tests"
    );
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
