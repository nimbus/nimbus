//! Provider-batch classification boundary proofs.

use super::*;

#[test]
fn reservation_coordinator_is_classified_from_one_durable_generation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager =
        PortManager::new(temp_dir.path(), 15_000..=15_001).with_machine_port_proxy_bindings();
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
    let manager = PortManager::new(temp_dir.path(), 15_000..=15_000);
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
    let manager = PortManager::new(temp_dir.path(), 15_000..=15_000);
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

    let machine =
        PortManager::new(temp_dir.path(), 15_000..=15_000).with_machine_port_proxy_bindings();
    let provider = machine
        .classify_netavark_cleanup_batch(&tenant, &sandbox, &[], &[], None)
        .expect_err("machine provider mode must reject Netavark classification");
    assert!(
        provider.to_string().contains("Netavark")
            && provider.to_string().contains("MachinePortProxy"),
        "provider rejection must name both configured and requested owners: {provider}"
    );
}
