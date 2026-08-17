//! Netavark process-lifetime and cleanup-pending adapter proofs.

use super::super::NetavarkPortLifetimeRegistry;
use super::*;
use crate::backends::oci::port_lease::OciPortBindLifetimeBatch;
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

#[test]
fn active_listener_readiness_requires_the_exact_retained_batch_and_handles_empty_explicitly() {
    let temp_dir = TempDir::new().expect("temporary state root should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_100..=15_100);
    let tenant = tenant_id("tenant-netavark-readiness");
    let sandbox = SandboxId::new("netavark-readiness");
    let bindings = [SandboxPortBinding::tcp("http", 15_100, 8080)];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox, &bindings, &[])
        .expect("published listener should reserve");
    let batch = manager
        .claim_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("Netavark should claim under exact lifetimes");
    manager
        .activate_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            &batch,
        )
        .expect("Netavark should activate under exact lifetimes");
    let registry = NetavarkPortLifetimeRegistry::default();
    registry
        .insert(&tenant, &sandbox, batch)
        .map_err(|(error, _batch)| error)
        .expect("live batch should register");

    manager
        .inspect_active_netavark_bindings_with_lifetimes(
            &registry,
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("the exact active listener and retained lifetime should be ready");
    manager
        .inspect_active_netavark_bindings_with_lifetimes(
            &NetavarkPortLifetimeRegistry::default(),
            &tenant,
            &SandboxId::new("empty-publication"),
            &[],
            &[],
        )
        .expect("an explicit empty publication set needs no lifetime batch");

    let authority = LocalPortLeaseAuthority::open(temp_dir.path()).expect("authority should open");
    let before = authority.list().expect("authority should list");
    let missing = manager
        .inspect_active_netavark_bindings_with_lifetimes(
            &NetavarkPortLifetimeRegistry::default(),
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect_err("a non-empty publication without its retained lifetime must fail");
    assert!(
        missing.to_string().contains("no retained exact"),
        "missing lifetime diagnosis should be stable: {missing}"
    );
    let wrong_bindings = [SandboxPortBinding::tcp("http", 15_100, 9090)];
    manager
        .inspect_active_netavark_bindings_with_lifetimes(
            &registry,
            &tenant,
            &sandbox,
            &wrong_bindings,
            &reserved.published_leases,
        )
        .expect_err("a substituted guest binding must fail");
    assert_eq!(
        authority.list().expect("authority should relist"),
        before,
        "readiness inspection must not mutate durable listener authority"
    );
}

#[test]
fn empty_listener_reconciliation_rejects_a_conflicting_retained_batch() {
    let temp_dir = TempDir::new().expect("temporary state root should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_150..=15_150);
    let tenant = tenant_id("tenant-empty-netavark-reconcile");
    let sandbox = SandboxId::new("empty-netavark-reconcile");
    let registry = NetavarkPortLifetimeRegistry::default();
    let conflicting = OciPortBindLifetimeBatch::from_reclaimed(Vec::new(), Vec::new())
        .expect("empty conflicting batch should construct");
    registry
        .insert(&tenant, &sandbox, conflicting)
        .map_err(|(error, _batch)| error)
        .expect("conflicting empty batch should register");

    let error = manager
        .reconcile_active_netavark_bindings_with_lifetimes(&registry, &tenant, &sandbox, &[], &[])
        .expect_err("empty desired publication must not suppress conflicting retained evidence");
    assert!(
        error
            .to_string()
            .contains("retains a process-lifetime batch"),
        "conflict diagnosis should remain exact: {error}"
    );
    assert!(
        registry
            .take(&tenant, &sandbox)
            .expect("registry should remain inspectable")
            .is_some(),
        "failed reconciliation must preserve the conflicting batch"
    );
}

#[test]
fn dead_owner_reconciliation_reclaims_once_at_a_higher_lifetime_generation() {
    let temp_dir = TempDir::new().expect("temporary state root should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_200..=15_200);
    let tenant = tenant_id("tenant-netavark-reclaim");
    let sandbox = SandboxId::new("netavark-reclaim");
    let bindings = [SandboxPortBinding::tcp("http", 15_200, 8080)];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox, &bindings, &[])
        .expect("published listener should reserve");
    let batch = manager
        .claim_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("Netavark should claim under exact lifetimes");
    manager
        .activate_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            &batch,
        )
        .expect("Netavark should activate under exact lifetimes");
    let first_generation = batch.lifetimes()[0].lifetime().generation().as_u64();
    drop(batch);

    let recovered_registry = NetavarkPortLifetimeRegistry::default();
    manager
        .reconcile_active_netavark_bindings_with_lifetimes(
            &recovered_registry,
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("dead owner with exact provider presence should be reclaimable");
    manager
        .inspect_active_netavark_bindings_with_lifetimes(
            &recovered_registry,
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("the reclaimed lifetime should immediately satisfy readiness");

    let authority = LocalPortLeaseAuthority::open(temp_dir.path()).expect("authority should open");
    let reclaimed = authority
        .inspect(reserved.published_leases[0].lease_id())
        .expect("reclaimed listener should inspect")
        .expect("reclaimed listener should remain durable");
    let reclaimed_generation = reclaimed
        .active_lifetime()
        .expect("reclaimed listener should retain active lifetime")
        .generation()
        .as_u64();
    assert!(
        reclaimed_generation > first_generation,
        "owner-death recovery must fence the former lifetime generation"
    );

    manager
        .reconcile_active_netavark_bindings_with_lifetimes(
            &recovered_registry,
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("same-process reconciliation should be idempotent");
    assert_eq!(
        authority
            .inspect(reserved.published_leases[0].lease_id())
            .expect("listener should re-inspect")
            .expect("listener should remain durable"),
        reclaimed,
        "idempotent reconciliation must not advance lifetime generation again"
    );
}

#[test]
fn live_owner_or_substituted_reconciliation_fails_before_durable_mutation() {
    let temp_dir = TempDir::new().expect("temporary state root should exist");
    let manager = OciPortLeaseCoordinator::new(temp_dir.path(), 15_300..=15_300);
    let tenant = tenant_id("tenant-netavark-reclaim-fence");
    let sandbox = SandboxId::new("netavark-reclaim-fence");
    let bindings = [SandboxPortBinding::tcp("http", 15_300, 8080)];
    let reserved = reserve_complete_launch(&manager, &tenant, &sandbox, &bindings, &[])
        .expect("published listener should reserve");
    let batch = manager
        .claim_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect("Netavark should claim under exact lifetimes");
    manager
        .activate_netavark_bindings_with_lifetimes(
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
            &batch,
        )
        .expect("Netavark should activate under exact lifetimes");
    let live_registry = NetavarkPortLifetimeRegistry::default();
    live_registry
        .insert(&tenant, &sandbox, batch)
        .map_err(|(error, _batch)| error)
        .expect("live owner should retain its lifetime");

    let authority = LocalPortLeaseAuthority::open(temp_dir.path()).expect("authority should open");
    let before = authority.list().expect("authority should list");
    let foreign_registry = NetavarkPortLifetimeRegistry::default();
    let live_error = manager
        .reconcile_active_netavark_bindings_with_lifetimes(
            &foreign_registry,
            &tenant,
            &sandbox,
            &bindings,
            &reserved.published_leases,
        )
        .expect_err("a still-live owner must fence foreign reconciliation");
    assert!(
        live_error.to_string().contains("live process lifetime"),
        "live-owner rejection should name the missing death proof: {live_error}"
    );

    let wrong_bindings = [SandboxPortBinding::tcp("http", 15_300, 9090)];
    manager
        .reconcile_active_netavark_bindings_with_lifetimes(
            &foreign_registry,
            &tenant,
            &sandbox,
            &wrong_bindings,
            &reserved.published_leases,
        )
        .expect_err("substituted desired binding must fail before recovery");
    assert_eq!(
        authority.list().expect("authority should relist"),
        before,
        "live-owner and substituted recovery failures must preserve durable bytes"
    );

    assert!(
        live_registry
            .take(&tenant, &sandbox)
            .expect("live registry should inspect")
            .is_some(),
        "failed foreign reconciliation must not consume the real live batch"
    );
}
