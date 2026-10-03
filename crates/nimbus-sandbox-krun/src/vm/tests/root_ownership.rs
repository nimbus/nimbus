//! Backend configuration keeps workload artifacts separate from node authority.

use super::support::*;

#[test]
fn krun_config_names_workload_and_network_roots_explicitly() {
    let root = PathBuf::from("/tmp/nimbus-krun-root-contract");
    let defaulted = KrunSandboxBackendConfig::under_root(&root);
    assert_eq!(defaulted.workload_state_root, root.join("state"));
    assert_eq!(defaulted.network_state_root, root.join("state"));

    let workload = root.join("project-state");
    let network = root.join("node-network-state");
    let split = KrunSandboxBackendConfig::plan_only(root.join("bundles"), &workload)
        .with_network_state_root(&network);
    assert_eq!(split.workload_state_root, workload);
    assert_eq!(split.network_state_root, network);
}

#[test]
fn krun_plan_keeps_artifacts_workload_local_and_authority_network_rooted() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let workload = temp_dir.path().join("project-state");
    let network = temp_dir.path().join("node-network-state");
    let backend = KrunSandboxBackend::new(
        KrunSandboxBackendConfig::plan_only(temp_dir.path().join("bundles"), &workload)
            .with_network_state_root(&network),
    );
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("krun-split-root-plan");

    let plan = backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("split-root plan should render");
    assert_eq!(plan.manifest.network_layout.workload_state_root, workload);
    assert_eq!(plan.manifest.network_layout.network_state_root, network);
    assert!(
        plan.manifest
            .conmon_layout
            .container_state_dir
            .starts_with(&workload),
        "conmon artifacts must remain under the workload root"
    );
    assert!(
        plan.manifest
            .network_layout
            .network_root
            .starts_with(&workload),
        "netns and provider artifacts must remain under the workload root"
    );
    assert!(
        plan.manifest
            .bundle_layout
            .bundle_dir
            .starts_with(temp_dir.path().join("bundles")),
        "bundle artifacts must remain under the bundle root"
    );

    backend
        .segment_allocator
        .segment_for(&spec.tenant_id)
        .expect("portable segment authority should allocate");
    let network_authority = nimbus_network::LocalNetworkStateStore::authority_path_for(&network);
    let workload_authority = nimbus_network::LocalNetworkStateStore::authority_path_for(&workload);
    assert!(network_authority.exists());
    assert!(!workload_authority.exists());
}

#[test]
fn krun_reload_rejects_a_substituted_network_root_before_runtime_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let workload = temp_dir.path().join("project-state");
    let original_network = temp_dir.path().join("node-network-state");
    let foreign_network = temp_dir.path().join("foreign-network-state");
    let bundle_root = temp_dir.path().join("bundles");
    let original = KrunSandboxBackend::new(
        KrunSandboxBackendConfig::plan_only(&bundle_root, &workload)
            .with_network_state_root(&original_network),
    );
    let plan = original
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-root-substitution"),
            None,
            None,
        )
        .expect("original plan should render");
    let handle = materialize_plan_only_plan_fixture(&original, plan)
        .expect("original plan should persist its exact root witness");

    let runtime_effect = temp_dir.path().join("runtime-effect");
    let runtime_probe = temp_dir.path().join("runtime-probe.sh");
    fs::write(
        &runtime_probe,
        format!("#!/bin/sh\ntouch '{}'\nexit 1\n", runtime_effect.display()),
    )
    .expect("runtime probe should write");
    let mut permissions = fs::metadata(&runtime_probe)
        .expect("runtime probe metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&runtime_probe, permissions).expect("runtime probe should be executable");

    let mut substituted = KrunSandboxBackendConfig::plan_only(&bundle_root, &workload)
        .with_network_state_root(&foreign_network);
    substituted.start_mode = KrunStartMode::Execute;
    substituted.runtime_path = runtime_probe;
    let substituted = KrunSandboxBackend::new(substituted);

    let error = substituted
        .inspect_sync(&handle.id)
        .expect_err("a foreign network root must fail before runtime inspection");
    let message = error.to_string();
    assert!(message.contains("network-root mismatch"), "{message}");
    assert!(
        message.contains(&original_network.display().to_string()),
        "{message}"
    );
    assert!(
        message.contains(&foreign_network.display().to_string()),
        "{message}"
    );
    assert!(
        !runtime_effect.exists(),
        "root authentication must precede runtime/provider effects"
    );
}

#[test]
fn krun_root_authentication_rejects_tenant_and_conmon_substitution() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::plan_only(
        temp_dir.path().join("bundles"),
        temp_dir.path().join("state"),
    ));
    let sandbox_id = SandboxId::new("krun-root-witness-validation");
    let manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id, None, None)
        .expect("root witness plan should render")
        .manifest;

    let mut tenant_substitution = manifest.clone();
    tenant_substitution.handle.tenant_id =
        TenantId::new("foreign-tenant").expect("foreign tenant should validate");
    let tenant_error = backend
        .validate_manifest_roots(&sandbox_id, &tenant_substitution)
        .expect_err("handle/spec tenant substitution must fail closed");
    assert!(
        tenant_error.to_string().contains("tenant mismatch"),
        "{tenant_error}"
    );

    let mut conmon_substitution = manifest;
    conmon_substitution.conmon_layout.state_root = temp_dir.path().join("foreign-workload");
    let conmon_error = backend
        .validate_manifest_roots(&sandbox_id, &conmon_substitution)
        .expect_err("conmon root substitution must fail closed");
    assert!(
        conmon_error.to_string().contains("workload-root mismatch"),
        "{conmon_error}"
    );
}
