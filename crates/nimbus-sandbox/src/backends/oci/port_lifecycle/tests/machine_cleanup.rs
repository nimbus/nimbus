//! Durable machine-listener cleanup classification proofs.

use super::*;

fn restart_retained_machine_batch(
    manager: &OciPortLeaseCoordinator,
    tenant: &TenantId,
    sandbox: &SandboxId,
    bindings: &[SandboxPortBinding],
) -> ReservedLaunchPorts {
    let reserved = reserve_complete_launch(manager, tenant, sandbox, bindings, &[])
        .expect("machine listener batch should reserve");
    let claims = manager
        .claim_machine_bindings(tenant, sandbox, bindings, &reserved.published_leases)
        .expect("machine provider attempt should become durable");
    let expected = manager
        .activate_machine_bindings(
            tenant,
            sandbox,
            bindings,
            &reserved.published_leases,
            &claims,
        )
        .expect("machine listener batch should activate");
    manager
        .prepare_machine_bindings_for_rebind(&reserved.published_leases, &expected)
        .expect("confirmed provider stop should retain exact listener receipts");
    reserved
}

#[test]
fn empty_machine_cleanup_requires_an_authenticated_empty_binding_set() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_000)
        .with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-empty-cleanup");
    let sandbox = SandboxId::new("machine-empty-cleanup");
    let binding = SandboxPortBinding::tcp("http", 15_000, 8080);

    let error = manager
        .classify_machine_cleanup_batch(&tenant, &sandbox, std::slice::from_ref(&binding), &[])
        .expect_err("a truncated nonempty binding set must fail before classification");
    assert!(
        error.to_string().contains("1 published bindings")
            && error.to_string().contains("0 durable port leases"),
        "cardinality rejection must identify the incomplete durable authority: {error}"
    );

    assert_eq!(
        manager
            .classify_machine_cleanup_batch(&tenant, &sandbox, &[], &[])
            .expect("an authenticated empty machine batch should classify"),
        LaunchPortBatchState::TerminalNoEffect,
        "no requested listeners means no provider effect remains"
    );
    assert!(
        manager
            .machine_bindings_are_terminal_without_effect(&tenant, &sandbox, &[], &[])
            .expect("the authenticated empty machine batch should inspect"),
        "the empty set is vacuously terminal only after exact cardinality authentication"
    );
}

#[test]
fn machine_restart_receipts_release_atomically_and_replay_idempotently() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_001)
        .with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-restart-release");
    let sandbox = SandboxId::new("machine-restart-release");
    let bindings = [
        SandboxPortBinding::tcp("http", 15_000, 8080),
        SandboxPortBinding::tcp("metrics", 15_001, 9090),
    ];
    let reserved = restart_retained_machine_batch(&manager, &tenant, &sandbox, &bindings);

    assert_eq!(
        manager
            .classify_machine_cleanup_batch(
                &tenant,
                &sandbox,
                &bindings,
                &reserved.published_leases,
            )
            .expect("uniform exact receipts should classify"),
        LaunchPortBatchState::RestartRetained
    );
    manager
        .release_restart_retained_machine_bindings(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("uniform exact receipts should release atomically");

    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    for request in &reserved.published_leases {
        let record = authority
            .inspect(request.lease_id())
            .expect("released lease should inspect")
            .expect("released lease should remain durable");
        assert_eq!(record.phase(), nimbus_network::PortLeasePhase::Released);
        assert!(record.confirmed_stopped_binding().is_none());
    }
    assert_eq!(
        manager
            .classify_machine_cleanup_batch(
                &tenant,
                &sandbox,
                &bindings,
                &reserved.published_leases,
            )
            .expect("uniform released receipts should classify"),
        LaunchPortBatchState::TerminalNoEffect,
        "released listeners are terminal no-effect, not restart-retained"
    );
    manager
        .release_restart_retained_machine_bindings(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("terminal release replay should be idempotent");
}

#[test]
fn released_machine_binding_authenticates_exact_historical_provider_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_000)
        .with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-released-binding");
    let sandbox = SandboxId::new("machine-released-binding");
    let bindings = [SandboxPortBinding::tcp("http", 15_000, 8080)];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox, &bindings, &[])
        .expect("machine listener should reserve");
    let claims = manager
        .claim_machine_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("machine provider attempt should become durable");
    manager
        .activate_machine_bindings(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            &claims,
        )
        .expect("machine listener should activate");
    manager
        .withdraw_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("effect owner should withdraw exact listener authority");
    manager
        .release_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("effect owner should record terminal release");

    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    let before = authority
        .list()
        .expect("released provider evidence should inspect");
    assert!(
        before[0].binding().is_some(),
        "normal provider teardown deliberately retains historical binding evidence"
    );
    assert_eq!(
        manager
            .classify_machine_cleanup_batch(
                &tenant,
                &sandbox,
                &bindings,
                &reserved.published_leases,
            )
            .expect("the exact historical provider binding should authenticate"),
        LaunchPortBatchState::TerminalNoEffect
    );
    assert_eq!(
        authority
            .list()
            .expect("released provider evidence should re-inspect"),
        before,
        "classification must preserve exact historical provider evidence"
    );
}

#[test]
fn mixed_terminal_machine_coordinators_fail_before_any_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_001)
        .with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-terminal-coordinator");
    let sandbox = SandboxId::new("machine-terminal-coordinator");
    let first_binding = SandboxPortBinding::tcp("released", 15_000, 8080);
    let second_binding = SandboxPortBinding::tcp("failed", 15_001, 8081);
    let first = reserve_complete_launch(
        &manager,
        &tenant,
        &sandbox,
        std::slice::from_ref(&first_binding),
        &[],
    )
    .expect("first launch coordinator should reserve");
    let second = reserve_complete_launch(
        &manager,
        &tenant,
        &sandbox,
        std::slice::from_ref(&second_binding),
        &[],
    )
    .expect("second launch coordinator should reserve");
    assert_ne!(
        first.reservation_claim, second.reservation_claim,
        "fixture must retain two distinct launch coordinators"
    );
    manager
        .release_never_bound_requests(&first.published_leases, &first.reservation_claim)
        .expect("first coordinator should release its exact listener");
    let failed_claim = manager
        .claim_machine_bindings(
            &tenant,
            &sandbox,
            std::slice::from_ref(&second_binding),
            &second.published_leases,
        )
        .expect("second coordinator should claim its provider attempt")
        .pop()
        .expect("one listener should return one claim");
    manager
        .record_machine_proxy_bind_failure(
            &second.published_leases[0],
            &failed_claim,
            std::net::SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 15_001),
            std::io::ErrorKind::AddrInUse,
        )
        .expect("second coordinator should record exact no-effect failure");

    let bindings = [first_binding, second_binding];
    let leases = [
        first.published_leases[0].clone(),
        second.published_leases[0].clone(),
    ];
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    let before = authority.list().expect("terminal batch should inspect");
    let error = manager
        .classify_machine_cleanup_batch(&tenant, &sandbox, &bindings, &leases)
        .expect_err("terminal evidence from different coordinators must fail closed");
    assert!(
        error
            .to_string()
            .contains("different reservation coordinator"),
        "rejection should name the mismatched terminal coordinator: {error}"
    );
    assert_eq!(
        authority.list().expect("terminal batch should re-inspect"),
        before,
        "coordinator mismatch must reject the complete batch before mutation"
    );
}

#[test]
fn mixed_machine_restart_batch_fails_before_any_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_001)
        .with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-restart-mixed");
    let sandbox = SandboxId::new("machine-restart-mixed");
    let bindings = [
        SandboxPortBinding::tcp("http", 15_000, 8080),
        SandboxPortBinding::tcp("metrics", 15_001, 9090),
    ];
    let reserved = restart_retained_machine_batch(&manager, &tenant, &sandbox, &bindings);
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    authority
        .release_after_confirmed_stop(&reserved.published_leases[0])
        .expect("fixture should create one terminal member");
    let before = authority.list().expect("mixed batch should inspect");

    let error = manager
        .release_restart_retained_machine_bindings(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect_err("mixed retained and released members must fail closed");
    assert!(
        error.to_string().contains("mixes restart-retained"),
        "mixed batch rejection should name the lifecycle conflict: {error}"
    );
    assert_eq!(
        authority.list().expect("mixed batch should re-inspect"),
        before,
        "classification must reject the complete batch before mutation"
    );
}

#[test]
fn active_machine_batch_remains_provider_owned_without_mutation() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_000)
        .with_machine_port_proxy_bindings();
    let tenant = tenant_id("tenant-machine-active-classification");
    let sandbox = SandboxId::new("machine-active-classification");
    let bindings = [SandboxPortBinding::tcp("http", 15_000, 8080)];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox, &bindings, &[])
        .expect("machine listener should reserve");
    let claims = manager
        .claim_machine_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect("machine provider attempt should become durable");
    manager
        .activate_machine_bindings(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            &claims,
        )
        .expect("machine listener should activate");
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");
    let before = authority.list().expect("active batch should inspect");

    assert_eq!(
        manager
            .classify_machine_cleanup_batch(
                &tenant,
                &sandbox,
                &bindings,
                &reserved.published_leases,
            )
            .expect("exact active batch should classify"),
        LaunchPortBatchState::ProviderOwned
    );
    assert_eq!(
        authority.list().expect("active batch should re-inspect"),
        before,
        "read-only classification must preserve the exact provider fence"
    );
}
