//! Container network planning and pre-provider-effect validation.

use super::*;

#[test]
fn pre_netavark_setup_failure_preserves_no_effect_authority() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.netavark_path = PathBuf::from("/usr/bin/false");
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080)),
            &SandboxId::new("container-setup-detach-compensation"),
            None,
            None,
        )
        .expect("execute manifest should reserve complete network authority")
        .manifest;
    let claims = backend
        .port_lease_coordinator()
        .claim_netavark_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("test must cross the durable claim boundary");
    std::fs::create_dir_all(
        manifest
            .network_layout
            .netns_path
            .parent()
            .expect("netns parent should exist"),
    )
    .expect("netns parent should create");
    std::fs::write(&manifest.network_layout.netns_path, b"owned test netns\n")
        .expect("netns retry handle should exist");

    let error = backend
        .complete_network_setup(
            &manifest,
            manifest
                .network_config
                .as_ref()
                .expect("planned launch should retain network config"),
            None,
            Err(SandboxError::OperationFailed {
                message: "forced netavark setup failure".to_owned(),
            }),
        )
        .expect_err("failed setup must enter the exact detach compensation seam");
    let message = error.to_string();
    assert!(
        message.contains("forced netavark setup failure")
            && !message.contains("detach compensation also failed"),
        "pre-provider failure must preserve the primary error without inventing ambiguity: \
         {message}"
    );
    if cfg!(target_os = "linux") {
        assert!(
            !manifest.network_layout.netns_path.exists(),
            "the separately owned namespace may be removed after Netavark proves no effect"
        );
    }
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should reopen");
    for (request, expected_claim) in manifest.port_leases.iter().zip(claims) {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("claimed lease must remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert_eq!(
            record.bind_claim(),
            Some(&expected_claim),
            "outer launch compensation still owns each exact unactivated bind claim"
        );
    }
}

#[test]
fn foreign_initial_launch_claim_fails_before_container_provider_effects() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080)),
            &SandboxId::new("foreign-container-launch-claim"),
            None,
            None,
        )
        .expect("launch should reserve its complete port batch")
        .manifest;
    let authoritative_claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("initial launch should retain coordinator authority");
    let foreign_provider: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider id should parse");
    manifest.launch_reservation_claim = Some(NetworkReservationClaim::new(
        NetworkProviderHandle::new(foreign_provider, "foreign-container-coordinator")
            .expect("foreign claim should validate"),
    ));
    let mut launch_batch = manifest.port_leases.clone();
    launch_batch.push(
        manifest
            .egress_proxy
            .as_ref()
            .expect("execute launch should reserve its PEP")
            .port_lease
            .clone(),
    );

    let error = backend
        .launch_manifest(&mut manifest, true)
        .expect_err("a foreign coordinator must fail before container provider effects");
    assert!(
        error
            .to_string()
            .contains("different launch reservation coordinator"),
        "the preflight rejection must identify the foreign coordinator: {error}"
    );
    assert!(
        !manifest.network_layout.netns_path.exists()
            && !manifest.network_layout.status_path.exists(),
        "coordinator authentication must precede namespace and Netavark effects"
    );
    let authority =
        nimbus_network::LocalPortLeaseAuthority::open(&backend.config.network_state_root)
            .expect("authority should reopen");
    for request in &launch_batch {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("lease should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert_eq!(record.reservation_claim(), Some(&authoritative_claim));
        assert!(
            record.bind_claim().is_none()
                && record.binding().is_none()
                && record.failure().is_none()
        );
    }
    backend
        .port_lease_coordinator()
        .release_never_bound_requests(&launch_batch, &authoritative_claim)
        .expect("the exact coordinator should clean up the test batch");
}

#[test]
fn plan_only_cleanup_does_not_contact_machine_port_forwarder() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    // The claim keeps this forwarder endpoint unanswered for the whole test,
    // which is what makes a contacted forwarder observable as a failure.
    let port_window = PortWindow::claim();
    let unavailable_port = port_window.port(0);
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.start_mode = ContainerStartMode::PlanOnly;
    config.machine_port_forwarder = Some(sample_forwarder(unavailable_port));
    let backend = ContainerSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080)),
            &sandbox_id(),
            None,
            None,
        )
        .expect("plan-only manifest should lower")
        .manifest;
    assert!(
        manifest.port_leases.is_empty(),
        "plan-only lowering must not reserve host-global port authority"
    );

    backend
        .release_execution_artifacts(&mut manifest)
        .expect("plan-only cleanup must not contact an effect provider it never activated");
}

/// MTN5 DNS-off posture: the container backend disables the in-subnet
/// aardvark-dns resolver (`enable_dns=false`), matching the krun backend. Under
/// the H1 pin gateway:53 is unreachable, so the resolver is dead weight and a
/// cross-tenant DNS-leak surface; names resolve host-side through the egress PEP.
#[test]
fn container_network_config_disables_bridge_dns_resolver() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));

    let tenant = nimbus_core::TenantId::new("dns-tenant").expect("tenant should parse");
    assert!(
        !backend
            .network_config(&tenant)
            .expect("network config should resolve")
            .enable_dns,
        "the container backend must disable the bridge DNS resolver (enable_dns=false)"
    );
}
