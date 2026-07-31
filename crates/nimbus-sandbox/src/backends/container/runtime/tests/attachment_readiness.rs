//! Complete host-managed attachment readiness proofs for the container
//! pre-spawn and live-status consumers.

use super::*;

/// NNC0.6 regression for NNCF6. Reaching the netns-created boundary does not
/// prove that Netavark status, the egress pin, listener lifetimes, or the
/// complete durable attachment exists. The production pre-spawn gate must
/// consume the common complete-readiness decision.
#[test]
fn nnc0_6_container_is_not_ready_at_partial_attachment_boundary() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let proxy_reservation = TcpListener::bind("127.0.0.1:0").expect("PEP port fixture should bind");
    let proxy_port = proxy_reservation
        .local_addr()
        .expect("PEP port fixture should report its address")
        .port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = proxy_port..=proxy_port;
    let backend = ContainerSandboxBackend::new(config);
    let manifest = backend
        .plan_start_with_id(&sample_spec(), &sandbox_id(), None, None)
        .expect("plan should lower")
        .manifest;
    std::fs::create_dir_all(
        manifest
            .network_layout
            .netns_path
            .parent()
            .expect("netns path should have a parent"),
    )
    .expect("netns parent should create");
    std::fs::write(&manifest.network_layout.netns_path, b"netns")
        .expect("netns-created boundary should persist");
    assert!(
        !manifest.network_layout.status_path.exists(),
        "precondition: Netavark status must still be absent at this partial boundary"
    );
    drop(proxy_reservation);
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("planned launch should retain its reservation claim"),
            ),
        )
        .expect("a ready PEP isolates the incomplete attachment condition");

    let readiness = backend.require_complete_attachment_readiness(&manifest);

    assert!(
        readiness.is_err(),
        "NNCF6: workload liveness without complete same-generation attachment evidence \
         must not pass the container pre-spawn gate"
    );
}

#[test]
fn container_live_status_withdraws_and_restores_endpoints_with_attachment_evidence() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let endpoint_listener =
        TcpListener::bind("127.0.0.1:0").expect("published endpoint fixture should bind");
    let endpoint_port = endpoint_listener
        .local_addr()
        .expect("published endpoint fixture should report its address")
        .port();
    let pep_reservation = TcpListener::bind("127.0.0.1:0").expect("PEP port fixture should bind");
    let pep_port = pep_reservation
        .local_addr()
        .expect("PEP port fixture should report its address")
        .port();
    let mut config = ContainerSandboxBackendConfig::under_root(temp_dir.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = endpoint_port.min(pep_port)..=endpoint_port.max(pep_port);
    let pin = Arc::new(FixedOciEgressPinProvider::ready());
    let backend = ContainerSandboxBackend::new(config).with_egress_pin_provider(pin.clone());
    let sandbox_id = SandboxId::new("container-attachment-readiness-withdrawal");
    let spec = sample_spec_for_tenant("container-attachment-readiness-withdrawal", "live-runtime")
        .with_port_binding(SandboxPortBinding::tcp(
            "published-api",
            endpoint_port,
            8080,
        ));
    let mut manifest = backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("execute planning should reserve exact network and PEP authority")
        .manifest;
    let launch_claim = manifest
        .launch_reservation_claim
        .clone()
        .expect("execute planning should retain its reservation claim");
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &launch_claim,
        )
        .expect("fixture should adopt the exact attachment reservation");
    let network_config = manifest
        .require_network_config()
        .expect("execute manifest should retain network config")
        .clone();
    let ports = backend
        .port_lease_coordinator_for_manifest(&manifest)
        .expect("manifest should select its exact port authority");
    let hostname = hostname_for(&manifest.spec);
    backend
        .attachment_adapter(&manifest, &network_config, &hostname, None)
        .attach_with_test_host(
            &backend.attachment_lifecycle(&ports),
            AttachmentAttachAuthority::FreshLaunch(&launch_claim),
            |_| {
                pin.apply(
                    &manifest.network_layout,
                    manifest
                        .egress_proxy
                        .as_ref()
                        .expect("execute manifest should retain its PEP assignment"),
                )
            },
        )
        .expect("fixture should realize the complete host-managed attachment");
    drop(pep_reservation);
    backend
        .ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&launch_claim),
        )
        .expect("fixture should start the exact desired PEP");
    backend
        .require_complete_attachment_readiness(&manifest)
        .expect("complete evidence should reach the existing runtime-spawn boundary");

    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"running\"}}'",
            manifest.handle.id
        ),
    ]);
    let _ = std::fs::remove_file(&manifest.conmon_layout.exit_status_file);
    synchronize_handle_status(&mut manifest, SandboxStatus::Ready);
    let status_bytes = std::fs::read(&manifest.network_layout.status_path)
        .expect("exact Netavark status should read");

    let ready = backend
        .detect_runtime_status(&manifest)
        .expect("complete live runtime should inspect");
    assert_eq!(ready, SandboxStatus::Ready);
    synchronize_handle_status(&mut manifest, ready);
    assert_eq!(
        manifest.handle.published_endpoints.len(),
        1,
        "complete attachment evidence should publish the endpoint"
    );

    std::fs::remove_file(&manifest.network_layout.status_path)
        .expect("Netavark status facet should be removable");
    let withdrawn = backend
        .detect_runtime_status(&manifest)
        .expect("live runtime with lost attachment evidence should inspect");
    assert_eq!(withdrawn, SandboxStatus::NotReady);
    synchronize_handle_status(&mut manifest, withdrawn);
    assert!(
        manifest.handle.published_endpoints.is_empty(),
        "losing one required attachment facet must withdraw every endpoint"
    );

    std::fs::write(&manifest.network_layout.status_path, status_bytes)
        .expect("exact Netavark status facet should restore");
    let restored = backend
        .detect_runtime_status(&manifest)
        .expect("restored exact attachment evidence should inspect");
    assert_eq!(restored, SandboxStatus::Ready);
    synchronize_handle_status(&mut manifest, restored);
    assert_eq!(
        manifest.handle.published_endpoints.len(),
        1,
        "restoring exact evidence should permit application readiness to recover"
    );
    drop(endpoint_listener);
}

#[test]
fn netavark_endpoint_effect_requires_complete_current_port_leases() {
    let temp_dir = TempDir::new().expect("tempdir should build");
    let backend =
        ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(temp_dir.path()));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec().with_port_binding(SandboxPortBinding::tcp("http", 18080, 8080)),
            &SandboxId::new("netavark-port-authority"),
            None,
            None,
        )
        .expect("execute manifest should reserve the endpoint")
        .manifest;
    assert_eq!(manifest.port_leases.len(), 1);
    manifest.port_leases.clear();

    let error = backend
        .configure_network(
            &manifest,
            AttachmentAttachAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("planned launch should retain coordinator claim"),
            ),
            MachinePortPreparationReleaseAuthority::FreshLaunch(
                manifest
                    .launch_reservation_claim
                    .as_ref()
                    .expect("planned launch should retain coordinator claim"),
            ),
        )
        .expect_err("provider setup without the complete lease set must fail");
    assert!(
        error
            .to_string()
            .contains("1 published bindings but 0 durable port leases"),
        "the rejection must name the missing authority: {error}"
    );
    assert!(
        !manifest.network_layout.netns_path.exists()
            && !manifest.network_layout.status_path.exists(),
        "lease validation must precede namespace creation and Netavark provider effects"
    );
}
