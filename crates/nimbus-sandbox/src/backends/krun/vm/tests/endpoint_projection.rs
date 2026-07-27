//! Desired, durable, and observed endpoint-projection proofs.

use super::support::*;

use nimbus_network::LocalPortLeaseAuthority;

use crate::backends::krun::vm::readiness::synchronize_handle_status;

#[test]
fn execute_auto_port_projection_uses_reserved_port_only_when_ready() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.published_port_range = 25_000..=25_010;
    let backend = KrunSandboxBackend::new(config);
    let sandbox_id = SandboxId::new("krun-auto-port-observed-projection");
    let spec = sample_spec_for_tenant("krun-auto-port-projection", "api");
    let mut manifest = backend
        .plan_start_with_id(&spec, &sandbox_id, Some(&sample_launch_defaults()), None)
        .expect("execute planning should reserve image-exposed listeners")
        .manifest;
    assert!(
        manifest.handle.published_endpoints.is_empty(),
        "reservation must not publish execute-mode endpoints before readiness"
    );
    assert_eq!(
        manifest.spec.port_bindings.len(),
        manifest.port_leases.len()
    );
    assert!(!manifest.port_leases.is_empty());

    let authority = LocalPortLeaseAuthority::open(&backend.config.state_root)
        .expect("port authority should reopen");
    for (binding, lease) in manifest
        .spec
        .port_bindings
        .iter()
        .zip(&manifest.port_leases)
    {
        let record = authority
            .inspect(lease.lease_id())
            .expect("reserved lease should inspect")
            .expect("reserved lease should remain durable");
        assert_eq!(
            record
                .reserved_port()
                .expect("execute reservation should select a port")
                .get(),
            binding.host_port,
            "desired binding must use the exact authority-selected port"
        );
    }
    let persisted = backend
        .read_manifest(&sandbox_id)
        .expect("planned manifest should inspect")
        .expect("planned manifest should remain durable");
    assert_eq!(persisted.spec.port_bindings, manifest.spec.port_bindings);
    assert_eq!(persisted.port_leases, manifest.port_leases);

    let desired_bindings = manifest.spec.port_bindings.clone();
    let durable_leases = manifest.port_leases.clone();
    synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    assert_eq!(
        manifest.handle.published_endpoints.len(),
        desired_bindings.len(),
        "Ready observation must publish every exact desired endpoint"
    );
    for (endpoint, binding) in manifest
        .handle
        .published_endpoints
        .iter()
        .zip(&desired_bindings)
    {
        assert_eq!(endpoint.address.port(), binding.host_port);
        assert_eq!(endpoint.name, binding.name);
        assert_eq!(endpoint.protocol, binding.protocol);
    }

    synchronize_handle_status(&mut manifest, SandboxStatus::NotReady);
    assert!(
        manifest.handle.published_endpoints.is_empty(),
        "NotReady observation must withdraw endpoints"
    );
    assert_eq!(
        manifest.spec.port_bindings, desired_bindings,
        "observed withdrawal must not mutate desired bindings"
    );
    assert_eq!(
        manifest.port_leases, durable_leases,
        "observed withdrawal must not mutate durable lease authority"
    );
}
