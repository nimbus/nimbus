//! Execute-mode launch compensation proofs.

use super::support::*;

use nimbus_network::{
    LocalNetworkStateStore, LocalPortLeaseAuthority, NetworkProviderHandle, NetworkProviderId,
    NetworkReservationClaim, NetworkReservationLifetimeAttempt, NetworkStatePartition,
    PortLeasePhase,
};
use std::sync::Arc;

use crate::backends::oci::network::{
    AttachmentAttachAuthority, OciSegmentAllocator, RecordingSegmentAllocator,
    default_network_attachment_id,
};
use crate::backends::oci::port_lease::{OciPortProvider, claim_bind_attempts};
use crate::error::SandboxError;

mod restart_fencing;

fn adopt_launch_network(
    backend: &KrunSandboxBackend,
    manifest: &mut KrunSandboxManifest,
) -> NetworkReservationClaim {
    let claim = manifest
        .require_reserved_claim()
        .expect("fixture should begin with reserved launch authority")
        .clone();
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &default_network_attachment_id(&manifest.handle.id),
            &claim,
        )
        .expect("fixture should adopt its exact attachment");
    manifest.launch_authority = KrunLaunchAuthority::Adopted {
        reservation_claim: claim.clone(),
    };
    claim
}

fn mark_provider_owned(
    backend: &KrunSandboxBackend,
    manifest: &mut KrunSandboxManifest,
) -> NetworkReservationClaim {
    let claim = adopt_launch_network(backend, manifest);
    manifest.launch_authority = KrunLaunchAuthority::ProviderOwned;
    claim
}

fn activate_netavark_with_live_lifetimes(
    backend: &KrunSandboxBackend,
    manifest: &KrunSandboxManifest,
) {
    let port_lease_coordinator = backend.port_lease_coordinator();
    let lifetimes = port_lease_coordinator
        .claim_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("Netavark bind attempt should retain exact live lifetimes");
    port_lease_coordinator
        .activate_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
            &lifetimes,
        )
        .expect("test provider evidence should activate under its exact lifetimes");
    backend
        .netavark_port_lifetimes
        .insert(&manifest.spec.tenant_id, &manifest.handle.id, lifetimes)
        .map_err(|(error, _batch)| error)
        .expect("fixture should retain the active Netavark lifetime batch");
}

#[test]
fn adopted_krun_attachment_cleanup_releases_never_bound_launch_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = KrunSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-adopted-cleanup"),
            None,
            None,
        )
        .expect("launch should reserve attachment, IPAM, publication, and PEP authority")
        .manifest;
    adopt_launch_network(&backend, &mut manifest);
    let mut launch_batch = manifest.port_leases.clone();
    launch_batch.push(
        manifest
            .egress_proxy
            .as_ref()
            .expect("execute launch should reserve its PEP")
            .port_lease
            .clone(),
    );

    backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Final,
        )
        .expect("confirmed provider absence should compensate mixed adopted/reserved authority");

    let authority = LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("authority should reopen");
    for request in &launch_batch {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("released evidence should remain durable");
        assert_eq!(
            record.phase(),
            PortLeasePhase::Released,
            "every never-bound launch port must release only after provider absence"
        );
    }
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&manifest.spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "cleanup must remove IPAM before releasing and finalizing the adopted attachment"
    );
}

#[test]
fn adopted_krun_final_cleanup_retains_claimed_pep_without_process_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf())
        .with_network_state_root(temp_dir.path().join("node-network-state"));
    config.netavark_path = PathBuf::from("/usr/bin/true");
    let backend = KrunSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec_for_tenant("krun-claimed-pep", "api"),
            &SandboxId::new("krun-claimed-pep"),
            None,
            None,
        )
        .expect("launch should reserve attachment and PEP authority")
        .manifest;
    let reservation_claim = adopt_launch_network(&backend, &mut manifest);
    let request = manifest
        .egress_proxy
        .as_ref()
        .expect("execute launch should reserve its PEP")
        .port_lease
        .clone();
    let port_lease_coordinator = backend.port_lease_coordinator();
    let claim = claim_bind_attempts(
        port_lease_coordinator
            .authority()
            .expect("backend must retain its process-derived port authority"),
        std::slice::from_ref(&request),
        OciPortProvider::EgressPep,
        Some(&reservation_claim),
    )
    .expect("fixture should publish the ambiguous PEP bind claim")
    .pop()
    .expect("one PEP claim should be returned");

    let error = backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Final,
        )
        .expect_err("claim-only PEP evidence must not authorize provider absence");

    assert!(
        error.to_string().contains("non-Netavark provider claim"),
        "cleanup must identify the still-ambiguous PEP claim: {error}"
    );
    let record = LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("port authority should reopen")
        .inspect(request.lease_id())
        .expect("PEP lease should inspect")
        .expect("PEP lease must remain durable");
    assert_eq!(record.phase(), PortLeasePhase::Reserved);
    assert_eq!(record.reservation_claim(), Some(&reservation_claim));
    assert_eq!(record.bind_claim(), Some(&claim));
    assert_eq!(
        manifest.launch_authority,
        KrunLaunchAuthority::Adopted { reservation_claim },
        "cleanup must not manufacture Released authority from a bind claim"
    );
}

#[test]
fn netavark_endpoint_effect_requires_complete_current_port_leases() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-netavark-port-authority"),
            None,
            None,
        )
        .expect("execute manifest should reserve the endpoint")
        .manifest;
    assert_eq!(manifest.port_leases.len(), 2);
    manifest.port_leases.clear();

    let error = backend
        .configure_network(
            &manifest,
            AttachmentAttachAuthority::FreshLaunch(
                manifest
                    .reservation_claim()
                    .expect("execute manifest should retain its launch claim"),
            ),
            true,
        )
        .expect_err("provider setup without the complete lease set must fail");
    assert!(
        error
            .to_string()
            .contains("2 published bindings but 0 durable port leases"),
        "the rejection must name the missing authority: {error}"
    );
    assert!(
        !manifest.network_layout.netns_path.exists()
            && !manifest.network_layout.status_path.exists(),
        "lease validation must precede namespace creation and Netavark provider effects"
    );
}

#[test]
fn restart_teardown_returns_confirmed_absent_netavark_bindings_to_reserved() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-netavark-restart"),
            None,
            None,
        )
        .expect("execute launch should reserve published listeners")
        .manifest;
    mark_provider_owned(&backend, &mut manifest);
    activate_netavark_with_live_lifetimes(&backend, &manifest);
    manifest.egress_proxy = None;

    backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Restart,
        )
        .expect("confirmed Netavark absence should retain the exact ports for restart");
    let authority = LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("authority should reopen");
    let first_cleanup = manifest
        .port_leases
        .iter()
        .map(|request| {
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("retained lease should remain durable")
        })
        .collect::<Vec<_>>();
    backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Restart,
        )
        .expect("restart cleanup replay should accept already-clean retained bindings");

    for ((request, binding), first_record) in manifest
        .port_leases
        .iter()
        .zip(&manifest.spec.port_bindings)
        .zip(&first_cleanup)
    {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("retained lease should remain durable");
        assert_eq!(
            &record, first_record,
            "cleanup replay must preserve the exact retained authority"
        );
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert_eq!(
            record.reserved_port(),
            std::num::NonZeroU16::new(binding.host_port)
        );
        assert!(
            record.binding().is_none()
                && record.bind_claim().is_none()
                && record.failure().is_none(),
            "restart authority must not claim provider evidence after confirmed teardown"
        );
    }
}

#[test]
fn restart_reset_keeps_exit_receipt_until_stale_pidfiles_are_removed() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-restart-artifact-checkpoint"),
            None,
            None,
        )
        .expect("execute launch should reserve published listeners")
        .manifest;
    mark_provider_owned(&backend, &mut manifest);
    activate_netavark_with_live_lifetimes(&backend, &manifest);
    manifest.egress_proxy = None;
    manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/false");
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            manifest.handle.id
        ),
    ]);
    fs::create_dir_all(&manifest.conmon_layout.container_state_dir)
        .expect("runtime state directory should exist");
    fs::write(&manifest.conmon_layout.exit_status_file, b"23\n")
        .expect("restart exit receipt should exist");
    fs::create_dir(&manifest.conmon_layout.pidfile)
        .expect("directory-shaped pidfile should force removal failure");
    fs::write(manifest.conmon_layout.pidfile.join("blocker"), b"retain")
        .expect("pidfile blocker should exist");
    fs::create_dir(&manifest.conmon_layout.conmon_pidfile)
        .expect("directory-shaped conmon pidfile should force removal failure");
    fs::write(
        manifest.conmon_layout.conmon_pidfile.join("blocker"),
        b"retain",
    )
    .expect("conmon pidfile blocker should exist");

    let pidfile_error = backend
        .reset_runtime_for_restart(&manifest)
        .expect_err("pidfile removal failure must retain the exit checkpoint");
    assert!(
        pidfile_error.to_string().contains("stale runtime artifact"),
        "the exact artifact failure must remain visible: {pidfile_error}"
    );
    assert!(
        manifest.conmon_layout.exit_status_file.exists(),
        "pidfile failure must not consume restart eligibility"
    );
    let authority = LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("authority should reopen");
    let retained = manifest
        .port_leases
        .iter()
        .map(|request| {
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("retained lease should remain durable")
        })
        .collect::<Vec<_>>();

    fs::remove_file(manifest.conmon_layout.pidfile.join("blocker"))
        .expect("pidfile blocker should remove");
    fs::remove_dir(&manifest.conmon_layout.pidfile)
        .expect("directory-shaped pidfile should remove");
    let conmon_error = backend
        .reset_runtime_for_restart(&manifest)
        .expect_err("conmon pidfile failure must retain the exit checkpoint");
    assert!(
        conmon_error.to_string().contains("stale runtime artifact"),
        "the exact conmon artifact failure must remain visible: {conmon_error}"
    );
    assert!(
        manifest.conmon_layout.exit_status_file.exists(),
        "conmon pidfile failure must not consume restart eligibility"
    );
    for (request, expected) in manifest.port_leases.iter().zip(&retained) {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("retained lease should remain durable"),
            *expected,
            "restart retry must preserve exact retained network authority"
        );
    }

    fs::remove_file(manifest.conmon_layout.conmon_pidfile.join("blocker"))
        .expect("conmon pidfile blocker should remove");
    fs::remove_dir(&manifest.conmon_layout.conmon_pidfile)
        .expect("directory-shaped conmon pidfile should remove");
    backend
        .reset_runtime_for_restart(&manifest)
        .expect("exact runtime absence and clean artifacts must converge");
    assert!(
        !manifest.conmon_layout.exit_status_file.exists(),
        "successful cleanup must consume the exit receipt last"
    );
    for (request, expected) in manifest.port_leases.iter().zip(&retained) {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("retained lease should remain durable"),
            *expected,
            "successful retry must not duplicate network transitions"
        );
    }
}

#[test]
fn dead_restart_netavark_claim_returns_to_reserved_before_terminal_release() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-restart-claimed-final-cleanup"),
            None,
            None,
        )
        .expect("execute launch should reserve published listeners")
        .manifest;
    let claim = adopt_launch_network(&backend, &mut manifest);
    manifest.egress_proxy = None;
    let port_lease_coordinator = backend.port_lease_coordinator();
    let restart_lifetimes = port_lease_coordinator
        .claim_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("restart setup must durably claim the next attempt");
    let restart_claims = restart_lifetimes.claims().to_vec();
    drop(restart_lifetimes);
    let authority = LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("authority should reopen");
    for (request, expected_claim) in manifest.port_leases.iter().zip(&restart_claims) {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("restart claim should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert_eq!(record.bind_claim(), Some(expected_claim));
        assert!(
            record.confirmed_stopped_binding().is_none(),
            "an initial claim-only launch must not fabricate a prior-stop receipt"
        );
    }

    let interrupted_recoveries = port_lease_coordinator
        .recover_netavark_claims_after_owner_death(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
            &restart_claims,
        )
        .expect("fresh recovery should quarantine the exact dead claim batch");
    drop(interrupted_recoveries);
    for request in &manifest.port_leases {
        assert_eq!(
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("quarantined claim should remain durable")
                .phase(),
            PortLeasePhase::CleanupPending,
            "an interrupted absence check must retain its exact cleanup fence"
        );
    }

    backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Restart,
        )
        .expect("confirmed provider absence must return dead claims to rebindable reservations");
    for request in &manifest.port_leases {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("restart-retained evidence should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert_eq!(record.reservation_claim(), Some(&claim));
        assert!(
            record.bind_claim().is_none()
                && record.binding().is_none()
                && record.failure().is_none()
                && record.active_lifetime().is_none(),
            "restart recovery must clear only the dead provider claim and lifetime"
        );
        assert!(
            record.confirmed_stopped_binding().is_none(),
            "restart recovery must not fabricate provider binding evidence"
        );
    }

    let terminal_lifetimes = port_lease_coordinator
        .claim_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the clean restart reservation must admit a higher bind lifetime");
    drop(terminal_lifetimes);
    backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Final,
        )
        .expect(
            "confirmed provider absence must abandon exact restart claims and release their slots",
        );
    for request in &manifest.port_leases {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("released evidence should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Released);
        assert!(
            record.bind_claim().is_none()
                && record.binding().is_none()
                && record.confirmed_stopped_binding().is_none()
                && record.failure().is_none(),
            "terminal cleanup must retire every provider and restart receipt; the coordinator \
             claim may remain only as terminal audit evidence"
        );
    }
    backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Final,
        )
        .expect("terminal cleanup replay should remain idempotent");
}

#[test]
fn failed_restart_teardown_retains_exact_active_netavark_evidence() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-netavark-restart-failure"),
            None,
            None,
        )
        .expect("execute launch should reserve published listeners")
        .manifest;
    mark_provider_owned(&backend, &mut manifest);
    activate_netavark_with_live_lifetimes(&backend, &manifest);
    manifest.egress_proxy = None;
    fs::create_dir_all(
        manifest
            .network_layout
            .netns_path
            .parent()
            .expect("netns parent should exist"),
    )
    .expect("netns parent should create");
    fs::write(&manifest.network_layout.netns_path, b"owned test netns\n")
        .expect("netns marker should create");
    let mut setup_config = manifest
        .network_config
        .clone()
        .expect("fixture should retain network config");
    setup_config.netavark_path = PathBuf::from("/usr/bin/true");
    crate::backends::oci::network::setup_container_network(
        &backend.ipam_authority,
        &crate::backends::oci::network::OciNetavarkOperation::new(
            &manifest.network_layout,
            &setup_config,
            &manifest.handle.id,
            manifest.spec.display_name(),
            manifest.spec.display_name(),
            &manifest.spec.port_bindings,
            None,
        ),
    )
    .expect("fixture should establish Ready Netavark authority before restart teardown");

    let error = backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Restart,
        )
        .expect_err("missing IPAM evidence must make provider teardown ambiguous");
    assert!(
        error.to_string().contains("netavark teardown"),
        "restart must report the provider teardown failure: {error}"
    );
    assert!(
        manifest.network_layout.netns_path.exists(),
        "ambiguous Netavark detach must retain the namespace retry handle"
    );

    let authority = LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("authority should reopen");
    for request in &manifest.port_leases {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("active fence should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Active);
        assert!(
            record.binding().is_some(),
            "failed provider teardown must retain exact active binding evidence"
        );
    }
}

#[test]
fn failed_krun_activation_teardown_retains_retry_evidence_until_confirmed_detach() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let netavark_stub = temp_dir.path().join("netavark-retry-stub");
    let netavark_count = temp_dir.path().join("netavark-retry-stub.count");
    fs::write(
        &netavark_stub,
        b"#!/bin/sh\n\
          count_file=\"$0.count\"\n\
          count=0\n\
          if [ -f \"$count_file\" ]; then read -r count < \"$count_file\"; fi\n\
          count=$((count + 1))\n\
          printf '%s\\n' \"$count\" > \"$count_file\"\n\
          if [ \"$count\" -le 2 ]; then\n\
            printf '%s\\n' 'forced exact teardown retry failure' >&2\n\
            exit 1\n\
          fi\n\
          exit 0\n",
    )
    .expect("Netavark retry stub should write");
    fs::set_permissions(&netavark_stub, fs::Permissions::from_mode(0o755))
        .expect("Netavark retry stub should be executable");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.netavark_path = netavark_stub;
    let backend = KrunSandboxBackend::new(config);
    let mut manifest = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("krun-activation-detach-fence"),
            None,
            None,
        )
        .expect("execute launch should reserve complete network authority")
        .manifest;
    adopt_launch_network(&backend, &mut manifest);
    let lifetimes = backend
        .port_lease_coordinator()
        .claim_netavark_bindings_with_lifetimes(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
        .expect("the activation boundary must retain exact Netavark claims");
    let claims = lifetimes.claims().to_vec();
    fs::create_dir_all(
        manifest
            .network_layout
            .netns_path
            .parent()
            .expect("netns path should have a parent"),
    )
    .expect("netns parent should create");
    fs::write(&manifest.network_layout.netns_path, b"owned krun netns\n")
        .expect("netns retry marker should create");
    let mut setup_config = manifest
        .network_config
        .clone()
        .expect("fixture should retain network config");
    setup_config.netavark_path = PathBuf::from("/usr/bin/true");
    crate::backends::oci::network::setup_container_network(
        &backend.ipam_authority,
        &crate::backends::oci::network::OciNetavarkOperation::new(
            &manifest.network_layout,
            &setup_config,
            &manifest.handle.id,
            manifest.spec.display_name(),
            manifest.spec.display_name(),
            &manifest.spec.port_bindings,
            None,
        ),
    )
    .expect("fixture should establish Ready Netavark authority before activation failure");
    let authority = LocalPortLeaseAuthority::open(&backend.config.network_state_root)
        .expect("authority should open");
    let before = manifest
        .port_leases
        .iter()
        .map(|request| {
            authority
                .inspect(request.lease_id())
                .expect("lease should inspect")
                .expect("claimed lease should remain durable")
        })
        .collect::<Vec<_>>();

    let error = backend.failed_netavark_configuration(
        &manifest,
        manifest
            .network_config
            .as_ref()
            .expect("execute manifest should persist its network config"),
        lifetimes,
        SandboxError::OperationFailed {
            message: "forced Netavark activation failure".to_owned(),
        },
    );
    assert!(
        error
            .to_string()
            .contains("forced Netavark activation failure")
            && error
                .to_string()
                .contains("exact-generation detach compensation also failed"),
        "the result must preserve both activation and detach failures: {error}"
    );
    assert!(
        manifest.network_layout.netns_path.exists(),
        "failed inline detach must retain the namespace retry handle"
    );
    assert_eq!(
        fs::read_to_string(&netavark_count)
            .expect("first teardown invocation should be recorded")
            .trim(),
        "1"
    );
    for ((request, expected_claim), expected_record) in
        manifest.port_leases.iter().zip(&claims).zip(&before)
    {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("claim fence should remain durable");
        assert_eq!(&record, expected_record);
        assert_eq!(record.bind_claim(), Some(expected_claim));
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
    }
    let network_store = LocalNetworkStateStore::open(&backend.config.network_state_root)
        .expect("network authority should reopen");
    let deleting_before_retry = network_store
        .read::<serde_json::Value>(&NetworkStatePartition::TenantIpam(
            manifest.spec.tenant_id.clone(),
        ))
        .expect("pending delete authority should inspect")
        .expect("pending delete partition should remain durable");

    let cleanup_error = backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Final,
        )
        .expect_err("outer cleanup must also fail closed while detach is ambiguous");
    assert!(
        cleanup_error
            .to_string()
            .contains("refusing a duplicate delete"),
        "outer cleanup must inspect the exact durable delete attempt without replaying its \
         ambiguous provider effect: {cleanup_error}"
    );
    assert!(
        manifest.network_layout.netns_path.exists(),
        "outer cleanup must not manufacture provider absence by deleting the namespace"
    );
    assert_eq!(
        fs::read_to_string(&netavark_count)
            .expect("the sole teardown invocation should remain recorded")
            .trim(),
        "1",
        "ambiguous teardown retry must never invoke the provider a second time"
    );
    let deleting_after_retry = network_store
        .read::<serde_json::Value>(&NetworkStatePartition::TenantIpam(
            manifest.spec.tenant_id.clone(),
        ))
        .expect("retried delete authority should inspect")
        .expect("retried delete partition should remain durable");
    assert_eq!(
        deleting_after_retry, deleting_before_retry,
        "a failed retry must preserve the exact setup generation, delete attempt, IPAM, and reservation authority byte-for-byte"
    );
    for ((request, expected_claim), expected_record) in
        manifest.port_leases.iter().zip(&claims).zip(&before)
    {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("claim fence should remain durable");
        assert_eq!(
            record.request(),
            expected_record.request(),
            "ambiguous cleanup must preserve the exact immutable lease generation"
        );
        assert_eq!(
            record.reservation_claim(),
            expected_record.reservation_claim(),
            "ambiguous cleanup must preserve the exact launch coordinator"
        );
        assert_eq!(record.bind_claim(), Some(expected_claim));
        assert_eq!(
            record.active_lifetime(),
            expected_record.active_lifetime(),
            "ambiguous cleanup must preserve the exact dead provider lifetime"
        );
        assert_eq!(record.phase(), PortLeasePhase::CleanupPending);
    }

    fs::remove_file(&manifest.network_layout.netns_path)
        .expect("provider absence fixture should remove the exact namespace projection");
    backend
        .release_network_artifacts(
            &manifest,
            super::super::lifecycle::NetworkArtifactTeardownMode::Final,
        )
        .expect("exact observed absence should confirm detach and converge cleanup");
    assert_eq!(
        fs::read_to_string(&netavark_count)
            .expect("the sole teardown invocation should remain recorded")
            .trim(),
        "1",
        "absence confirmation must not replay the already ambiguous provider effect"
    );
    #[cfg(target_os = "linux")]
    assert!(
        !manifest.network_layout.netns_path.exists(),
        "Linux namespace removal must follow confirmed provider detach"
    );
    for request in &manifest.port_leases {
        let record = authority
            .inspect(request.lease_id())
            .expect("lease should inspect")
            .expect("terminal evidence should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Released);
        assert_eq!(
            record.reservation_claim(),
            manifest.reservation_claim(),
            "terminal no-effect release must retain only its exact replay-authentication claim"
        );
        assert!(
            record.bind_claim().is_none()
                && record.binding().is_none()
                && record.confirmed_stopped_binding().is_none()
                && record.failure().is_none(),
            "terminal cleanup must retire every provider receipt: {record:?}"
        );
    }
}

#[test]
fn stale_foreign_launch_plan_cannot_disturb_the_durable_winner() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let winner = backend
        .plan_start_with_id(
            &sample_spec(),
            &SandboxId::new("foreign-krun-launch-claim"),
            None,
            None,
        )
        .expect("launch should reserve its complete port batch")
        .manifest;
    let authoritative_claim = winner
        .require_reserved_claim()
        .expect("initial launch should retain coordinator authority")
        .clone();
    let foreign_provider: NetworkProviderId = "netprovider_01ARZ3NDEKTSV4RRFFQ69G5FAV"
        .parse()
        .expect("fixture provider id should parse");
    let mut stale = KrunStartPlan {
        manifest: winner.clone(),
    };
    stale.manifest.launch_authority = KrunLaunchAuthority::Reserved {
        reservation_claim: NetworkReservationClaim::new(
            NetworkProviderHandle::new(foreign_provider, "foreign-krun-coordinator")
                .expect("foreign claim should validate"),
        ),
    };
    let mut launch_batch = winner.port_leases.clone();
    launch_batch.push(
        winner
            .egress_proxy
            .as_ref()
            .expect("execute launch should reserve its PEP")
            .port_lease
            .clone(),
    );

    let error = backend
        .execute_start_after_preflight(&stale, Ok(()))
        .expect_err("a stale foreign plan must fail before krun provider effects");
    assert!(
        error
            .to_string()
            .contains("no longer owns the durable reserved launch plan"),
        "the preflight rejection must identify the stale owner: {error}"
    );
    assert!(
        !winner.network_layout.netns_path.exists() && !winner.network_layout.status_path.exists(),
        "coordinator authentication must precede namespace and Netavark effects"
    );
    assert_eq!(
        backend
            .read_manifest(&winner.handle.id)
            .expect("winner manifest should inspect")
            .expect("winner manifest should remain durable"),
        winner,
        "the rejected stale launch may not rewrite the winner's desired state"
    );
    let authority = LocalPortLeaseAuthority::open(&backend.config.network_state_root)
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
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&winner.spec.tenant_id)
            .expect("winner segment authority should inspect")
            .is_some(),
        "the stale launch may not invoke broad segment teardown"
    );
}

#[test]
fn broad_teardown_rejects_reserved_and_plan_only_authority_before_provider_effects() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-broad-teardown-fence", "api");
    let recorder = Arc::new(RecordingSegmentAllocator::new(
        spec.tenant_id.clone(),
        "10.79.0.0/24",
        79,
    ));
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf()),
        injected,
    );
    let reserved = backend
        .plan_start_with_id(
            &spec,
            &SandboxId::new("krun-reserved-teardown-fence"),
            None,
            None,
        )
        .expect("execute plan should reserve exact authority")
        .manifest;
    let before_reserved = recorder.operations();

    let reserved_error = backend
        .release_network_artifacts(
            &reserved,
            super::super::lifecycle::NetworkArtifactTeardownMode::Final,
        )
        .expect_err("reserved authority must not authorize broad teardown");
    assert!(
        reserved_error
            .to_string()
            .contains("cannot run provider teardown from launch authority Reserved"),
        "the authority rejection must be explicit: {reserved_error}"
    );
    assert_eq!(
        recorder.operations(),
        before_reserved,
        "reserved-authority rejection must precede quarantine or release effects"
    );

    let plan_only = sample_manifest(
        sample_spec_for_tenant("krun-plan-only-fence", "api"),
        KrunStartMode::PlanOnly,
    );
    let before_plan_only = recorder.operations();
    let plan_only_error = backend
        .release_network_artifacts(
            &plan_only,
            super::super::lifecycle::NetworkArtifactTeardownMode::Final,
        )
        .expect_err("plan-only authority must not authorize broad teardown");
    assert!(
        plan_only_error
            .to_string()
            .contains("cannot run provider teardown from launch authority PlanOnly"),
        "the plan-only rejection must be explicit: {plan_only_error}"
    );
    assert_eq!(
        recorder.operations(),
        before_plan_only,
        "plan-only rejection must precede every provider effect"
    );
}

#[test]
fn losing_port_coordinator_cannot_release_winning_segment_authority() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    let first = KrunSandboxBackend::new(config.clone());
    let second = KrunSandboxBackend::new(config);
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("shared-segment-coordinator");

    let winner = first
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("first coordinator should reserve the launch")
        .manifest;
    let losing_error = second
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect_err("second coordinator must lose the exact port reservation");
    assert!(
        losing_error
            .to_string()
            .contains("already exists; refusing to replace another launch owner"),
        "the losing planner must fail at the durable manifest owner fence: {losing_error}"
    );

    let segments = first
        .segment_allocator
        .inspect_segments(&spec.tenant_id)
        .expect("segment authority should inspect");
    assert!(
        segments.is_some(),
        "losing port admission must not release segment authority still owned by the winning launch"
    );
    let authority = LocalPortLeaseAuthority::open(&first.config.network_state_root)
        .expect("port authority should reopen");
    for request in winner.port_leases.iter().chain(
        winner
            .egress_proxy
            .iter()
            .map(|assignment| &assignment.port_lease),
    ) {
        let record = authority
            .inspect(request.lease_id())
            .expect("winning lease should inspect")
            .expect("winning lease should remain durable");
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert_eq!(
            record.reservation_claim(),
            Some(
                winner
                    .require_reserved_claim()
                    .expect("winner should retain reserved authority")
            )
        );
    }
}

#[test]
fn post_adoption_cleanup_failure_is_returned_with_primary_error() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.netavark_path = PathBuf::from("/usr/bin/false");
    config.runtime_path = PathBuf::from("/usr/bin/false");
    let recorder = Arc::new(
        RecordingSegmentAllocator::new(sample_spec().tenant_id, "10.76.0.0/24", 76)
            .with_quarantine_failure("forced segment quarantine failure"),
    );
    let injected: Arc<OciSegmentAllocator> = recorder;
    let backend = KrunSandboxBackend::with_segment_allocator(config, injected);
    let mut launch_plan = backend
        .plan_start(&sample_spec())
        .expect("execute planning should reserve launch authority");
    launch_plan.manifest.conmon_launch.delete_command = CommandSpec::new("/usr/bin/true");
    launch_plan.manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "printf '%s\\n' 'container `{0}` does not exist: open `/run/crun/{0}/status`: No such file or directory' >&2; exit 1",
            launch_plan.manifest.handle.id
        ),
    ]);
    backend
        .write_manifest(&launch_plan.manifest)
        .expect("explicit runtime-absence fixture must remain the durable launch plan");

    let error = backend
        .execute_start_after_preflight(&launch_plan, Ok(()))
        .expect_err("provider launch and adopted-authority cleanup must both fail");
    let message = error.to_string();
    assert!(
        message.contains("krun launch failed:")
            && (message.contains("netavark setup")
                || message.contains("krun execution requires a Linux host")),
        "the primary launch failure must remain visible: {message}"
    );
    assert!(
        message.contains("cleanup also failed")
            && message.contains("forced segment quarantine failure"),
        "post-adoption cleanup failure must not be discarded: {message}"
    );
    let persisted = backend
        .read_manifest(&launch_plan.manifest.handle.id)
        .expect("cleanup checkpoint should inspect")
        .expect("cleanup checkpoint should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Stopping);
    assert!(
        persisted.permits_provider_teardown(),
        "failed broad cleanup must retain authenticated provider teardown authority"
    );
}

#[test]
fn pre_provider_failure_compensates_unstarted_ports_and_segment_hold() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    let state_root = config.network_state_root.clone();
    let backend = KrunSandboxBackend::new(config);
    let spec = sample_spec();
    let launch_plan = backend
        .plan_start(&spec)
        .expect("execute planning should reserve the complete launch batch");
    assert_eq!(launch_plan.manifest.port_leases.len(), 2);
    assert!(launch_plan.manifest.egress_proxy.is_some());

    let error = backend
        .execute_start_after_preflight(
            &launch_plan,
            Err(crate::error::SandboxError::BackendUnavailable {
                message: "forced pre-provider rejection".to_owned(),
            }),
        )
        .expect_err("pre-provider rejection should fail the launch");
    assert!(
        error.to_string().contains("forced pre-provider rejection"),
        "the original preflight failure must remain primary: {error}"
    );

    let records = nimbus_network::LocalPortLeaseAuthority::open(&state_root)
        .expect("port authority should reopen")
        .list()
        .expect("port leases should list");
    assert_eq!(records.len(), 3);
    assert!(
        records
            .iter()
            .all(|record| record.phase() == nimbus_network::PortLeasePhase::Released),
        "every publication and PEP reservation was proven never bound: {records:?}"
    );
    let segments = backend
        .segment_allocator
        .inspect_segments(&spec.tenant_id)
        .expect("segment authority should inspect")
        .unwrap_or_default();
    assert!(
        segments.is_empty(),
        "pre-provider compensation must finalize the unrealized segment hold: {segments:?}"
    );
}

#[test]
fn artifact_cleanup_failure_does_not_suppress_never_bound_network_release() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    let state_root = config.network_state_root.clone();
    let backend = KrunSandboxBackend::new(config);
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("krun-independent-compensation");
    let artifact_path = temp_dir.path().join("rootfs-shaped-file");
    fs::write(&artifact_path, b"not a directory")
        .expect("artifact cleanup blocker should be a regular file");
    let launch_artifact = super::super::KrunLaunchArtifact::Rootfs(MaterializedImageRootfs {
        image_reference: "registry.example.com/acme/api:independent-cleanup".to_owned(),
        rootfs_path: artifact_path.clone(),
    });
    let mut manifest = backend
        .plan_start_with_id(&spec, &sandbox_id, None, Some(launch_artifact))
        .expect("execute planning should reserve artifact and network authority")
        .manifest;

    let error = backend.persist_unstarted_launch_failure(
        &mut manifest,
        SandboxError::BackendUnavailable {
            message: "forced pre-provider rejection".to_owned(),
        },
    );
    let message = error.to_string();
    assert!(
        message.contains("forced pre-provider rejection")
            && message.contains("krun launch artifact compensation failed"),
        "the primary and independent artifact failure must both survive: {message}"
    );
    assert!(
        artifact_path.exists() && manifest.launch_artifact.is_some(),
        "failed artifact cleanup must retain its exact retry evidence"
    );

    let records = LocalPortLeaseAuthority::open(&state_root)
        .expect("port authority should reopen")
        .list()
        .expect("port leases should list");
    assert_eq!(
        records.len(),
        manifest.port_leases.len() + usize::from(manifest.egress_proxy.is_some())
    );
    assert!(
        records
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released),
        "independent safe network compensation must release every never-bound lease despite the \
         artifact failure: {records:?}"
    );
    let segments = backend
        .segment_allocator
        .inspect_segments(&spec.tenant_id)
        .expect("segment authority should inspect")
        .unwrap_or_default();
    assert!(
        segments.is_empty(),
        "independent safe network compensation must finalize the unrealized segment hold despite \
         the artifact failure: {segments:?}"
    );
}

#[test]
fn unpublished_manifest_compensation_retains_original_lifetime_through_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    let state_root = config.network_state_root.clone();
    let backend = KrunSandboxBackend::new(config);
    let spec = sample_spec();
    let sandbox_id = SandboxId::new("krun-unpublished-live-lifetime");

    let error = backend
        .plan_start_with_id_at_reserved_publication_for_test(
            &spec,
            &sandbox_id,
            |reserved_manifest| {
                let request = reserved_manifest
                    .port_leases
                    .first()
                    .expect("reservation callback should observe published request projection");
                let authority = LocalPortLeaseAuthority::open(&state_root)
                    .expect("competing authority should reopen");
                let reservation_claim = reserved_manifest
                    .require_reserved_claim()
                    .expect("reserved manifest should retain its launch coordinator");
                assert!(
                    matches!(
                        authority
                            .try_acquire_reservation_lifetime(reservation_claim)
                            .expect("live reservation lifetime inspection should succeed"),
                        NetworkReservationLifetimeAttempt::LiveOwner
                    ),
                    "the original coordinator lifetime must remain live before publication"
                );
                assert!(
                    authority.reserve(request.clone()).is_err(),
                    "a claimless contender must not replay the live coordinator reservation"
                );
                Err(SandboxError::OperationFailed {
                    message: "injected reserved-manifest publication failure".to_owned(),
                })
            },
        )
        .expect_err("reserved-manifest publication failure should compensate the launch");
    assert!(
        error
            .to_string()
            .contains("injected reserved-manifest publication failure"),
        "the publication failure must remain primary: {error}"
    );

    let persisted = backend
        .read_manifest(&sandbox_id)
        .expect("compensation result should inspect")
        .expect("compensation result should remain durable");
    assert!(persisted.shutdown_requested);
    assert_eq!(persisted.status, SandboxStatus::Failed);
    assert_eq!(persisted.launch_authority, KrunLaunchAuthority::Released);
    let records = LocalPortLeaseAuthority::open(&state_root)
        .expect("port authority should reopen")
        .list()
        .expect("port leases should list");
    assert_eq!(records.len(), 3);
    assert!(
        records
            .iter()
            .all(|record| record.phase() == PortLeasePhase::Released),
        "same-owner compensation must release the exact publication and PEP batch: {records:?}"
    );
    assert!(
        backend
            .segment_allocator
            .inspect_segments(&spec.tenant_id)
            .expect("segment authority should inspect")
            .unwrap_or_default()
            .is_empty(),
        "the lifetime must remain held through complete IPAM and segment compensation"
    );
}

#[test]
fn vm_config_materialization_failure_compensates_unstarted_launch_batch() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    let state_root = config.network_state_root.clone();
    let backend = KrunSandboxBackend::new(config);
    let missing_rootfs = temp_dir.path().join("missing-rootfs");
    let spec = sample_spec_with_rootfs(&missing_rootfs).with_resource_limits(
        SandboxResourceLimits::default()
            .with_cpu_count(2)
            .with_memory_limit_bytes(256 * 1024 * 1024),
    );

    let sandbox_id = SandboxId::new("vm-config-materialization-failure");
    let network_plan =
        sample_provision_network_plan(&spec, &sandbox_id, "vm-config-materialization-failure");
    backend
        .reserve_provision_network(spec.clone(), sandbox_id.clone(), network_plan)
        .expect("the exact network envelope should reserve before materialization");
    let error = backend
        .prepare_provision_workload(&sandbox_id)
        .expect_err("vm config write should fail before bind");
    assert!(
        error.to_string().contains("failed to write krun vm config"),
        "the materialization failure must remain primary: {error}"
    );
    block_on(backend.stop(&sandbox_id))
        .expect("the coordinator's rollback stop should release the reserved launch batch");

    let records = nimbus_network::LocalPortLeaseAuthority::open(&state_root)
        .expect("port authority should reopen")
        .list()
        .expect("port leases should list");
    assert_eq!(records.len(), 3);
    assert!(
        records
            .iter()
            .all(|record| record.phase() == nimbus_network::PortLeasePhase::Released),
        "failed materialization must release every never-bound reservation: {records:?}"
    );
    let segments = backend
        .segment_allocator
        .inspect_segments(&spec.tenant_id)
        .expect("segment authority should inspect")
        .unwrap_or_default();
    assert!(
        segments.is_empty(),
        "failed materialization must finalize the unrealized segment hold: {segments:?}"
    );
}

#[test]
fn first_attachment_reservation_observes_a_durable_claim_only_manifest() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-claim-before-placement", "api");
    let sandbox_id = SandboxId::new("krun-claim-before-placement");
    let state_root = temp_dir.path().join("state");
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed_in_callback = Arc::clone(&observed);
    let state_root_in_callback = state_root.clone();
    let sandbox_id_in_callback = sandbox_id.clone();
    let recorder = Arc::new(
        RecordingSegmentAllocator::new(spec.tenant_id.clone(), "10.80.0.0/24", 80)
            .with_reserve_attachment_observer(move |claim| {
                let manifest_path = crate::artifact_paths::manifest_path_for_sandbox_id(
                    &state_root_in_callback,
                    &sandbox_id_in_callback,
                )
                .expect("manifest lookup should succeed")
                .expect("claim shell must exist before attachment reservation");
                let persisted: KrunSandboxManifest = serde_json::from_slice(
                    &fs::read(&manifest_path).expect("claim shell should be readable"),
                )
                .expect("claim shell should deserialize");
                assert_eq!(
                    persisted
                        .require_reserved_claim()
                        .expect("claim should persist"),
                    claim
                );
                assert!(persisted.network_config.is_none());
                assert!(persisted.port_leases.is_empty());
                assert!(persisted.egress_proxy.is_none());
                observed_in_callback.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }),
    );
    let injected: Arc<OciSegmentAllocator> = recorder;
    let config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    assert_eq!(config.workload_state_root, state_root);
    let backend = KrunSandboxBackend::with_segment_allocator(config, injected);

    backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("launch should complete after the claim-shell observation");

    assert!(
        observed.load(std::sync::atomic::Ordering::SeqCst),
        "the first attachment effect must observe the durable claim-only barrier"
    );
}

#[test]
fn attachment_adoption_observes_a_durable_adopting_manifest() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-adopting-barrier", "api");
    let sandbox_id = SandboxId::new("krun-adopting-barrier");
    let state_root = temp_dir.path().join("state");
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed_in_callback = Arc::clone(&observed);
    let state_root_in_callback = state_root.clone();
    let sandbox_id_in_callback = sandbox_id.clone();
    let recorder = Arc::new(
        RecordingSegmentAllocator::new(spec.tenant_id.clone(), "10.82.0.0/24", 82)
            .with_adopt_attachment_observer(move |claim| {
                let manifest_path = crate::artifact_paths::manifest_path_for_sandbox_id(
                    &state_root_in_callback,
                    &sandbox_id_in_callback,
                )
                .expect("manifest lookup should succeed")
                .expect("adoption intent must exist before allocator adoption");
                let persisted: KrunSandboxManifest = serde_json::from_slice(
                    &fs::read(&manifest_path).expect("adoption intent should be readable"),
                )
                .expect("adoption intent should deserialize");
                assert_eq!(
                    persisted.launch_authority,
                    KrunLaunchAuthority::Adopting {
                        reservation_claim: claim.clone(),
                    },
                    "allocator adoption must observe its exact durable intermediate authority"
                );
                assert_eq!(persisted.status, SandboxStatus::Starting);
                assert!(
                    !persisted.port_leases.is_empty() || persisted.egress_proxy.is_some(),
                    "adoption intent must retain its exact host-port reservation batch"
                );
                assert!(persisted.network_config.is_some());
                observed_in_callback.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }),
    );
    let injected: Arc<OciSegmentAllocator> = recorder;
    let backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf()),
        injected,
    );
    let plan = backend
        .plan_start_with_id(&spec, &sandbox_id, None, None)
        .expect("launch should reserve its complete network batch");

    let _ = backend.execute_start_after_preflight(&plan, Ok(()));

    assert!(
        observed.load(std::sync::atomic::Ordering::SeqCst),
        "the adoption effect must be ordered after durable Adopting authority"
    );
}

#[test]
fn ambiguous_placement_failure_retains_reserved_authority_for_restart_cleanup() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let spec = sample_spec_for_tenant("krun-placement-retry", "api");
    let sandbox_id = SandboxId::new("krun-placement-retry");
    let network_plan =
        sample_provision_network_plan(&spec, &sandbox_id, "ambiguous-placement-retry");
    let recorder = Arc::new(
        RecordingSegmentAllocator::new(spec.tenant_id.clone(), "10.81.0.0/24", 81)
            .with_reserve_attachment_observer(|_| {
                Err(SandboxError::OperationFailed {
                    message: "injected ambiguous placement acknowledgement".to_owned(),
                })
            })
            .with_release_reserved_failure("injected exact placement cleanup failure"),
    );
    let injected: Arc<OciSegmentAllocator> = recorder.clone();
    let backend = KrunSandboxBackend::with_segment_allocator(
        KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf()),
        injected,
    );

    let error = backend
        .plan_reserved_provision_with_id(&spec, &sandbox_id, &network_plan)
        .expect_err("ambiguous placement and exact cleanup failure must fail");
    assert!(
        error
            .to_string()
            .contains("injected ambiguous placement acknowledgement")
            && error
                .to_string()
                .contains("injected exact placement cleanup failure"),
        "both the primary placement result and exact cleanup failure must survive: {error}"
    );
    let persisted = backend
        .read_manifest(&sandbox_id)
        .expect("restart checkpoint should inspect")
        .expect("restart checkpoint should remain durable");
    assert_eq!(persisted.status, SandboxStatus::Stopping);
    assert!(matches!(
        persisted.launch_authority,
        KrunLaunchAuthority::Reserved { .. }
    ));
    assert_eq!(
        persisted.provision_network_plan.as_ref(),
        Some(&network_plan),
        "the claim-only manifest must retain the complete compiler plan for exact restart compensation"
    );
    assert!(
        persisted.network_config.is_none()
            && persisted.port_leases.is_empty()
            && persisted.egress_proxy.is_none(),
        "ambiguous placement must not fabricate later provider or port evidence"
    );
    assert!(
        recorder.operations().iter().any(|operation| matches!(
            operation,
            crate::backends::oci::network::SegmentAllocatorOperation::ReleaseReservedAttachment(
                tenant,
                attachment
            ) if tenant == &spec.tenant_id
                && attachment == network_plan.attachment_id()
        )),
        "the failed planning pass must attempt only exact claimed compensation"
    );
}

#[test]
fn port_quota_failure_after_krun_placement_releases_the_segment_hold() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.max_published_ports_per_tenant = Some(0);
    let backend = KrunSandboxBackend::new(config);
    let spec = sample_spec();

    let error = backend
        .plan_start(&spec)
        .expect_err("port admission must fail after execute-mode placement");
    assert!(
        error.to_string().contains("port quota"),
        "the original port-admission failure must remain primary: {error}"
    );

    let segments = backend
        .segment_allocator
        .inspect_segments(&spec.tenant_id)
        .expect("segment authority should inspect")
        .unwrap_or_default();
    assert!(
        segments.is_empty(),
        "failed port admission must finalize the unrealized segment hold: {segments:?}"
    );
}

fn prepared_materialized_launch(rootfs_path: &Path) -> PreparedMaterializedImageLaunch {
    let mut launch_defaults = sample_launch_defaults();
    launch_defaults.rootfs = SandboxRootfsSpec::new(rootfs_path);
    PreparedMaterializedImageLaunch {
        artifact: MaterializedImageRootfs {
            image_reference: "registry.example.com/acme/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_owned(),
            rootfs_path: rootfs_path.to_path_buf(),
        },
        launch_defaults,
    }
}

#[test]
fn initial_manifest_publication_failure_removes_only_the_exact_materialized_rootfs() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(
        temp_dir.path().to_path_buf(),
    ));
    let spec = sparse_image_spec("initial-publication-owned-rootfs");
    let sandbox_id = SandboxId::new("initial-publication-owned-rootfs");
    let owned_rootfs = rootfs_artifact_path(temp_dir.path(), &spec, &sandbox_id);
    let owned_runtime_rootfs = owned_rootfs.join("rootfs");
    fs::create_dir_all(&owned_runtime_rootfs).expect("owned materialized rootfs should exist");
    fs::write(owned_runtime_rootfs.join("owned"), b"owned").expect("owned marker should write");

    let error = backend
        .plan_start_with_materialized_image_at_initial_publication_for_test(
            &spec,
            &sandbox_id,
            prepared_materialized_launch(&owned_runtime_rootfs),
            |manifest| {
                fs::create_dir_all(&manifest.conmon_layout.manifest_path)
                    .expect("manifest publication obstacle should exist");
                Ok(())
            },
        )
        .expect_err("initial manifest publication should fail");
    assert!(
        error
            .to_string()
            .contains("already exists; refusing to replace another launch owner"),
        "the injected initial-publication failure must remain primary: {error}"
    );
    assert!(
        !owned_rootfs.exists(),
        "a failed initial publication must remove the rootfs materialized by this exact launch"
    );

    let foreign_id = SandboxId::new("initial-publication-foreign-rootfs");
    let foreign_rootfs = temp_dir.path().join("foreign-rootfs");
    fs::create_dir_all(&foreign_rootfs).expect("foreign rootfs should exist");
    fs::write(foreign_rootfs.join("foreign"), b"foreign").expect("foreign marker should write");

    backend
        .plan_start_with_materialized_image_at_initial_publication_for_test(
            &spec,
            &foreign_id,
            prepared_materialized_launch(&foreign_rootfs),
            |manifest| {
                fs::create_dir_all(&manifest.conmon_layout.manifest_path)
                    .expect("foreign manifest publication obstacle should exist");
                Ok(())
            },
        )
        .expect_err("foreign-path initial manifest publication should fail closed");
    assert!(
        foreign_rootfs.join("foreign").exists(),
        "cleanup must never delete a rootfs outside this launch's deterministic artifact path"
    );
}

#[test]
fn later_krun_planning_failure_compensates_ports_and_segment_hold() {
    let temp_dir = TempDir::new().expect("temporary directory should exist");
    let blocked_bundle_root = temp_dir.path().join("bundle-root-is-a-file");
    fs::write(&blocked_bundle_root, b"not a directory").expect("obstacle should write");
    let mut config = KrunSandboxBackendConfig::under_root(temp_dir.path().to_path_buf());
    config.bundle_root = blocked_bundle_root;
    let state_root = config.network_state_root.clone();
    let backend = KrunSandboxBackend::new(config);
    let spec = sample_spec();

    let error = backend
        .plan_start(&spec)
        .expect_err("bundle materialization must fail after the atomic reservation");
    assert!(
        error.to_string().contains("bundle"),
        "the original bundle failure must remain primary: {error}"
    );

    let records = nimbus_network::LocalPortLeaseAuthority::open(&state_root)
        .expect("port authority should reopen")
        .list()
        .expect("port leases should list");
    assert_eq!(
        records.len(),
        3,
        "two publications and the PEP request should be recorded"
    );
    assert!(
        records
            .iter()
            .all(|record| record.phase() == nimbus_network::PortLeasePhase::Released),
        "known no-effect planning failure must leave no port fence: {records:?}"
    );
    let segments = backend
        .segment_allocator
        .inspect_segments(&spec.tenant_id)
        .expect("segment authority should inspect")
        .unwrap_or_default();
    assert!(
        segments.is_empty(),
        "later planning compensation must finalize the unrealized segment hold: {segments:?}"
    );
}
