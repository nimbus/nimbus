//! Provider-batch classification boundary proofs.

use super::*;

#[test]
fn reservation_coordinator_is_classified_from_one_durable_generation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_001)
        .with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-reservation-snapshot");
    let sandbox = SandboxId::new("reservation-snapshot");
    let bindings = [
        SandboxPortBinding::tcp("first", 15_000, 8080),
        SandboxPortBinding::tcp("second", 15_001, 8081),
    ];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox, &bindings, &[])
        .expect("complete listener batch should reserve");
    let claims = manager
        .claim_machine_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("complete listener batch should retain bind claims");
    let expected_claim = reserved.reservation_claim.clone();
    let inspected_leases = reserved.published_leases.clone();
    let inspecting_manager = manager.clone();
    let (first_read_tx, first_read_rx) = mpsc::channel();
    let (activation_complete_tx, activation_complete_rx) = mpsc::channel();
    let inspector = std::thread::spawn(move || {
        inspecting_manager.reservation_claim_for_requests_with_observer(
            &inspected_leases,
            |index| {
                if index == 0 {
                    first_read_tx
                        .send(())
                        .expect("first durable record read should signal");
                    activation_complete_rx
                        .recv_timeout(Duration::from_secs(2))
                        .expect("activation must complete before classification resumes");
                }
            },
        )
    });
    first_read_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("classification should reach the inter-generation barrier");

    manager
        .activate_machine_bindings(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            &claims,
        )
        .expect("the complete batch should activate atomically");
    activation_complete_tx
        .send(())
        .expect("classification should resume after activation");

    assert_eq!(
        inspector
            .join()
            .expect("classification worker should not panic")
            .expect("one-snapshot classification must not synthesize a mixed generation"),
        Some(expected_claim),
        "classification must report the uniform coordinator from its original snapshot"
    );
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("port authority should reopen");
    let active = authority.list().expect("active authority should inspect");
    assert_eq!(active.len(), 2);
    assert!(
        active.iter().all(|record| {
            record.phase() == nimbus_network::PortLeasePhase::Active
                && record.reservation_claim().is_none()
        }),
        "the concurrently committed generation must be uniformly active and claimless"
    );
}

#[test]
fn empty_netavark_cleanup_is_terminal_no_effect_and_release_is_noop() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_000);
    let tenant = tenant_id("tenant-netavark-empty-cleanup");
    let sandbox = SandboxId::new("netavark-empty-cleanup");

    assert_eq!(
        manager
            .classify_netavark_cleanup_batch(&tenant, &sandbox, &[], &[], None)
            .expect("an authenticated empty Netavark batch should classify"),
        LaunchPortBatchState::TerminalNoEffect,
        "no requested listeners means no provider effect remains"
    );
    manager
        .release_restart_retained_bindings(&tenant, &sandbox, &[], &[])
        .expect("terminal no-effect release should be an idempotent no-op");
    assert!(
        nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
            .expect("port authority should open")
            .list()
            .expect("port authority should list")
            .is_empty(),
        "empty cleanup must not synthesize durable lease authority"
    );
}

#[test]
fn empty_netavark_cleanup_requires_cardinality_and_provider_authentication() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_000);
    let tenant = tenant_id("tenant-netavark-empty-auth");
    let sandbox = SandboxId::new("netavark-empty-auth");
    let binding = SandboxPortBinding::tcp("http", 15_000, 8080);

    let cardinality = manager
        .classify_netavark_cleanup_batch(
            &tenant,
            &sandbox,
            std::slice::from_ref(&binding),
            &[],
            None,
        )
        .expect_err("a truncated nonempty binding set must fail before classification");
    assert!(
        cardinality.to_string().contains("0 durable port leases"),
        "cardinality rejection must identify missing durable authority: {cardinality}"
    );

    let machine = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_000)
        .with_machine_port_proxy_bindings();
    let provider = machine
        .classify_netavark_cleanup_batch(&tenant, &sandbox, &[], &[], None)
        .expect_err("machine provider mode must reject Netavark classification");
    assert!(
        provider.to_string().contains("Netavark")
            && provider.to_string().contains("MachinePortProxy"),
        "provider rejection must name both configured and requested owners: {provider}"
    );
}

#[test]
fn separate_owner_terminal_publication_accepts_foreign_historical_provider_evidence_read_only() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_000);
    let tenant = tenant_id("tenant-separate-owner-terminal");
    let sandbox = SandboxId::new("separate-owner-terminal");
    let spec = crate::SandboxSpec::new(
        tenant.clone(),
        crate::SandboxOwnerSpec::standalone_named("separate-owner-terminal"),
        crate::SandboxBackendKind::Krun,
        crate::SandboxRootSpec::rootfs("/separate-owner-terminal"),
        crate::SandboxProcessSpec::new(["/bin/true"]),
    )
    .with_port_binding(SandboxPortBinding::tcp("http", 15_000, 8080));
    let plan = crate::provision::test_support::sandbox_provision_network_plan_fixture(
        &spec,
        &sandbox,
        "separate-owner-terminal",
    );
    let claim = new_launch_reservation_claim().expect("launch claim should mint");
    let mut reserved = manager
        .reserve_exact_provision_ports(&plan, None, &claim)
        .expect("separate owner listener should reserve");
    reserved
        .confirm_manifest_published()
        .expect("exact plan members should become durable");
    let bindings = reserved.published_bindings.clone();
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    let request = &reserved.published_leases[0];
    let provider_handle = nimbus_network::NetworkProviderHandle::new(
        nimbus_network::NetworkProviderId::for_registration_key("nimbus-server.test-ingress"),
        "server-owned-test-listener",
    )
    .expect("foreign provider handle should validate");
    let bind_claim = nimbus_network::PortBindClaim::new(provider_handle.clone());
    let provider_binding = nimbus_network::PortLeaseBinding::new(
        nimbus_network::PortBoundEndpoint::new(
            request.binding().protocol(),
            request.binding().realm().clone(),
            request.binding().target().clone(),
            std::num::NonZeroU16::new(15_000).expect("fixture port should be non-zero"),
        )
        .expect("foreign provider endpoint should validate"),
        nimbus_network::PortBindingProvenance::NimbusOwned,
        provider_handle,
    );
    authority
        .claim_bind(
            request,
            Some(&reserved.reservation_claim),
            bind_claim.clone(),
        )
        .expect("foreign provider attempt should become durable");
    authority
        .adopt_claimed(
            request,
            Some(&reserved.reservation_claim),
            &bind_claim,
            provider_binding,
        )
        .expect("foreign provider binding should become durable");
    authority
        .activate_claimed(request, &bind_claim)
        .expect("foreign provider listener should activate");
    let active = authority
        .list()
        .expect("active separate-owner evidence should inspect");
    let error = manager
        .authenticate_separate_owner_publication_terminal(
            &reserved.published_leases,
            &tenant,
            &bindings,
            &reserved.published_leases,
        )
        .expect_err("attachment teardown must reject a separate owner's live effect");
    assert!(
        error.to_string().contains("effect owner") && error.to_string().contains("Active"),
        "live rejection must identify the unresolved separate authority: {error}"
    );
    assert_eq!(
        authority
            .list()
            .expect("failed terminal authentication should re-inspect"),
        active,
        "failed terminal authentication must be read-only"
    );

    authority
        .withdraw(request)
        .expect("separate owner should withdraw its exact listener");
    authority
        .release(request)
        .expect("separate owner should record terminal release");
    let before = authority
        .list()
        .expect("terminal separate-owner evidence should inspect");
    assert!(
        before[0].binding().is_some(),
        "terminal authority should retain historical foreign-provider evidence"
    );
    let records = manager
        .authenticate_separate_owner_publication_terminal(
            &reserved.published_leases,
            &tenant,
            &bindings,
            &reserved.published_leases,
        )
        .expect("provider-neutral terminal authentication should accept the exact foreign record");
    assert_eq!(records, before);
    assert_eq!(
        authority
            .list()
            .expect("terminal authentication should remain read-only"),
        before,
        "attachment teardown must not rewrite or release separate-owner evidence"
    );
}
