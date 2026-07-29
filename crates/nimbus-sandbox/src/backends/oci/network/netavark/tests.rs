use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use nimbus_core::TenantId;
use nimbus_network::LocalNetworkStateStore;
use serde_json::Value;
use tempfile::tempdir;

use super::super::ipam::{
    allocate_container_ips, begin_netavark_setup, deallocate_container_ips_after_confirmed_detach,
};
use super::super::layout::{OciNetworkConfig, OciNetworkLayout};
use super::*;

const BARRIER_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn reserved_teardown_with_precreated_namespace_never_calls_netavark_or_rewrites_ipam() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant =
        TenantId::new("tenant-netavark-reserved-no-effect").expect("tenant should validate");
    let sandbox = SandboxId::new("netavark-reserved-no-effect");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    std::fs::write(&layout.netns_path, b"nimbus-owned-namespace")
        .expect("separate namespace effect should exist");
    let authority_path = LocalNetworkStateStore::authority_path_for(temp_dir.path());
    let before = std::fs::read(&authority_path).expect("network authority bytes should read");
    let calls = AtomicUsize::new(0);

    teardown_container_network_with_runner(&layout, &config, &sandbox, |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Null)
    })
    .expect("reserved Netavark generation should converge as provider no-effect");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "namespace existence alone must never authorize a Netavark teardown effect"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("network authority bytes should reread"),
        before,
        "no-effect teardown must leave the exact Reserved generation unchanged"
    );
    assert_eq!(
        std::fs::read(&layout.netns_path).expect("namespace sentinel should remain"),
        b"nimbus-owned-namespace",
        "Netavark no-effect classification must not absorb namespace ownership"
    );
}

#[test]
fn reserved_teardown_rejects_status_projection_without_provider_effect() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant =
        TenantId::new("tenant-netavark-reserved-status-conflict").expect("tenant should validate");
    let sandbox = SandboxId::new("netavark-reserved-status-conflict");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    std::fs::write(&layout.status_path, b"contradictory-status")
        .expect("contradictory projection should exist");
    let authority_path = LocalNetworkStateStore::authority_path_for(temp_dir.path());
    let before = std::fs::read(&authority_path).expect("network authority bytes should read");
    let calls = AtomicUsize::new(0);

    let error = teardown_container_network_with_runner(&layout, &config, &sandbox, |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Null)
    })
    .expect_err("a status projection must contradict durable Reserved no-effect authority");

    assert!(
        error.to_string().contains("status projection"),
        "the conflict must name the unowned observed projection: {error}"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "contradictory observed state must not manufacture provider authority"
    );
    assert_eq!(
        std::fs::read(&authority_path).expect("network authority bytes should reread"),
        before,
        "conflict handling must preserve the durable Reserved generation"
    );
    assert_eq!(
        std::fs::read(&layout.status_path).expect("status sentinel should remain"),
        b"contradictory-status",
        "unowned observed evidence must remain available for reconciliation"
    );
}

#[test]
fn setup_operation_claim_blocks_release_and_replacement_during_provider_effect() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant = TenantId::new("tenant-netavark-setup-interleaving")
        .expect("tenant identity should validate");
    let sandbox = SandboxId::new("netavark-setup-interleaving");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let current = OciNetworkConfig::default();
    allocate_container_ips(&layout, &current, &sandbox)
        .expect("current generation should reserve IPAM");

    let (entered_sender, entered_receiver) = mpsc::sync_channel(0);
    let (release_sender, release_receiver) = mpsc::sync_channel(0);
    let worker_layout = layout.clone();
    let worker_config = current.clone();
    let worker_sandbox = sandbox.clone();
    let worker = thread::spawn(move || {
        setup_container_network_with_runner(
            &worker_layout,
            &worker_config,
            &worker_sandbox,
            |action, _assigned_ips| {
                assert_eq!(action, "setup");
                entered_sender
                    .send(())
                    .expect("setup runner should announce entry");
                release_receiver
                    .recv_timeout(BARRIER_TIMEOUT)
                    .expect("setup runner release must remain bounded");
                Ok(Value::Null)
            },
        )
    });
    entered_receiver
        .recv_timeout(BARRIER_TIMEOUT)
        .expect("setup must reach the post-claim provider boundary");

    let release_error = deallocate_container_ips_after_confirmed_detach(
        &layout,
        &sandbox,
        &current.reservation_claim,
    )
    .expect_err("an in-flight setup claim must fence IPAM release");
    assert!(
        release_error
            .to_string()
            .contains("Netavark provider operation remains provisioning"),
        "release rejection must name the exact provider phase: {release_error}"
    );
    let mut replacement = current.clone();
    replacement.reservation_claim =
        crate::backends::oci::port_lease::new_launch_reservation_claim()
            .expect("replacement claim should mint");
    let replacement_error = allocate_container_ips(&layout, &replacement, &sandbox)
        .expect_err("replacement generation must remain fenced during setup");
    assert!(
        replacement_error
            .to_string()
            .contains("different launch coordinator"),
        "replacement rejection must retain the generation diagnostic: {replacement_error}"
    );

    release_sender
        .send(())
        .expect("setup runner should be released");
    let assigned = worker
        .join()
        .expect("setup worker should not panic")
        .expect("current setup should complete");
    assert_eq!(assigned.len(), 1);
}

#[test]
fn teardown_operation_claim_blocks_release_and_replacement_during_provider_effect() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant = TenantId::new("tenant-netavark-teardown-interleaving")
        .expect("tenant identity should validate");
    let sandbox = SandboxId::new("netavark-teardown-interleaving");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let current = OciNetworkConfig::default();
    allocate_container_ips(&layout, &current, &sandbox)
        .expect("current generation should reserve IPAM");
    setup_container_network_with_runner(&layout, &current, &sandbox, |_, _| Ok(Value::Null))
        .expect("fixture setup should complete");
    std::fs::write(&layout.netns_path, b"current-netns")
        .expect("provider netns marker should exist");

    let (entered_sender, entered_receiver) = mpsc::sync_channel(0);
    let (release_sender, release_receiver) = mpsc::sync_channel(0);
    let worker_layout = layout.clone();
    let worker_config = current.clone();
    let worker_sandbox = sandbox.clone();
    let worker = thread::spawn(move || {
        teardown_container_network_with_runner(
            &worker_layout,
            &worker_config,
            &worker_sandbox,
            |action, _assigned_ips| {
                assert_eq!(action, "teardown");
                entered_sender
                    .send(())
                    .expect("teardown runner should announce entry");
                release_receiver
                    .recv_timeout(BARRIER_TIMEOUT)
                    .expect("teardown runner release must remain bounded");
                Ok(Value::Null)
            },
        )
    });
    entered_receiver
        .recv_timeout(BARRIER_TIMEOUT)
        .expect("teardown must reach the post-claim provider boundary");

    let release_error = deallocate_container_ips_after_confirmed_detach(
        &layout,
        &sandbox,
        &current.reservation_claim,
    )
    .expect_err("an in-flight teardown claim must fence IPAM release");
    assert!(
        release_error
            .to_string()
            .contains("Netavark provider operation remains deleting"),
        "release rejection must name the exact provider phase: {release_error}"
    );
    let mut replacement = current.clone();
    replacement.reservation_claim =
        crate::backends::oci::port_lease::new_launch_reservation_claim()
            .expect("replacement claim should mint");
    allocate_container_ips(&layout, &replacement, &sandbox)
        .expect_err("replacement generation must remain fenced during teardown");

    release_sender
        .send(())
        .expect("teardown runner should be released");
    worker
        .join()
        .expect("teardown worker should not panic")
        .expect("current teardown should complete");
    deallocate_container_ips_after_confirmed_detach(&layout, &sandbox, &current.reservation_claim)
        .expect("confirmed current detach should release IPAM");
    allocate_container_ips(&layout, &replacement, &sandbox)
        .expect("replacement may reserve only after current detach completion");
    std::fs::write(&layout.status_path, b"replacement-status")
        .expect("replacement projection should create");
    assert_eq!(
        std::fs::read(&layout.status_path).expect("replacement projection should remain"),
        b"replacement-status"
    );
}

#[test]
fn reopened_pending_setup_fails_closed_without_rerunning_provider() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant = TenantId::new("tenant-netavark-reopen").expect("tenant identity should validate");
    let sandbox = SandboxId::new("netavark-reopen");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    let _abandoned =
        begin_netavark_setup(&layout, &config, &sandbox).expect("setup claim should persist");

    let provider_ran = Arc::new(AtomicBool::new(false));
    let observed_provider_ran = Arc::clone(&provider_ran);
    let error = setup_container_network_with_runner(&layout, &config, &sandbox, move |_, _| {
        observed_provider_ran.store(true, Ordering::SeqCst);
        Ok(Value::Null)
    })
    .expect_err("a reopened pending claim must require inspect-before-retry");
    assert!(
        error.to_string().contains("inspect-before-retry")
            && error.to_string().contains("provisioning"),
        "reopen rejection must retain exact recovery guidance: {error}"
    );
    assert!(
        !provider_ran.load(Ordering::SeqCst),
        "pending durable authority must reject before invoking the provider"
    );
    let release_error = deallocate_container_ips_after_confirmed_detach(
        &layout,
        &sandbox,
        &config.reservation_claim,
    )
    .expect_err("pending setup must survive reopen and fence replacement");
    assert!(
        release_error
            .to_string()
            .contains("Netavark provider operation remains provisioning"),
        "durable pending state must be visible to release: {release_error}"
    );
}

#[test]
fn projection_retry_does_not_rerun_confirmed_netavark_teardown() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant =
        TenantId::new("tenant-netavark-projection-retry").expect("tenant identity should validate");
    let sandbox = SandboxId::new("netavark-projection-retry");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    setup_container_network_with_runner(&layout, &config, &sandbox, |action, _assigned_ips| {
        assert_eq!(action, "setup");
        Ok(Value::Null)
    })
    .expect("fixture setup should publish Ready provider authority");
    std::fs::write(&layout.netns_path, b"current-netns")
        .expect("provider netns marker should exist");
    std::fs::remove_file(&layout.status_path).expect("setup projection should remove");
    std::fs::create_dir(&layout.status_path)
        .expect("directory-shaped projection should make file removal fail");

    let teardown_calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&teardown_calls);
    let error =
        teardown_container_network_with_runner(&layout, &config, &sandbox, move |action, _| {
            assert_eq!(action, "teardown");
            observed_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        })
        .expect_err("projection removal failure must retain completion authority");
    assert!(
        error
            .to_string()
            .contains("after provider absence was recorded"),
        "failure must distinguish durable provider absence from pending projection cleanup: \
         {error}"
    );
    assert_eq!(
        teardown_calls.load(Ordering::SeqCst),
        1,
        "the first teardown must execute the provider exactly once"
    );
    deallocate_container_ips_after_confirmed_detach(&layout, &sandbox, &config.reservation_claim)
        .expect_err("projection-pending teardown must keep IPAM fenced");

    std::fs::remove_dir(&layout.status_path).expect("projection obstacle should remove");
    teardown_container_network_with_runner(&layout, &config, &sandbox, |_, _| {
        panic!("projection-only retry must not rerun the Netavark provider")
    })
    .expect("projection-only retry should complete the durable teardown");
    assert_eq!(
        teardown_calls.load(Ordering::SeqCst),
        1,
        "projection retry must preserve the one acknowledged provider effect"
    );
    deallocate_container_ips_after_confirmed_detach(&layout, &sandbox, &config.reservation_claim)
        .expect("completed projection cleanup may release exact IPAM authority");
}
