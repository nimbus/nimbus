use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use nimbus_core::TenantId;
use nimbus_network::{LocalNetworkStateStore, NetworkProviderHandle, NetworkProviderId};
use serde_json::Value;
use tempfile::tempdir;

use super::super::ipam::{
    allocate_container_ips, begin_netavark_setup, deallocate_container_ips_after_confirmed_detach,
    inspect_netavark_provider_operation,
};
use super::super::layout::{OciNetworkConfig, OciNetworkLayout};
use super::*;
use crate::backends::oci::network::direct_test_ipam_authority;

const BARRIER_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(unix)]
#[test]
fn successful_provider_exit_wins_the_stdin_broken_pipe_race() {
    let mut command = std::process::Command::new("/usr/bin/true");
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = run_netavark_command(&mut command, &vec![b'x'; 1024 * 1024])
        .expect("an exit-zero provider should settle a concurrent stdin close");

    assert!(output.status.success());
}

#[cfg(unix)]
#[test]
fn failed_provider_exit_remains_failed_after_the_stdin_broken_pipe_race() {
    let mut command = std::process::Command::new("/usr/bin/false");
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let output = run_netavark_command(&mut command, &vec![b'x'; 1024 * 1024])
        .expect("a concurrent stdin close should preserve the provider exit result");

    assert!(!output.status.success());
}

fn foreign_provider_attempt(label: &str) -> NetworkProviderHandle {
    NetworkProviderHandle::new(
        NetworkProviderId::for_registration_key("nimbus-sandbox.netavark-substitution-test"),
        format!("attempt:{label}"),
    )
    .expect("foreign provider attempt should validate")
}

#[test]
fn reserved_teardown_with_precreated_namespace_never_calls_netavark_or_rewrites_ipam() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant =
        TenantId::new("tenant-netavark-reserved-no-effect").expect("tenant should validate");
    let sandbox = SandboxId::new("netavark-reserved-no-effect");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&ipam_authority, &layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    std::fs::write(&layout.netns_path, b"nimbus-owned-namespace")
        .expect("separate namespace effect should exist");
    let authority_path = LocalNetworkStateStore::authority_path_for(temp_dir.path());
    let before = std::fs::read(&authority_path).expect("network authority bytes should read");
    let calls = AtomicUsize::new(0);

    teardown_container_network_with_runner(&ipam_authority, &layout, &config, &sandbox, |_, _| {
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
fn prepared_setup_cleanup_never_calls_netavark_even_when_namespace_exists() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant =
        TenantId::new("tenant-netavark-prepared-setup-no-effect").expect("tenant should validate");
    let sandbox = SandboxId::new("netavark-prepared-setup-no-effect");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&ipam_authority, &layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    begin_netavark_setup(&ipam_authority, &layout, &config, &sandbox)
        .expect("setup attempt should become durable");
    std::fs::write(&layout.netns_path, b"namespace-created-before-provider")
        .expect("namespace checkpoint should exist");
    let calls = AtomicUsize::new(0);

    teardown_container_network_with_runner(&ipam_authority, &layout, &config, &sandbox, |_, _| {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Null)
    })
    .expect("prepared-only setup must converge without a provider delete");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "namespace presence must not manufacture a Netavark effect after prepared-only setup"
    );
    assert!(matches!(
        inspect_netavark_provider_operation(&ipam_authority, &layout, &config, &sandbox)
            .expect("no-effect teardown should inspect"),
        super::super::dto::NetavarkProviderOperation::Detached
    ));
    assert_eq!(
        std::fs::read(&layout.netns_path).expect("namespace remains separately owned"),
        b"namespace-created-before-provider",
        "provider no-effect confirmation must not absorb namespace ownership"
    );
}

#[test]
fn reserved_teardown_rejects_status_projection_without_provider_effect() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant =
        TenantId::new("tenant-netavark-reserved-status-conflict").expect("tenant should validate");
    let sandbox = SandboxId::new("netavark-reserved-status-conflict");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&ipam_authority, &layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    std::fs::write(&layout.status_path, b"contradictory-status")
        .expect("contradictory projection should exist");
    let authority_path = LocalNetworkStateStore::authority_path_for(temp_dir.path());
    let before = std::fs::read(&authority_path).expect("network authority bytes should read");
    let calls = AtomicUsize::new(0);

    let error = teardown_container_network_with_runner(
        &ipam_authority,
        &layout,
        &config,
        &sandbox,
        |_, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        },
    )
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
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let current = OciNetworkConfig::default();
    allocate_container_ips(&ipam_authority, &layout, &current, &sandbox)
        .expect("current generation should reserve IPAM");

    let (entered_sender, entered_receiver) = mpsc::sync_channel(0);
    let (release_sender, release_receiver) = mpsc::sync_channel(0);
    let worker_layout = layout.clone();
    let worker_config = current.clone();
    let worker_sandbox = sandbox.clone();
    let worker_ipam_authority = ipam_authority.clone();
    let worker = thread::spawn(move || {
        setup_container_network_with_runner(
            &worker_ipam_authority,
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
        &ipam_authority,
        &layout,
        &sandbox,
        &current.attachment_id,
        &current.reservation_claim,
        current.provider_kind(),
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
    let replacement_error =
        allocate_container_ips(&ipam_authority, &layout, &replacement, &sandbox)
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
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let current = OciNetworkConfig::default();
    allocate_container_ips(&ipam_authority, &layout, &current, &sandbox)
        .expect("current generation should reserve IPAM");
    setup_container_network_with_runner(&ipam_authority, &layout, &current, &sandbox, |_, _| {
        Ok(Value::Null)
    })
    .expect("fixture setup should complete");
    std::fs::write(&layout.netns_path, b"current-netns")
        .expect("provider netns marker should exist");

    let (entered_sender, entered_receiver) = mpsc::sync_channel(0);
    let (release_sender, release_receiver) = mpsc::sync_channel(0);
    let worker_layout = layout.clone();
    let worker_config = current.clone();
    let worker_sandbox = sandbox.clone();
    let worker_ipam_authority = ipam_authority.clone();
    let worker = thread::spawn(move || {
        teardown_container_network_with_runner(
            &worker_ipam_authority,
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
        &ipam_authority,
        &layout,
        &sandbox,
        &current.attachment_id,
        &current.reservation_claim,
        current.provider_kind(),
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
    allocate_container_ips(&ipam_authority, &layout, &replacement, &sandbox)
        .expect_err("replacement generation must remain fenced during teardown");

    release_sender
        .send(())
        .expect("teardown runner should be released");
    worker
        .join()
        .expect("teardown worker should not panic")
        .expect("current teardown should complete");
    deallocate_container_ips_after_confirmed_detach(
        &ipam_authority,
        &layout,
        &sandbox,
        &current.attachment_id,
        &current.reservation_claim,
        current.provider_kind(),
    )
    .expect("confirmed current detach should release IPAM");
    allocate_container_ips(&ipam_authority, &layout, &replacement, &sandbox)
        .expect("replacement may reserve only after current detach completion");
    std::fs::write(&layout.status_path, b"replacement-status")
        .expect("replacement projection should create");
    assert_eq!(
        std::fs::read(&layout.status_path).expect("replacement projection should remain"),
        b"replacement-status"
    );
}

#[test]
fn reopened_prepared_setup_reuses_exact_attempt_once() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant = TenantId::new("tenant-netavark-reopen").expect("tenant identity should validate");
    let sandbox = SandboxId::new("netavark-reopen");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&ipam_authority, &layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    let (_, abandoned) = begin_netavark_setup(&ipam_authority, &layout, &config, &sandbox)
        .expect("setup claim should persist");

    let provider_calls = Arc::new(AtomicUsize::new(0));
    let observed_provider_calls = Arc::clone(&provider_calls);
    setup_container_network_with_runner(
        &ipam_authority,
        &layout,
        &config,
        &sandbox,
        move |_, _| {
            observed_provider_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        },
    )
    .expect("a fresh owner should resume the exact prepared setup");
    assert_eq!(
        provider_calls.load(Ordering::SeqCst),
        1,
        "the retained attempt must execute exactly once"
    );
    assert!(matches!(
        inspect_netavark_provider_operation(&ipam_authority, &layout, &config, &sandbox)
            .expect("ready setup should inspect"),
        super::super::dto::NetavarkProviderOperation::Ready { setup_attempt }
            if &setup_attempt == abandoned.operation_attempt()
    ));
    let release_error = deallocate_container_ips_after_confirmed_detach(
        &ipam_authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &config.reservation_claim,
        config.provider_kind(),
    )
    .expect_err("ready setup must continue to fence replacement before detach");
    assert!(
        release_error
            .to_string()
            .contains("Netavark provider operation remains ready"),
        "durable ready state must be visible to release: {release_error}"
    );
}

#[test]
fn substituted_setup_and_teardown_attempts_fail_before_provider_effect() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant =
        TenantId::new("tenant-netavark-attempt-substitution").expect("tenant should validate");
    let sandbox = SandboxId::new("netavark-attempt-substitution");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&ipam_authority, &layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    let (assigned_ips, setup_claim) =
        begin_netavark_setup(&ipam_authority, &layout, &config, &sandbox)
            .expect("setup attempt should prepare");
    assert!(matches!(
        inspect_netavark_provider_operation(&ipam_authority, &layout, &config, &sandbox)
            .expect("prepared setup should inspect"),
        super::super::dto::NetavarkProviderOperation::SetupPrepared {
            operation_attempt
        } if &operation_attempt == setup_claim.operation_attempt()
    ));
    let authority_path = LocalNetworkStateStore::authority_path_for(temp_dir.path());
    let setup_bytes = std::fs::read(&authority_path).expect("setup authority should read");
    let setup_calls = AtomicUsize::new(0);
    let substituted_setup = PreparedNetavarkSetup {
        assigned_ips: assigned_ips.clone(),
        claim: setup_claim
            .with_operation_attempt_for_test(foreign_provider_attempt("foreign-setup")),
    };
    let setup_error = execute_prepared_container_network_setup_with_runner(
        &ipam_authority,
        &layout,
        &config,
        &sandbox,
        substituted_setup,
        |_, _| {
            setup_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        },
    )
    .expect_err("a substituted setup capability must fail before the provider");
    assert!(
        setup_error.to_string().contains("does not own"),
        "setup rejection must name the exact operation capability: {setup_error}"
    );
    assert_eq!(setup_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        std::fs::read(&authority_path).expect("setup authority should reread"),
        setup_bytes,
        "substituted setup must preserve exact authority bytes"
    );

    execute_prepared_container_network_setup_with_runner(
        &ipam_authority,
        &layout,
        &config,
        &sandbox,
        PreparedNetavarkSetup {
            assigned_ips,
            claim: setup_claim.clone(),
        },
        |action, _| {
            assert_eq!(action, "setup");
            setup_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        },
    )
    .expect("the exact setup capability should execute");
    assert_eq!(setup_calls.load(Ordering::SeqCst), 1);

    std::fs::write(&layout.netns_path, b"provider-present")
        .expect("provider namespace marker should exist");
    let (delete_ips, delete_claim) =
        match begin_netavark_teardown(&ipam_authority, &layout, &config, &sandbox, None)
            .expect("delete attempt should prepare")
        {
            NetavarkTeardownPlan::Run {
                assigned_ips,
                claim,
            } => (assigned_ips, claim),
            _ => panic!("ready provider authority must prepare one delete"),
        };
    let delete_bytes = std::fs::read(&authority_path).expect("delete authority should read");
    let delete_calls = AtomicUsize::new(0);
    let substituted_delete = NetavarkTeardownPlan::Run {
        assigned_ips: delete_ips.clone(),
        claim: delete_claim
            .with_operation_attempt_for_test(foreign_provider_attempt("foreign-delete")),
    };
    let delete_error =
        execute_teardown_plan(&ipam_authority, &layout, substituted_delete, &mut |_, _| {
            delete_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        })
        .expect_err("a substituted delete capability must fail before the provider");
    assert!(
        delete_error.to_string().contains("does not own"),
        "delete rejection must name the exact operation capability: {delete_error}"
    );
    assert_eq!(delete_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        std::fs::read(&authority_path).expect("delete authority should reread"),
        delete_bytes,
        "substituted delete must preserve exact authority bytes"
    );

    execute_teardown_plan(
        &ipam_authority,
        &layout,
        NetavarkTeardownPlan::Run {
            assigned_ips: delete_ips,
            claim: delete_claim,
        },
        &mut |action, _| {
            assert_eq!(action, "teardown");
            delete_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        },
    )
    .expect("the exact delete capability should execute");
    assert_eq!(delete_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn projection_retry_does_not_rerun_confirmed_netavark_teardown() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant =
        TenantId::new("tenant-netavark-projection-retry").expect("tenant identity should validate");
    let sandbox = SandboxId::new("netavark-projection-retry");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&ipam_authority, &layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    setup_container_network_with_runner(
        &ipam_authority,
        &layout,
        &config,
        &sandbox,
        |action, _assigned_ips| {
            assert_eq!(action, "setup");
            Ok(Value::Null)
        },
    )
    .expect("fixture setup should publish Ready provider authority");
    std::fs::write(&layout.netns_path, b"current-netns")
        .expect("provider netns marker should exist");
    std::fs::remove_file(&layout.status_path).expect("setup projection should remove");
    std::fs::create_dir(&layout.status_path)
        .expect("directory-shaped projection should make file removal fail");

    let teardown_calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&teardown_calls);
    let error = teardown_container_network_with_runner(
        &ipam_authority,
        &layout,
        &config,
        &sandbox,
        move |action, _| {
            assert_eq!(action, "teardown");
            observed_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        },
    )
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
    deallocate_container_ips_after_confirmed_detach(
        &ipam_authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &config.reservation_claim,
        config.provider_kind(),
    )
    .expect_err("projection-pending teardown must keep IPAM fenced");

    std::fs::remove_dir(&layout.status_path).expect("projection obstacle should remove");
    teardown_container_network_with_runner(&ipam_authority, &layout, &config, &sandbox, |_, _| {
        panic!("projection-only retry must not rerun the Netavark provider")
    })
    .expect("projection-only retry should complete the durable teardown");
    assert_eq!(
        teardown_calls.load(Ordering::SeqCst),
        1,
        "projection retry must preserve the one acknowledged provider effect"
    );
    deallocate_container_ips_after_confirmed_detach(
        &ipam_authority,
        &layout,
        &sandbox,
        &config.attachment_id,
        &config.reservation_claim,
        config.provider_kind(),
    )
    .expect("completed projection cleanup may release exact IPAM authority");
}

#[test]
fn deleting_recovery_confirms_exact_interface_absence_without_rerunning_provider() {
    let temp_dir = tempdir().expect("temporary directory should create");
    let tenant = TenantId::new("tenant-netavark-delete-interface-absent")
        .expect("tenant identity should validate");
    let sandbox = SandboxId::new("netavark-delete-interface-absent");
    let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
    let ipam_authority = direct_test_ipam_authority(&layout);
    layout
        .ensure_directories()
        .expect("network layout should create");
    let config = OciNetworkConfig::default();
    allocate_container_ips(&ipam_authority, &layout, &config, &sandbox)
        .expect("current generation should reserve IPAM");
    setup_container_network_with_runner(
        &ipam_authority,
        &layout,
        &config,
        &sandbox,
        |action, _| {
            assert_eq!(action, "setup");
            Ok(Value::Null)
        },
    )
    .expect("fixture setup should publish exact provider authority");
    std::fs::write(&layout.netns_path, b"namespace-without-provider-interface")
        .expect("separately owned namespace should remain present");

    let claim = match begin_netavark_teardown(&ipam_authority, &layout, &config, &sandbox, None)
        .expect("teardown should prepare")
    {
        NetavarkTeardownPlan::Run { claim, .. } => claim,
        _ => panic!("ready provider authority must prepare exact delete"),
    };
    begin_netavark_teardown_execution(&ipam_authority, &layout, &claim)
        .expect("lost-response fixture should cross the pre-effect fence");
    let recovered = begin_netavark_teardown(&ipam_authority, &layout, &config, &sandbox, None)
        .expect("fresh owner should recover exact deleting authority");
    assert!(matches!(
        recovered,
        NetavarkTeardownPlan::InspectDeleting { .. }
    ));

    let provider_calls = AtomicUsize::new(0);
    let inspection_calls = AtomicUsize::new(0);
    execute_teardown_plan_with_inspector(
        &ipam_authority,
        &layout,
        recovered,
        &mut |_, _| {
            provider_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        },
        &mut |path| {
            assert_eq!(path, layout.netns_path);
            inspection_calls.fetch_add(1, Ordering::SeqCst);
            NetavarkLinkObservation::Absent
        },
    )
    .expect("exact interface absence should complete the ambiguous delete");

    assert_eq!(provider_calls.load(Ordering::SeqCst), 0);
    assert_eq!(inspection_calls.load(Ordering::SeqCst), 1);
    assert!(
        layout.netns_path.exists(),
        "Netavark recovery must not absorb separately owned namespace removal"
    );
    assert!(
        !layout.status_path.exists(),
        "confirmed provider absence must remove only its observed projection"
    );
    assert!(matches!(
        inspect_netavark_provider_operation(&ipam_authority, &layout, &config, &sandbox)
            .expect("terminal provider operation should inspect"),
        super::super::dto::NetavarkProviderOperation::Detached
    ));
}

#[test]
fn deleting_recovery_refuses_present_or_unknown_interface_without_provider_calls() {
    for (label, observation, expected) in [
        (
            "present",
            NetavarkLinkObservation::Present,
            "exact container interface remains present",
        ),
        (
            "unknown",
            NetavarkLinkObservation::Unknown {
                reason: "injected inspection ambiguity".to_owned(),
            },
            "injected inspection ambiguity",
        ),
    ] {
        let temp_dir = tempdir().expect("temporary directory should create");
        let tenant = TenantId::new(format!("tenant-netavark-delete-interface-{label}"))
            .expect("tenant identity should validate");
        let sandbox = SandboxId::new(format!("netavark-delete-interface-{label}"));
        let layout = OciNetworkLayout::under_root(temp_dir.path(), &tenant, &sandbox);
        let ipam_authority = direct_test_ipam_authority(&layout);
        layout
            .ensure_directories()
            .expect("network layout should create");
        let config = OciNetworkConfig::default();
        allocate_container_ips(&ipam_authority, &layout, &config, &sandbox)
            .expect("current generation should reserve IPAM");
        setup_container_network_with_runner(&ipam_authority, &layout, &config, &sandbox, |_, _| {
            Ok(Value::Null)
        })
        .expect("fixture setup should publish exact provider authority");
        std::fs::write(&layout.netns_path, b"namespace-provider-state-uncertain")
            .expect("provider namespace marker should remain present");
        let claim = match begin_netavark_teardown(&ipam_authority, &layout, &config, &sandbox, None)
            .expect("teardown should prepare")
        {
            NetavarkTeardownPlan::Run { claim, .. } => claim,
            _ => panic!("ready provider authority must prepare exact delete"),
        };
        begin_netavark_teardown_execution(&ipam_authority, &layout, &claim)
            .expect("fixture should cross the pre-effect fence");
        let recovered = begin_netavark_teardown(&ipam_authority, &layout, &config, &sandbox, None)
            .expect("fresh owner should recover exact deleting authority");
        let authority_path = LocalNetworkStateStore::authority_path_for(temp_dir.path());
        let before = std::fs::read(&authority_path).expect("deleting authority should read");
        let provider_calls = AtomicUsize::new(0);
        let error = execute_teardown_plan_with_inspector(
            &ipam_authority,
            &layout,
            recovered,
            &mut |_, _| {
                provider_calls.fetch_add(1, Ordering::SeqCst);
                Ok(Value::Null)
            },
            &mut |_| observation.clone(),
        )
        .expect_err("present or ambiguous provider effect must remain fenced");
        assert!(
            error.to_string().contains(expected),
            "failure must name the exact observation: {error}"
        );
        assert_eq!(provider_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::read(&authority_path).expect("fenced authority should reread"),
            before,
            "failed inspection must be byte-stable"
        );
        assert!(layout.status_path.exists());
    }
}

#[test]
fn netavark_link_output_classifier_accepts_only_exact_presence_or_absence() {
    assert_eq!(
        classify_netavark_link_command_output(
            true,
            br#"[{"ifindex":2,"ifname":"eth0"}]"#,
            b"",
            "eth0",
        ),
        NetavarkLinkObservation::Present
    );
    for stderr in [
        b"Device \"eth0\" does not exist.".as_slice(),
        b"Cannot find device \"eth0\"".as_slice(),
    ] {
        assert_eq!(
            classify_netavark_link_command_output(false, b"", stderr, "eth0"),
            NetavarkLinkObservation::Absent
        );
    }
    for (success, stdout, stderr) in [
        (true, br#"[]"#.as_slice(), b"".as_slice()),
        (
            true,
            br#"[{"ifindex":2,"ifname":"eth1"}]"#.as_slice(),
            b"".as_slice(),
        ),
        (true, b"not-json".as_slice(), b"".as_slice()),
        (
            false,
            b"".as_slice(),
            b"nsenter: reassociate failed".as_slice(),
        ),
    ] {
        assert!(matches!(
            classify_netavark_link_command_output(success, stdout, stderr, "eth0"),
            NetavarkLinkObservation::Unknown { .. }
        ));
    }
}
