//! Per-listener teardown progress and retry proofs.

use super::*;

fn two_listener_fixture(
    name: &str,
) -> (
    TempDir,
    OciPortLeaseCoordinator,
    TenantId,
    SandboxId,
    [SandboxPortBinding; 2],
    ReservedLaunchPorts,
) {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_001);
    let tenant = tenant_id(&format!("tenant-{name}"));
    let sandbox = SandboxId::new(name);
    let bindings = [
        SandboxPortBinding::tcp("blocked", 15_000, 8080),
        SandboxPortBinding::tcp("progress", 15_001, 8081),
    ];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox, &bindings, &[])
        .expect("two-listener batch should reserve");
    (temp_dir, manager, tenant, sandbox, bindings, reserved)
}

fn activate_listener(
    manager: &OciPortLeaseCoordinator,
    tenant: &TenantId,
    sandbox: &SandboxId,
    binding: &SandboxPortBinding,
    lease: &nimbus_network::PortLeaseRequest,
) {
    let claims = manager
        .claim_netavark_bindings(
            tenant,
            sandbox,
            std::slice::from_ref(binding),
            std::slice::from_ref(lease),
        )
        .expect("exact Netavark listener should claim");
    manager
        .activate_netavark_bindings(
            tenant,
            sandbox,
            std::slice::from_ref(binding),
            std::slice::from_ref(lease),
            &claims,
        )
        .expect("exact Netavark listener should activate");
}

fn phase(
    authority: &nimbus_network::LocalPortLeaseAuthority,
    request: &nimbus_network::PortLeaseRequest,
) -> nimbus_network::PortLeasePhase {
    authority
        .inspect(request.lease_id())
        .expect("lease should inspect")
        .expect("lease should remain durable")
        .phase()
}

#[test]
fn withdraw_batch_preserves_later_progress_and_retry_skips_completed_members() {
    let (temp_dir, manager, tenant, sandbox, bindings, reserved) =
        two_listener_fixture("withdraw-progress");
    activate_listener(
        &manager,
        &tenant,
        &sandbox,
        &bindings[1],
        &reserved.published_leases[1],
    );
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");

    let error = manager
        .withdraw_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect_err("the still-reserved first listener must reject withdrawal");
    assert_eq!(
        phase(&authority, &reserved.published_leases[0]),
        nimbus_network::PortLeasePhase::Reserved,
        "the failed member must retain its reservation coordinator"
    );
    assert_eq!(
        phase(&authority, &reserved.published_leases[1]),
        nimbus_network::PortLeasePhase::Withdrawing,
        "an earlier failure must not prevent a later exact listener from entering withdrawal"
    );
    assert!(
        error.to_string().contains("blocked")
            && error
                .to_string()
                .contains(reserved.published_leases[0].lease_id().as_str()),
        "aggregate withdrawal failure must identify the exact blocked listener: {error}"
    );

    let after_progress = authority
        .list()
        .expect("withdrawal progress should inspect");
    manager
        .withdraw_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect_err("retry must still report the unresolved first listener");
    assert_eq!(
        authority.list().expect("withdrawal retry should inspect"),
        after_progress,
        "retry must not replay or mutate the already-withdrawing listener"
    );
}

#[test]
fn release_batch_preserves_later_progress_and_retry_skips_completed_members() {
    let (temp_dir, manager, tenant, sandbox, bindings, reserved) =
        two_listener_fixture("release-progress");
    for (binding, lease) in bindings.iter().zip(&reserved.published_leases) {
        activate_listener(&manager, &tenant, &sandbox, binding, lease);
    }
    manager
        .withdraw_bindings(
            &tenant,
            &sandbox,
            &bindings[1..],
            &reserved.published_leases[1..],
        )
        .expect("the second listener should enter withdrawal");
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");

    let error = manager
        .release_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect_err("the still-active first listener must reject release");
    assert_eq!(
        phase(&authority, &reserved.published_leases[0]),
        nimbus_network::PortLeasePhase::Active,
        "the failed member must retain active provider authority"
    );
    assert_eq!(
        phase(&authority, &reserved.published_leases[1]),
        nimbus_network::PortLeasePhase::Released,
        "an earlier failure must not prevent a later withdrawn listener from releasing"
    );
    assert!(
        error.to_string().contains("blocked")
            && error
                .to_string()
                .contains(reserved.published_leases[0].lease_id().as_str()),
        "aggregate release failure must identify the exact blocked listener: {error}"
    );

    let after_progress = authority.list().expect("release progress should inspect");
    manager
        .release_bindings(&tenant, &sandbox, &bindings, &reserved.published_leases)
        .expect_err("retry must still report the unresolved first listener");
    assert_eq!(
        authority.list().expect("release retry should inspect"),
        after_progress,
        "retry must not replay or mutate the already-released listener"
    );
}

#[test]
fn teardown_batch_authenticates_every_member_before_any_progress() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_001);
    let tenant = tenant_id("tenant-authenticated-batch");
    let sandbox = SandboxId::new("authenticated-batch");
    let binding = SandboxPortBinding::tcp("owned", 15_000, 8080);
    let owned = reserve_complete_launch(
        &manager,
        &tenant,
        &sandbox,
        std::slice::from_ref(&binding),
        &[],
    )
    .expect("owned listener should reserve");
    activate_listener(
        &manager,
        &tenant,
        &sandbox,
        &binding,
        &owned.published_leases[0],
    );

    let foreign_tenant = tenant_id("tenant-foreign-batch");
    let foreign_sandbox = SandboxId::new("foreign-batch");
    let foreign_binding = SandboxPortBinding::tcp("foreign", 15_001, 8081);
    let foreign = reserve_complete_launch(
        &manager,
        &foreign_tenant,
        &foreign_sandbox,
        std::slice::from_ref(&foreign_binding),
        &[],
    )
    .expect("foreign listener should reserve");
    let bindings = [binding, foreign_binding];
    let leases = [
        owned.published_leases[0].clone(),
        foreign.published_leases[0].clone(),
    ];
    let authority = nimbus_network::LocalPortLeaseAuthority::open(temp_dir.path())
        .expect("authority should reopen");

    manager
        .withdraw_bindings(&tenant, &sandbox, &bindings, &leases)
        .expect_err("foreign withdrawal member must reject the entire batch before progress");
    assert_eq!(
        phase(&authority, &leases[0]),
        nimbus_network::PortLeasePhase::Active,
        "full-batch authentication must precede the first withdrawal mutation"
    );

    manager
        .withdraw_bindings(&tenant, &sandbox, &bindings[..1], &leases[..1])
        .expect("owned listener should enter withdrawal");
    manager
        .release_bindings(&tenant, &sandbox, &bindings, &leases)
        .expect_err("foreign release member must reject the entire batch before progress");
    assert_eq!(
        phase(&authority, &leases[0]),
        nimbus_network::PortLeasePhase::Withdrawing,
        "full-batch authentication must precede the first release mutation"
    );
    assert_eq!(
        phase(&authority, &leases[1]),
        nimbus_network::PortLeasePhase::Reserved,
        "foreign listener authority must remain untouched"
    );
}
