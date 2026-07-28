//! Netavark process-lifetime and cleanup-pending adapter proofs.

use super::super::NetavarkPortLifetimeRegistry;
use super::*;
use nimbus_network::{LocalPortLeaseAuthority, PortLeasePhase};

#[test]
fn live_owner_rejects_foreign_cleanup_and_dead_owner_stays_fenced_until_exact_absence() {
    let temp_dir = TempDir::new().expect("temporary state root should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_000..=15_000);
    let tenant = tenant_id("tenant-netavark-lifetime");
    let sandbox = SandboxId::new("netavark-lifetime");
    let bindings = [SandboxPortBinding::tcp("http", 15_000, 8080)];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox, &bindings, &[])
        .expect("published listener should reserve");
    let live_batch = manager
        .claim_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("Netavark should claim under one exact live lifetime");
    manager
        .activate_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            &live_batch,
        )
        .expect("Netavark should activate under the same lifetimes");
    let live_registry = NetavarkPortLifetimeRegistry::default();
    live_registry
        .insert(&tenant, &sandbox, live_batch)
        .map_err(|(error, _batch)| error)
        .expect("effect owner should retain its non-cloneable lifetime batch");

    let authority =
        LocalPortLeaseAuthority::open(temp_dir.path()).expect("authority should reopen");
    let active = authority
        .inspect(reserved.published_leases[0].lease_id())
        .expect("active listener should inspect")
        .expect("active listener should remain durable");
    let expected_binding = active
        .binding()
        .cloned()
        .expect("active listener should retain exact provider evidence");
    let recovery_registry = NetavarkPortLifetimeRegistry::default();
    let live_error = match manager.begin_netavark_cleanup(
        &recovery_registry,
        &tenant,
        &sandbox,
        &bindings,
        &reserved.published_leases,
    ) {
        Err(error) => error,
        Ok(_) => panic!("another process view must reject a still-live owner"),
    };
    assert!(
        live_error.to_string().contains("live process lifetime"),
        "live-owner rejection should name the missing death proof: {live_error}"
    );
    assert_eq!(
        authority
            .inspect(reserved.published_leases[0].lease_id())
            .expect("active listener should inspect")
            .expect("active listener should remain durable"),
        active,
        "failed foreign recovery must not mutate durable authority"
    );

    drop(
        live_registry
            .take(&tenant, &sandbox)
            .expect("live registry should remain readable")
            .expect("live effect owner should retain its exact batch"),
    );
    let cleanup = manager
        .begin_netavark_cleanup(
            &recovery_registry,
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("dead owner should yield one exclusive recovery capability")
        .expect("active provider evidence should require exact cleanup");
    let pending = authority
        .inspect(reserved.published_leases[0].lease_id())
        .expect("cleanup-pending listener should inspect")
        .expect("cleanup-pending listener should remain durable");
    assert_eq!(pending.phase(), PortLeasePhase::CleanupPending);
    assert_eq!(pending.binding(), Some(&expected_binding));

    let conflict_tenant = tenant_id("tenant-netavark-conflict");
    let conflict_sandbox = SandboxId::new("netavark-conflict");
    let conflict = reserve_complete_launch(
        &manager,
        &conflict_tenant,
        &conflict_sandbox,
        &bindings,
        &[],
    )
    .expect_err("unknown provider absence must keep the exact host port fenced");
    assert!(
        conflict.to_string().contains("CleanupPending"),
        "conflict must name the durable cleanup-pending fence: {conflict}"
    );

    manager
        .retain_ambiguous_netavark_cleanup(&recovery_registry, &tenant, &sandbox, Some(cleanup))
        .expect("ambiguous provider inspection should retain durable quarantine");
    assert_eq!(
        authority
            .inspect(reserved.published_leases[0].lease_id())
            .expect("quarantined listener should inspect")
            .expect("quarantined listener should remain durable")
            .phase(),
        PortLeasePhase::CleanupPending
    );

    let retry = manager
        .begin_netavark_cleanup(
            &recovery_registry,
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("retry should reacquire the same dead-owner generation")
        .expect("quarantined provider evidence should still require cleanup");
    manager
        .complete_netavark_cleanup(&reserved.published_leases, Some(&retry), true)
        .expect("exact provider absence should atomically release the batch");
    let released = authority
        .inspect(reserved.published_leases[0].lease_id())
        .expect("released listener should inspect")
        .expect("released listener should remain auditable");
    assert_eq!(released.phase(), PortLeasePhase::Released);
    assert_eq!(released.binding(), Some(&expected_binding));
    assert!(
        released.adoption_claim().is_some(),
        "terminal provider evidence must retain its exact attempt identity"
    );

    reserve_complete_launch(
        &manager,
        &conflict_tenant,
        &conflict_sandbox,
        &bindings,
        &[],
    )
    .expect("the exact host port may be reused only after authenticated absence");
}
