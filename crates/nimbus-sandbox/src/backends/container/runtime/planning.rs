use super::support::*;

use tempfile::TempDir;

#[test]
fn plan_only_backend_persists_a_container_manifest() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend = sample_plan_only_backend(temp_dir.path());

    let handle = backend
        .start_sync(sample_spec().with_port_binding(SandboxPortBinding::tcp("db", 5432, 5432)))
        .expect("container plan should start");

    assert_eq!(handle.backend, SandboxBackendKind::Container);
    let manifest_path = temp_dir
        .path()
        .join("state")
        .join("containers")
        .join(handle.id.as_str())
        .join("manifest.json");
    assert!(manifest_path.is_file(), "manifest should be written");
}

#[test]
fn plan_only_backend_auto_assigns_exposed_ports_from_published_range() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.launch_mode = ContainerLaunchMode::PlanOnly;
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
