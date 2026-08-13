use super::support::*;
use crate::SandboxError;
use crate::backends::oci::network::default_network_attachment_id;
use crate::backends::oci::network::{AttachmentAttachAuthority, FixedOciEgressPinProvider};
use crate::backends::oci::port_lease::{
    OciPortProvider, claim_bind_plan_member_attempt_with_lifetime,
};
use nimbus_network::{LocalPortLeaseAuthority, PortLeaseEffectScope, PortLeasePhase};
use std::sync::Arc;

fn sample_execution_attempt_id(id: &SandboxId) -> crate::SandboxExecutionAttemptId {
    crate::SandboxExecutionAttemptId::new(format!("test-execution-attempt:{id}"))
        .expect("test execution attempt should validate")
}

fn crossed_execution_attempt_id(id: &SandboxId) -> crate::SandboxExecutionAttemptId {
    crate::SandboxExecutionAttemptId::new(format!("crossed-execution-attempt:{id}"))
        .expect("crossed test execution attempt should validate")
}

#[test]
fn krun_provision_activation_classifies_runtime_state() {
    let root = TempDir::new().expect("temporary root should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("krun-activation-state-matrix");
    let spec = sample_spec_for_tenant("krun-activation-state", "api");
    let network_plan = sample_provision_network_plan(&spec, &id, "activation-state-matrix");
    backend
        .reserve_provision_network(
            spec,
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan,
        )
        .expect("activation fixture should reserve");
    let mut manifest = backend
        .read_manifest(&id)
        .expect("activation fixture manifest should read")
        .expect("activation fixture manifest should exist");

    for (state, expected) in [
        ("running", "succeeded"),
        ("creating", "in_progress"),
        ("created", "absent"),
        ("stopped", "ambiguous"),
        ("paused", "ambiguous"),
        ("unknown-provider-state", "ambiguous"),
    ] {
        manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
            "-c".to_owned(),
            format!(
                "printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"{state}\"}}'",
                id.as_str()
            ),
        ]);
        backend
            .write_manifest(&manifest)
            .expect("activation state fixture should persist");
        let observed = backend
            .inspect_provision_workload_activation(&id, &sample_execution_attempt_id(&id))
            .expect("activation state should inspect");
        let actual = match observed {
            crate::SandboxProvisionPhaseObservation::Succeeded { .. } => "succeeded",
            crate::SandboxProvisionPhaseObservation::Absent { .. } => "absent",
            crate::SandboxProvisionPhaseObservation::InProgress { .. } => "in_progress",
            crate::SandboxProvisionPhaseObservation::Ambiguous { .. } => "ambiguous",
        };
        assert_eq!(actual, expected, "runtime state {state} was misclassified");
    }
}

#[test]
fn reserve_is_durable_and_stays_unprepared_and_unattached() {
    let root = TempDir::new().expect("temporary root should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("wex-reserved-krun");
    let spec = sample_spec_for_tenant("tenant", "postgres-primary");
    let network_plan = sample_provision_network_plan(&spec, &id, "krun-reserve");

    let handle = backend
        .reserve_provision_network(
            spec.clone(),
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan.clone(),
        )
        .expect("reserve should acquire durable provider resources");
    assert_eq!(handle.id, id);
    let manifest = backend
        .read_manifest(&id)
        .expect("reserved manifest should be readable")
        .expect("reserved manifest should exist");
    assert!(manifest.network_config.is_some());
    assert!(matches!(
        manifest.launch_authority,
        KrunLaunchAuthority::Reserved { .. }
    ));
    assert!(!manifest.provision_prepared);
    assert!(
        !manifest.bundle_layout.config_path.exists(),
        "reservation must not materialize the workload bundle"
    );
    assert!(
        !manifest.network_layout.netns_path.exists(),
        "reservation must not create the workload network namespace"
    );
    assert_eq!(
        backend
            .inspect_provision_network_reservation(
                &id,
                &sample_execution_attempt_id(&id),
                &network_plan,
            )
            .expect("reservation inspection should succeed")
            .expect("reservation should be observed")
            .id,
        id
    );
    assert!(
        backend
            .inspect_provision_preparation(&id, &sample_execution_attempt_id(&id))
            .expect("preparation inspection should succeed")
            .is_none()
    );
    let error = backend
        .reserve_provision_network(
            spec,
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan,
        )
        .expect_err("direct replay must inspect rather than replace the reservation");
    assert!(error.to_string().contains("inspect it instead"));
}

#[test]
fn reservation_inspection_authenticates_plan_allocator_and_every_port_lease() {
    let root = TempDir::new().expect("temporary root should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("wex-reservation-inspection-krun");
    let spec = sample_spec_for_tenant("tenant", "reservation-inspection");
    let network_plan = sample_provision_network_plan(&spec, &id, "reservation-inspection");
    backend
        .reserve_provision_network(
            spec.clone(),
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan.clone(),
        )
        .expect("reservation fixture should reserve exact authority");
    assert!(
        backend
            .inspect_provision_network_reservation(
                &id,
                &sample_execution_attempt_id(&id),
                &network_plan,
            )
            .expect("exact reservation should inspect")
            .is_some()
    );

    let crossed_plan = sample_provision_network_plan(&spec, &id, "crossed-reservation-inspection");
    let crossed_error = backend
        .inspect_provision_network_reservation(
            &id,
            &sample_execution_attempt_id(&id),
            &crossed_plan,
        )
        .expect_err("crossed desired plan must not adopt another reservation");
    assert!(
        crossed_error
            .to_string()
            .contains("crossed its exact execution attempt or compiled network plan"),
        "{crossed_error}"
    );

    let manifest = backend
        .read_manifest(&id)
        .expect("reservation manifest should read")
        .expect("reservation manifest should exist");
    let reservation_claim = manifest
        .require_reserved_claim()
        .expect("reservation claim should persist")
        .clone();
    backend
        .port_lease_coordinator()
        .release_never_bound_launch_claim(&reservation_claim)
        .expect("fixture should compensate the exact port reservations");
    let compensated_error = backend
        .inspect_provision_network_reservation(
            &id,
            &sample_execution_attempt_id(&id),
            &network_plan,
        )
        .expect_err("stale manifest must not prove compensated provider authority");
    assert!(
        compensated_error
            .to_string()
            .contains("found prior bind, adoption, binding, stop, failure, or lifetime evidence"),
        "{compensated_error}"
    );
    assert!(
        !manifest.network_layout.netns_path.exists(),
        "read-only reservation inspection and compensation must not attach the workload"
    );
}

#[test]
fn reservation_inspection_rejects_claimed_effect_lifetime_before_binding() {
    let root = TempDir::new().expect("temporary root should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("wex-reservation-lifetime-krun");
    let spec = sample_spec_for_tenant("tenant", "reservation-lifetime");
    let network_plan = sample_provision_network_plan(&spec, &id, "reservation-lifetime");
    backend
        .reserve_provision_network(
            spec,
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan.clone(),
        )
        .expect("reservation fixture should reserve exact authority");
    let manifest = backend
        .read_manifest(&id)
        .expect("reservation manifest should read")
        .expect("reservation manifest should exist");
    let plan_members = KrunSandboxBackend::provision_port_plan_witness(&manifest);
    let request = plan_members
        .first()
        .expect("compiled plan should contain its PEP listener");
    let coordinator = backend.port_lease_coordinator();
    let (_claim, lifetime) = claim_bind_plan_member_attempt_with_lifetime(
        coordinator
            .authority()
            .expect("port authority should remain available"),
        &plan_members,
        request,
        OciPortProvider::EgressPep,
        manifest
            .require_reserved_claim()
            .expect("reservation claim should persist"),
        PortLeaseEffectScope::ProcessBound,
    )
    .expect("fixture should record a claimed process lifetime before any bind effect");

    let error = backend
        .inspect_provision_network_reservation(
            &id,
            &sample_execution_attempt_id(&id),
            &network_plan,
        )
        .expect_err("a claimed lifetime is not a never-effected reservation");
    assert!(
        error
            .to_string()
            .contains("found prior bind, adoption, binding, stop, failure, or lifetime evidence"),
        "{error}"
    );
    drop(lifetime);
    assert!(
        backend
            .inspect_provision_network_reservation(
                &id,
                &sample_execution_attempt_id(&id),
                &network_plan,
            )
            .is_err(),
        "dropping the process guard must retain historical lifetime fencing"
    );
}

#[test]
fn prepare_requires_reservation_and_stays_unattached() {
    let root = TempDir::new().expect("temporary root should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("wex-prepared-krun");
    let spec = sample_spec_for_tenant("tenant", "postgres-primary");
    let network_plan = sample_provision_network_plan(&spec, &id, "krun-prepare");

    let missing = backend
        .prepare_provision_workload(&id, &sample_execution_attempt_id(&id))
        .expect_err("preparation cannot invent a reservation");
    assert!(matches!(missing, SandboxError::NotFound { .. }));
    backend
        .reserve_provision_network(
            spec,
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan,
        )
        .expect("reservation should succeed");
    let handle = backend
        .prepare_provision_workload(&id, &sample_execution_attempt_id(&id))
        .expect("prepare should materialize the reserved workload");
    assert_eq!(handle.id, id);
    let manifest = backend
        .read_manifest(&id)
        .expect("prepared manifest should be readable")
        .expect("prepared manifest should exist");
    assert!(manifest.provision_prepared);
    assert!(manifest.bundle_layout.config_path.is_file());
    assert!(
        !manifest.network_layout.netns_path.exists(),
        "preparation must not create the workload network namespace"
    );
    assert_eq!(
        backend
            .inspect_provision_preparation(&id, &sample_execution_attempt_id(&id))
            .expect("preparation inspection should succeed")
            .expect("preparation should be observed")
            .id,
        id
    );
    assert_eq!(
        backend
            .prepare_provision_workload(&id, &sample_execution_attempt_id(&id))
            .expect("exact direct replay should adopt durable preparation")
            .id,
        id
    );
}

#[test]
fn crossed_attempt_rejects_preparation_before_artifact_or_manifest_effects() {
    let root = TempDir::new().expect("temporary root should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("wex-crossed-prepare-krun");
    let spec = sample_spec_for_tenant("tenant", "crossed-prepare");
    let manifest_path = crate::artifact_paths::manifest_path(
        &backend.config.workload_state_root,
        &spec.tenant_id,
        &id,
    );
    let network_plan = sample_provision_network_plan(&spec, &id, "crossed-prepare");
    backend
        .reserve_provision_network(
            spec,
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan,
        )
        .expect("reservation should persist the exact execution attempt");
    let reserved = backend
        .read_manifest(&id)
        .expect("reserved manifest should read")
        .expect("reserved manifest should exist");
    let manifest_before = fs::read(&manifest_path).expect("reserved manifest bytes should read");
    assert!(!reserved.bundle_layout.config_path.exists());

    let error = backend
        .prepare_provision_workload(&id, &crossed_execution_attempt_id(&id))
        .expect_err("a crossed attempt must not materialize workload artifacts");

    assert!(error.to_string().contains("crossed execution attempt"));
    assert!(
        !reserved.bundle_layout.config_path.exists(),
        "crossed preparation must not write the bundle config"
    );
    assert_eq!(
        fs::read(&manifest_path).expect("manifest should remain readable"),
        manifest_before,
        "crossed preparation must not mutate durable provider state"
    );
}

#[test]
fn crossed_attempt_rejects_activation_inspection_before_runtime_probe() {
    let root = TempDir::new().expect("temporary root should exist");
    let backend = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(root.path()));
    let id = SandboxId::new("wex-crossed-activation-inspection-krun");
    let spec = sample_spec_for_tenant("tenant", "crossed-activation-inspection");
    let manifest_path = crate::artifact_paths::manifest_path(
        &backend.config.workload_state_root,
        &spec.tenant_id,
        &id,
    );
    let network_plan = sample_provision_network_plan(&spec, &id, "crossed-inspection");
    backend
        .reserve_provision_network(
            spec,
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan,
        )
        .expect("reservation should persist the exact execution attempt");
    let marker = root.path().join("runtime-probe-ran");
    let mut manifest = backend
        .read_manifest(&id)
        .expect("reserved manifest should read")
        .expect("reserved manifest should exist");
    manifest.conmon_launch.state_command = CommandSpec::new("/bin/sh").args([
        "-c".to_owned(),
        format!(
            "touch \"$1\"; printf '%s\\n' '{{\"id\":\"{}\",\"status\":\"running\"}}'",
            id.as_str()
        ),
        "sh".to_owned(),
        marker.display().to_string(),
    ]);
    backend
        .write_manifest(&manifest)
        .expect("runtime probe fixture should persist");
    let manifest_before = fs::read(&manifest_path).expect("manifest bytes should read");

    let error = backend
        .inspect_provision_workload_activation(&id, &crossed_execution_attempt_id(&id))
        .expect_err("a crossed attempt must not execute the runtime probe");

    assert!(error.to_string().contains("crossed execution attempt"));
    assert!(
        !marker.exists(),
        "crossed inspection must not run the probe"
    );
    assert_eq!(
        fs::read(&manifest_path).expect("manifest should remain readable"),
        manifest_before,
        "crossed inspection must not mutate durable provider state"
    );
    assert!(matches!(
        backend
            .inspect_provision_workload_activation(&id, &sample_execution_attempt_id(&id))
            .expect("the exact attempt should execute the runtime probe"),
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    assert!(marker.is_file(), "the exact attempt should run the probe");
}

#[test]
fn reservation_preserves_compiler_identities_without_binding_or_routability() {
    let root = TempDir::new().expect("temporary root should exist");
    let config = KrunSandboxBackendConfig::under_root(root.path());
    let network_root = config.network_state_root.clone();
    let backend = KrunSandboxBackend::new(config);
    let id = SandboxId::new("wex-exact-krun-authority");
    let spec = sample_spec_for_tenant("tenant", "postgres-primary").with_port_bindings([
        SandboxPortBinding::tcp("exact", 18_124, 8_080),
        SandboxPortBinding::tcp("assigned", 0, 8_081),
    ]);
    let network_plan = sample_provision_network_plan(&spec, &id, "krun-exact-authority");
    let expected_plan = network_plan.network_plan().clone();
    let expected_attachment = network_plan.attachment_id().clone();
    let expected_leases = network_plan.port_leases();
    let expected_dependency = network_plan.dependency_listeners()[0].clone();
    assert_ne!(
        expected_attachment,
        default_network_attachment_id(&id),
        "the proof must not accidentally exercise the legacy sandbox-derived identity"
    );

    backend
        .reserve_provision_network(
            spec.clone(),
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan,
        )
        .expect("exact reservation should succeed");
    let manifest = backend
        .read_manifest(&id)
        .expect("manifest should read")
        .expect("manifest should exist");
    let network = manifest
        .network_config
        .as_ref()
        .expect("exact network config should persist");
    assert_eq!(network.attachment_id, expected_attachment);
    assert_eq!(network.network_plan.as_ref(), Some(&expected_plan));
    assert_eq!(manifest.port_leases, expected_leases);
    assert_eq!(manifest.spec.port_bindings[0].host_port, 18_124);
    assert_eq!(manifest.spec.port_bindings[1].host_port, 0);
    assert!(
        !manifest.network_layout.netns_path.exists(),
        "reservation must not make the attachment routable"
    );

    let authority =
        LocalPortLeaseAuthority::open(&network_root).expect("shared port authority should open");
    let records = authority
        .list_plan(expected_plan.plan_id())
        .expect("exact plan leases should inspect");
    assert_eq!(records.len(), expected_leases.len() + 1);
    for expected in &expected_leases {
        let record = records
            .iter()
            .find(|record| record.request().lease_id() == expected.lease_id())
            .expect("every compiler lease must remain present");
        assert_eq!(record.request(), expected);
        assert_eq!(record.phase(), PortLeasePhase::Reserved);
        assert!(record.binding().is_none());
        assert!(record.reservation_claim().is_some());
        match expected.binding().port() {
            nimbus_network::PortRequestMode::Exact(port) => {
                assert_eq!(record.reserved_port(), Some(*port));
            }
            nimbus_network::PortRequestMode::ProviderAssigned => {
                assert_eq!(record.reserved_port(), None);
            }
            nimbus_network::PortRequestMode::Range(_) => {
                panic!("compiled sandbox fixture must not contain a range request");
            }
        }
    }
    let pep_lease = &manifest
        .egress_proxy
        .as_ref()
        .expect("egress PEP reservation should persist")
        .port_lease;
    assert_eq!(
        pep_lease.lease_id(),
        &nimbus_network::PortLeaseId::for_listener(expected_dependency.listener_id())
    );
    assert_eq!(pep_lease.plan_id(), Some(expected_plan.plan_id()));
    assert_eq!(pep_lease.generation(), expected_plan.generation());
    assert_eq!(
        pep_lease.accounting(),
        nimbus_network::PortLeaseAccounting::HostInternal
    );
    let pep_record = records
        .iter()
        .find(|record| record.request().lease_id() == pep_lease.lease_id())
        .expect("the compiler dependency listener must own the PEP lease");
    assert_eq!(pep_record.phase(), PortLeasePhase::Reserved);
    assert!(pep_record.reserved_port().is_some());
    assert!(pep_record.binding().is_none());
    let claim = manifest
        .reservation_claim()
        .expect("reservation claim should persist");
    assert!(
        backend
            .segment_allocator
            .inspect_attachment_reservation(&spec.tenant_id, &expected_attachment, claim)
            .expect("exact allocator reservation should inspect")
            .association()
            .is_some(),
        "segment allocation must use the compiler attachment identity"
    );
}

#[test]
fn krun_attach_uses_compiler_planned_pep_authority() {
    let root = TempDir::new().expect("temporary root should exist");
    let pep_reservation = TcpListener::bind("127.0.0.1:0").expect("PEP tripwire should bind");
    let pep_port = pep_reservation
        .local_addr()
        .expect("PEP tripwire should report its port")
        .port();
    let mut config = KrunSandboxBackendConfig::under_root(root.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    let network_root = config.network_state_root.clone();
    let backend = KrunSandboxBackend::new(config).with_egress_pin_provider(Arc::new(
        crate::backends::oci::network::FixedOciEgressPinProvider::ready(),
    ));
    let id = SandboxId::new("krun-planned-pep-authority");
    let spec = sample_spec_for_tenant("krun-planned-pep", "api");
    let network_plan = sample_provision_network_plan(&spec, &id, "planned-pep-authority");
    let expected_listener = network_plan.dependency_listeners()[0].listener_id().clone();
    backend
        .reserve_provision_network(
            spec,
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan.clone(),
        )
        .expect("exact provision plan should reserve");
    let manifest = backend
        .read_manifest(&id)
        .expect("planned PEP manifest should read")
        .expect("planned PEP manifest should exist");
    let pep_request = manifest
        .egress_proxy
        .as_ref()
        .expect("planned PEP assignment should persist")
        .port_lease
        .clone();
    assert_eq!(
        pep_request.lease_id(),
        &nimbus_network::PortLeaseId::for_listener(&expected_listener)
    );
    assert_eq!(pep_request.plan_id(), Some(network_plan.plan_id()));
    drop(pep_reservation);

    backend
        .start_planned_provision_pep(
            &manifest,
            manifest
                .require_reserved_claim()
                .expect("planned PEP manifest should retain its claim"),
        )
        .expect("compiler-planned PEP authority should start without a derived listener");
    let authority = LocalPortLeaseAuthority::open(network_root).expect("authority should open");
    let record = authority
        .inspect(pep_request.lease_id())
        .expect("planned PEP lease should inspect")
        .expect("planned PEP lease should remain durable");
    assert_eq!(record.request(), &pep_request);
    assert_eq!(record.phase(), PortLeasePhase::Active);
    assert!(
        backend
            .egress_proxies
            .readiness(&manifest.spec.tenant_id, &manifest.handle.id)
            .expect("planned PEP readiness should inspect")
            .is_some()
    );
}

#[test]
fn krun_attach_recovers_dead_compiler_planned_pep_owner() {
    let root = TempDir::new().expect("temporary root should exist");
    let pep_reservation = TcpListener::bind("127.0.0.1:0").expect("PEP tripwire should bind");
    let pep_port = pep_reservation
        .local_addr()
        .expect("PEP tripwire should report its port")
        .port();
    let mut config = KrunSandboxBackendConfig::under_root(root.path());
    config.node_network_supernet = "127.0.0.0/24".to_owned();
    config.published_port_range = pep_port..=pep_port;
    let backend_config = config.clone();
    let network_root = config.network_state_root.clone();
    let backend = KrunSandboxBackend::new(config)
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
    let id = SandboxId::new("krun-planned-pep-owner-recovery");
    let spec = sample_spec_for_tenant("krun-planned-pep-recovery", "api");
    let tenant_id = spec.tenant_id.clone();
    let network_plan = sample_provision_network_plan(&spec, &id, "planned-pep-owner-recovery");
    backend
        .reserve_provision_network(
            spec,
            id.clone(),
            sample_execution_attempt_id(&id),
            network_plan,
        )
        .expect("exact provision plan should reserve");
    backend
        .prepare_provision_workload(&id, &sample_execution_attempt_id(&id))
        .expect("provision workload should prepare before attachment");
    let mut manifest = backend
        .read_manifest(&id)
        .expect("prepared manifest should read")
        .expect("prepared manifest should exist");
    let reservation_claim = manifest
        .require_reserved_claim()
        .expect("prepared manifest should retain its reservation claim")
        .clone();
    backend
        .mark_attachment_adopting(&mut manifest)
        .expect("fixture should enter adoption intent");
    backend
        .persist_effect_barrier(&manifest, "test planned PEP adoption intent")
        .expect("adoption intent should persist");
    let network_config = manifest
        .require_network_config()
        .expect("prepared manifest should retain network config")
        .clone();
    backend
        .segment_allocator
        .adopt_reserved_attachment(
            &manifest.spec.tenant_id,
            &network_config.attachment_id,
            &reservation_claim,
        )
        .expect("fixture should adopt its exact attachment authority");
    manifest
        .mark_adopted()
        .expect("fixture should retain adopted launch authority");
    backend
        .persist_effect_barrier(&manifest, "test planned PEP adoption result")
        .expect("adopted authority should persist");
    {
        let ports = backend.port_lease_coordinator();
        let hostname = super::super::start::hostname_for(&manifest.spec);
        backend
            .non_routable_attachment_adapter(&manifest, &network_config, &hostname)
            .attach_with_test_host(
                &backend.attachment_lifecycle(&ports),
                AttachmentAttachAuthority::FreshLaunch(&reservation_claim),
                |_| {
                    backend.egress_pin_provider.apply(
                        &manifest.network_layout,
                        manifest
                            .egress_proxy
                            .as_ref()
                            .expect("planned PEP assignment should persist"),
                    )
                },
            )
            .expect("fixture should realize the exact private attachment");
    }
    let pep_request = manifest
        .egress_proxy
        .as_ref()
        .expect("planned PEP assignment should persist")
        .port_lease
        .clone();
    drop(pep_reservation);
    backend
        .start_planned_provision_pep(&manifest, &reservation_claim)
        .expect("initial compiler-planned PEP should start");
    let authority = LocalPortLeaseAuthority::open(&network_root).expect("authority should open");
    let first_generation = authority
        .inspect(pep_request.lease_id())
        .expect("initial PEP lease should inspect")
        .expect("initial PEP lease should exist")
        .active_lifetime()
        .expect("initial PEP should retain its process lifetime")
        .generation();
    drop(authority);
    drop(manifest);
    drop(backend);

    let dead_owner = LocalPortLeaseAuthority::open(&network_root)
        .expect("authority should reopen after process-owner loss")
        .inspect(pep_request.lease_id())
        .expect("dead-owner PEP lease should inspect")
        .expect("dead-owner PEP lease should remain durable");
    assert_eq!(dead_owner.phase(), PortLeasePhase::Active);
    assert_eq!(
        dead_owner
            .active_lifetime()
            .expect("dead-owner detection must reconcile the retained durable lifetime")
            .generation(),
        first_generation,
        "process death must not erase durable effect history before fenced reconciliation"
    );

    let restarted = KrunSandboxBackend::new(backend_config)
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
    let observed = restarted
        .attach_provision_network(&id, &sample_execution_attempt_id(&id))
        .expect("attachment replay should recover the exact dead planned PEP owner");
    assert!(matches!(
        observed,
        crate::SandboxProvisionPhaseObservation::Succeeded { .. }
    ));
    let recovered = LocalPortLeaseAuthority::open(network_root)
        .expect("authority should reopen after recovery")
        .inspect(pep_request.lease_id())
        .expect("recovered PEP lease should inspect")
        .expect("recovered PEP lease should remain durable");
    assert_eq!(recovered.request(), &pep_request);
    assert_eq!(recovered.phase(), PortLeasePhase::Active);
    assert!(
        recovered
            .active_lifetime()
            .expect("recovered PEP should retain a new process lifetime")
            .generation()
            > first_generation,
        "recovery must advance the exact stable lease's process generation"
    );
    assert!(
        restarted
            .egress_proxies
            .readiness(&tenant_id, &id)
            .expect("recovered PEP readiness should inspect")
            .is_some(),
        "recovery must register one ready compiler-planned PEP"
    );
}
