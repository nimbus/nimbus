//! Complete host-managed attachment readiness contract.

use super::*;
use crate::backends::oci::egress::{
    EgressProxyAssignment, EgressProxyRegistry, EgressReadinessFailure, EgressReadinessState,
    PepPreAdoptionReleaseAuthority, egress_decision_log_root, egress_proxy_assignment_for_test,
    egress_trust_anchor_root, ensure_egress_proxy_running_with_release_authority,
};
use crate::backends::oci::network::{
    FixedOciEgressPinProvider, OciAttachmentAuxiliaryListener, OciAttachmentReadinessFailure,
    OciAttachmentReadinessState, OciEgressPinObservation, OciEgressPinProvider,
};
use nimbus_egress::EgressPolicy;
use nimbus_network::{
    NetworkConditionKind, NetworkConditionState, NetworkPlan, NetworkPlanContentDigest,
    NetworkPlanId, NetworkProviderHandle, NetworkProviderId, NetworkResourceGeneration,
    NetworkResourcePhase, NetworkStateTransition, NetworkTransitionEvidence, PortExposure,
};

struct ReadinessFixture {
    base: ContractFixture,
    config: OciNetworkConfig,
    bindings: Vec<SandboxPortBinding>,
    leases: Vec<nimbus_network::PortLeaseRequest>,
    assignment: EgressProxyAssignment,
    registry: EgressProxyRegistry,
    pin: Arc<FixedOciEgressPinProvider>,
    policy: EgressPolicy,
    host: ContractHostEffects,
}

impl ReadinessFixture {
    fn active(backend: ContractBackend, row: &str) -> Self {
        let mut base = ContractFixture::new(backend, row);
        let pep_port = std::net::TcpListener::bind("127.0.0.1:0")
            .expect("ephemeral PEP port should bind")
            .local_addr()
            .expect("ephemeral PEP listener should report its address")
            .port();
        base.ports =
            OciPortLeaseCoordinator::new(&base.layout.network_state_root, pep_port..=pep_port);
        let config = base.reserve_and_adopt();
        let listener_ip = std::net::Ipv4Addr::LOCALHOST;
        let published_port = if pep_port == 32_000 { 32_001 } else { 32_000 };
        let desired = [SandboxPortBinding::tcp("http", published_port, 8080)];
        let mut reserved = base
            .ports
            .reserve_launch_ports_for_sandbox(
                SandboxLaunchPortPlan::new(&base.tenant_id, &base.sandbox_id, &desired, &[])
                    .with_internal_listener(InternalListenerReservation::new(
                        "egress-pep",
                        target_for_ip(listener_ip.into()).expect("loopback target should lower"),
                        PortExposure::Private,
                    )),
                &base.claim,
            )
            .expect("complete listener batch should reserve");
        reserved
            .confirm_manifest_published()
            .expect("complete listener batch should become durable");
        let assignment = egress_proxy_assignment_for_test(
            listener_ip.into(),
            reserved
                .internal_listener
                .take()
                .expect("complete listener batch should include its PEP"),
        );
        let bindings = reserved.published_bindings;
        let leases = reserved.published_leases;
        let pin = Arc::new(FixedOciEgressPinProvider::ready());
        let host = ContractHostEffects::default();

        base.backend
            .adapter(Self::input(&base, &config, &bindings, &leases, &assignment))
            .attach_with(
                &base.lifecycle(),
                AttachmentAttachAuthority::FreshLaunch(&base.claim),
                &host,
                &mut ContractPhaseObserver::recording(),
                |_| pin.apply(&base.layout, &assignment),
            )
            .expect("complete attachment should become Active");
        assert_eq!(
            pin.apply_count(),
            1,
            "initial attachment should apply the exact egress pin once"
        );

        let registry = EgressProxyRegistry::with_roots_and_network_state(
            egress_decision_log_root(&base.layout.workload_state_root),
            egress_trust_anchor_root(&base.layout.workload_state_root),
            &base.layout.network_state_root,
        );
        let policy = EgressPolicy::deny_all();
        ensure_egress_proxy_running_with_release_authority(
            &registry,
            &base.tenant_id,
            &base.sandbox_id,
            Some(&assignment),
            &policy,
            PepPreAdoptionReleaseAuthority::FreshLaunch(&base.claim),
        )
        .expect("exact PEP should become ready");

        Self {
            base,
            config,
            bindings,
            leases,
            assignment,
            registry,
            pin,
            policy,
            host,
        }
    }

    fn input<'a>(
        base: &'a ContractFixture,
        config: &'a OciNetworkConfig,
        bindings: &'a [SandboxPortBinding],
        leases: &'a [nimbus_network::PortLeaseRequest],
        assignment: &'a EgressProxyAssignment,
    ) -> OciAttachmentInput<'a> {
        OciAttachmentInput {
            workload_state_root: &base.layout.workload_state_root,
            tenant_id: &base.tenant_id,
            sandbox_id: &base.sandbox_id,
            display_name: "NNC5.3 readiness contract workload",
            hostname: "nnc53-readiness",
            bindings,
            leases,
            auxiliary_listener: Some(OciAttachmentAuxiliaryListener::egress_pep(
                &assignment.port_lease,
                &assignment.host,
                assignment.port,
            )),
            layout: &base.layout,
            config,
            launch_claim: Some(&base.claim),
        }
    }

    fn pep(&self) -> EgressReadinessState {
        self.registry
            .authenticated_readiness(
                &self.base.tenant_id,
                &self.base.sandbox_id,
                Some(&self.assignment),
                &self.policy,
                None,
            )
            .expect("PEP readiness should inspect")
    }

    fn inspect(&self) -> OciAttachmentReadinessState {
        self.inspect_with(
            &self.config,
            &self.bindings,
            &self.leases,
            Some(&self.assignment),
            self.pep(),
        )
    }

    fn inspect_with(
        &self,
        config: &OciNetworkConfig,
        bindings: &[SandboxPortBinding],
        leases: &[nimbus_network::PortLeaseRequest],
        assignment: Option<&EgressProxyAssignment>,
        pep: EgressReadinessState,
    ) -> OciAttachmentReadinessState {
        let fallback = &self.assignment;
        self.base
            .backend
            .adapter(Self::input(
                &self.base,
                config,
                bindings,
                leases,
                assignment.unwrap_or(fallback),
            ))
            .inspect_host_managed_readiness(
                &self.base.lifecycle(),
                self.pin.as_ref(),
                assignment,
                pep,
            )
    }

    fn durable_snapshot(
        &self,
    ) -> (
        Vec<nimbus_network::DurableNetworkAttachmentState>,
        Vec<nimbus_network::PortLeaseRecord>,
        Vec<u8>,
        Vec<u8>,
        usize,
    ) {
        (
            self.base
                .attachments
                .list()
                .expect("attachment authority should list"),
            self.base
                .ports
                .authority()
                .expect("port authority should open")
                .list()
                .expect("port authority should list"),
            std::fs::read(&self.base.layout.status_path)
                .expect("Netavark status evidence should read"),
            std::fs::read(&self.base.layout.netns_path).expect("namespace evidence should read"),
            self.pin.apply_count(),
        )
    }

    fn assert_allocator_read_only_since(&self, before: usize) {
        let operations = self.base.allocator.operations();
        assert!(
            operations[before..].iter().all(|operation| matches!(
                operation,
                SegmentAllocatorOperation::InspectAttachment(_, _)
            )),
            "readiness may inspect allocator authority but must not mutate it: {:?}",
            &operations[before..]
        );
    }
}

impl Drop for ReadinessFixture {
    fn drop(&mut self) {
        let _ = self.registry.stop_with_assignment(
            &self.base.tenant_id,
            &self.base.sandbox_id,
            Some(&self.assignment),
        );
    }
}

fn inspect_early_durable_state(
    base: &ContractFixture,
    config: &OciNetworkConfig,
    attachments: Option<&nimbus_network::LocalNetworkAttachmentAuthority>,
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    backend: ContractBackend,
) -> OciAttachmentReadinessState {
    let assignment = EgressProxyAssignment::for_test("127.0.0.1", 15_600);
    let adapter = backend.adapter(OciAttachmentInput {
        workload_state_root: &base.layout.workload_state_root,
        tenant_id,
        sandbox_id,
        display_name: "NNC5.3 durable identity row",
        hostname: "nnc53-durable-identity",
        bindings: &[],
        leases: &[],
        auxiliary_listener: None,
        layout: &base.layout,
        config,
        launch_claim: Some(&base.claim),
    });
    adapter.inspect_host_managed_readiness(
        &OciAttachmentLifecycle::new(
            &base.allocator,
            attachments,
            &base.ipam,
            &base.ports,
            &base.lifetimes,
        ),
        &FixedOciEgressPinProvider::ready(),
        Some(&assignment),
        EgressReadinessState::NotReady(EgressReadinessFailure::MissingRegistration),
    )
}

fn install_durable_record(
    base: &ContractFixture,
    plan: &NetworkPlan,
    selected_provider_id: NetworkProviderId,
    stable_handle: Option<NetworkProviderHandle>,
    final_phase: NetworkResourcePhase,
) {
    let association = base
        .allocator
        .inspect_attachment_reservation(
            &base.tenant_id,
            &default_network_attachment_id(&base.sandbox_id),
            &base.claim,
        )
        .expect("allocator association should inspect")
        .association()
        .expect("adopted allocator reservation should carry association")
        .clone();
    let mut record = base
        .attachments
        .reserve(
            &base.tenant_id,
            selected_provider_id,
            plan,
            default_network_attachment_id(&base.sandbox_id),
            association,
        )
        .expect("durable row should reserve");
    if final_phase == NetworkResourcePhase::Reserved {
        return;
    }
    for phase in [
        NetworkResourcePhase::Provisioning,
        NetworkResourcePhase::Ready,
        NetworkResourcePhase::Publishing,
        NetworkResourcePhase::Active,
    ] {
        if phase == NetworkResourcePhase::Ready
            && let Some(handle) = stable_handle.clone()
        {
            record = base
                .attachments
                .record_provider_handle(&base.tenant_id, record.resource().version(), handle)
                .expect("durable row should record provider handle")
                .1;
        }
        record = base
            .attachments
            .apply_transition(
                &base.tenant_id,
                &NetworkStateTransition::new(
                    record.resource().version().clone(),
                    phase,
                    NetworkTransitionEvidence::Progress,
                ),
            )
            .expect("durable row should advance")
            .1;
        if phase == final_phase {
            break;
        }
    }
}

#[test]
fn container_and_krun_emit_one_exact_portable_ready_observation_without_effects() {
    for backend in [ContractBackend::Container, ContractBackend::Krun] {
        let fixture = ReadinessFixture::active(backend, "portable-ready");
        let before = fixture.durable_snapshot();
        let allocator_before = fixture.base.allocator.operations().len();
        let OciAttachmentReadinessState::Ready(evidence) = fixture.inspect() else {
            panic!("complete {} attachment should be ready", backend.label());
        };
        let record = fixture
            .base
            .attachments
            .list()
            .expect("attachment authority should list")
            .pop()
            .expect("one attachment should exist");
        assert_eq!(
            evidence.observation().version(),
            record.resource().version()
        );
        assert_eq!(
            evidence.observation().observed_phase(),
            NetworkResourcePhase::Active
        );
        assert_eq!(
            evidence.observation().provider_id(),
            Some(record.selected_provider_id())
        );
        assert_eq!(
            evidence
                .observation()
                .conditions()
                .iter()
                .find(|condition| condition.kind() == NetworkConditionKind::Ready)
                .map(|condition| condition.state()),
            Some(NetworkConditionState::True)
        );
        assert_eq!(evidence.assigned_ips().len(), 1);
        fixture.assert_allocator_read_only_since(allocator_before);
        assert_eq!(
            fixture.durable_snapshot(),
            before,
            "ready inspection must not mutate any durable or provider evidence"
        );
    }
}

#[test]
fn pin_false_unknown_missing_assignment_and_pep_failure_are_named_and_read_only() {
    let fixture = ReadinessFixture::active(ContractBackend::Container, "facet-failures");
    let before = fixture.durable_snapshot();
    let allocator_before = fixture.base.allocator.operations().len();

    fixture
        .pin
        .set_observation(OciEgressPinObservation::NotReady {
            reason: "injected missing default-drop rule".to_owned(),
        });
    assert!(matches!(
        fixture.inspect(),
        OciAttachmentReadinessState::NotReady(OciAttachmentReadinessFailure::EgressPinNotReady(_))
    ));
    fixture
        .pin
        .set_observation(OciEgressPinObservation::Unknown {
            reason: "injected nft inspection failure".to_owned(),
        });
    assert!(matches!(
        fixture.inspect(),
        OciAttachmentReadinessState::NotReady(OciAttachmentReadinessFailure::EgressPinUnknown(_))
    ));
    fixture.pin.set_observation(OciEgressPinObservation::Ready);
    assert!(matches!(
        fixture.inspect_with(
            &fixture.config,
            &fixture.bindings,
            &fixture.leases,
            None,
            fixture.pep(),
        ),
        OciAttachmentReadinessState::NotReady(
            OciAttachmentReadinessFailure::MissingEgressProxyAssignment
        )
    ));
    assert!(matches!(
        fixture.inspect_with(
            &fixture.config,
            &fixture.bindings,
            &fixture.leases,
            Some(&fixture.assignment),
            EgressReadinessState::NotReady(EgressReadinessFailure::AuditUnhealthy),
        ),
        OciAttachmentReadinessState::NotReady(OciAttachmentReadinessFailure::PepNotReady(
            EgressReadinessFailure::AuditUnhealthy
        ))
    ));
    assert_eq!(
        fixture.durable_snapshot(),
        before,
        "every false or unknown readiness facet must preserve exact bytes"
    );
    fixture.assert_allocator_read_only_since(allocator_before);
}

#[test]
fn dropped_provider_artifacts_and_listener_lifetime_withdraw_readiness_without_mutation() {
    for artifact in ["status", "namespace"] {
        let fixture =
            ReadinessFixture::active(ContractBackend::Krun, &format!("dropped-{artifact}"));
        let before = fixture.durable_snapshot();
        let path = if artifact == "status" {
            &fixture.base.layout.status_path
        } else {
            &fixture.base.layout.netns_path
        };
        std::fs::remove_file(path).expect("provider artifact should be removable");
        assert!(matches!(
            fixture.inspect(),
            OciAttachmentReadinessState::NotReady(OciAttachmentReadinessFailure::ProviderNotReady(
                _
            ))
        ));
        assert_eq!(
            fixture
                .base
                .attachments
                .list()
                .expect("attachment authority should relist"),
            before.0,
            "provider-artifact loss must not mutate attachment authority"
        );
        assert_eq!(
            fixture
                .base
                .ports
                .authority()
                .expect("port authority should open")
                .list()
                .expect("port authority should relist"),
            before.1,
            "provider-artifact loss must not mutate listener authority"
        );
        assert_eq!(fixture.pin.apply_count(), before.4);
    }

    let fixture = ReadinessFixture::active(ContractBackend::Container, "lost-lifetime");
    let before = fixture.durable_snapshot();
    let batch = fixture
        .base
        .lifetimes
        .take(&fixture.base.tenant_id, &fixture.base.sandbox_id)
        .expect("lifetime registry should inspect")
        .expect("complete attachment should retain its lifetime");
    assert!(matches!(
        fixture.inspect(),
        OciAttachmentReadinessState::NotReady(
            OciAttachmentReadinessFailure::ListenerPublicationRejected(_)
        )
    ));
    assert_eq!(
        fixture.base.attachments.list().expect("authority relist"),
        before.0
    );
    assert_eq!(
        fixture
            .base
            .ports
            .authority()
            .expect("port authority should open")
            .list()
            .expect("port authority relist"),
        before.1
    );
    drop(batch);
}

#[test]
fn malformed_or_nonregular_provider_artifacts_fail_closed_without_normalization() {
    for shape in [
        "malformed-status",
        "status-directory",
        "namespace-directory",
    ] {
        let fixture = ReadinessFixture::active(ContractBackend::Container, shape);
        let target = if shape == "namespace-directory" {
            &fixture.base.layout.netns_path
        } else {
            &fixture.base.layout.status_path
        };
        std::fs::remove_file(target).expect("provider artifact should be replaceable");
        if shape == "malformed-status" {
            std::fs::write(target, b"{").expect("malformed status should write");
        } else {
            std::fs::create_dir(target).expect("nonregular provider artifact should create");
        }
        let authority_before = std::fs::read(fixture.base.attachments.authority_path())
            .expect("authority bytes should read");
        let state = fixture.inspect();
        assert!(
            matches!(
                state,
                OciAttachmentReadinessState::NotReady(
                    OciAttachmentReadinessFailure::ProviderNotReady(_)
                )
            ),
            "{shape} must fail closed: {state:?}"
        );
        assert_eq!(
            std::fs::read(fixture.base.attachments.authority_path())
                .expect("authority bytes should reread"),
            authority_before,
            "{shape} inspection must not normalize durable authority"
        );
    }
}

#[test]
fn explicit_empty_listener_set_is_ready_but_machine_publication_is_not_netavark_readiness() {
    let base = ContractFixture::new(ContractBackend::Container, "empty-listeners");
    let config = base.reserve_and_adopt();
    let pin = FixedOciEgressPinProvider::ready();
    let assignment = EgressProxyAssignment::for_test("127.92.0.1", 15_500);
    let adapter = base.host_adapter(ContractBackend::Container, &config, &[], &[]);
    adapter
        .attach_with(
            &base.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&base.claim),
            &ContractHostEffects::default(),
            &mut ContractPhaseObserver::recording(),
            |_| pin.apply(&base.layout, &assignment),
        )
        .expect("empty-listener attachment should activate");
    let state = adapter.inspect_host_managed_readiness(
        &base.lifecycle(),
        &pin,
        Some(&assignment),
        EgressReadinessState::NotReady(EgressReadinessFailure::MissingRegistration),
    );
    assert!(
        matches!(
            state,
            OciAttachmentReadinessState::NotReady(OciAttachmentReadinessFailure::PepNotReady(
                EgressReadinessFailure::MissingRegistration
            ))
        ),
        "empty listener readiness should reach the composed PEP decision: {state:?}"
    );

    let forwarder = OciMachinePortForwarderConfig::for_provider_instance(
        "127.0.0.1",
        80,
        "/",
        "provider",
        NetworkResourceGeneration::new(1),
    )
    .expect("machine provider config should validate");
    let machine = base.machine_adapter(&config, &forwarder, &[], &[]);
    assert!(matches!(
        machine.inspect_host_managed_readiness(
            &base.lifecycle(),
            &pin,
            Some(&assignment),
            EgressReadinessState::NotReady(EgressReadinessFailure::MissingRegistration),
        ),
        OciAttachmentReadinessState::NotReady(
            OciAttachmentReadinessFailure::UnsupportedPublicationMode
        )
    ));
}

#[test]
fn missing_wrong_phase_and_missing_manager_authority_are_named_and_preserve_bytes() {
    for (row, authority_present, install_reserved) in [
        ("missing-record", true, false),
        ("missing-manager", false, false),
        ("reserved-phase", true, true),
    ] {
        let base = ContractFixture::new(ContractBackend::Container, row);
        let config = base.reserve_and_adopt();
        if install_reserved {
            let plan = super::super::plan::oci_attachment_plan(
                &base.tenant_id,
                &base.sandbox_id,
                AttachmentBackendKind::Container,
            );
            let handle = super::super::plan::oci_attachment_provider_handle(
                &base.tenant_id,
                &base.sandbox_id,
                AttachmentBackendKind::Container,
            )
            .expect("stable handle should compile");
            install_durable_record(
                &base,
                &plan,
                handle.provider_id().clone(),
                Some(handle),
                NetworkResourcePhase::Reserved,
            );
        }
        let before = std::fs::read(base.attachments.authority_path())
            .expect("network authority bytes should read");
        let state = inspect_early_durable_state(
            &base,
            &config,
            authority_present.then_some(&base.attachments),
            &base.tenant_id,
            &base.sandbox_id,
            ContractBackend::Container,
        );
        match row {
            "missing-record" => assert!(matches!(
                state,
                OciAttachmentReadinessState::NotReady(
                    OciAttachmentReadinessFailure::MissingDurableAuthority
                )
            )),
            "missing-manager" => assert!(matches!(
                state,
                OciAttachmentReadinessState::NotReady(
                    OciAttachmentReadinessFailure::DurableAuthorityRejected(_)
                )
            )),
            "reserved-phase" => assert!(matches!(
                state,
                OciAttachmentReadinessState::NotReady(OciAttachmentReadinessFailure::DurablePhase(
                    NetworkResourcePhase::Reserved
                ))
            )),
            other => panic!("unexpected durable row {other}: {state:?}"),
        }
        assert_eq!(
            std::fs::read(base.attachments.authority_path())
                .expect("network authority bytes should reread"),
            before,
            "{row} inspection must not mutate the shared authority"
        );
    }
}

#[test]
fn plan_generation_digest_provider_and_handle_substitutions_fail_closed_without_mutation() {
    for row in [
        "plan-id",
        "generation",
        "digest",
        "selected-provider",
        "stable-handle",
    ] {
        let base = ContractFixture::new(ContractBackend::Container, row);
        let config = base.reserve_and_adopt();
        let expected = super::super::plan::oci_attachment_plan(
            &base.tenant_id,
            &base.sandbox_id,
            AttachmentBackendKind::Container,
        );
        let expected_handle = super::super::plan::oci_attachment_provider_handle(
            &base.tenant_id,
            &base.sandbox_id,
            AttachmentBackendKind::Container,
        )
        .expect("stable handle should compile");
        let plan = match row {
            "plan-id" => NetworkPlan::new(
                NetworkPlanId::for_tenant_workload_plan(&base.tenant_id, "substituted-plan"),
                expected.generation(),
                expected.content_digest(),
                expected.requirements().clone(),
            ),
            "generation" => NetworkPlan::new(
                expected.plan_id().clone(),
                NetworkResourceGeneration::new(expected.generation().as_u64() + 1),
                expected.content_digest(),
                expected.requirements().clone(),
            ),
            "digest" => NetworkPlan::new(
                expected.plan_id().clone(),
                expected.generation(),
                NetworkPlanContentDigest::sha256(b"substituted attachment content"),
                expected.requirements().clone(),
            ),
            _ => expected.clone(),
        };
        let selected_provider = if row == "selected-provider" {
            NetworkProviderId::for_registration_key("nnc53.substituted-provider")
        } else {
            expected_handle.provider_id().clone()
        };
        let stable_handle = if row == "selected-provider" {
            NetworkProviderHandle::new(selected_provider.clone(), "substituted-provider-handle")
                .expect("substituted provider handle should validate")
        } else if row == "stable-handle" {
            NetworkProviderHandle::new(
                expected_handle.provider_id().clone(),
                "substituted-stable-handle",
            )
            .expect("substituted stable handle should validate")
        } else {
            expected_handle
        };
        install_durable_record(
            &base,
            &plan,
            selected_provider,
            Some(stable_handle),
            NetworkResourcePhase::Active,
        );
        let before = std::fs::read(base.attachments.authority_path())
            .expect("network authority bytes should read");
        let state = inspect_early_durable_state(
            &base,
            &config,
            Some(&base.attachments),
            &base.tenant_id,
            &base.sandbox_id,
            ContractBackend::Container,
        );
        assert!(
            matches!(
                state,
                OciAttachmentReadinessState::NotReady(
                    OciAttachmentReadinessFailure::DurableAuthorityRejected(_)
                )
            ),
            "{row} substitution must produce a named durable rejection: {state:?}"
        );
        assert_eq!(
            std::fs::read(base.attachments.authority_path())
                .expect("network authority bytes should reread"),
            before,
            "{row} rejection must preserve exact authority bytes"
        );
    }
}

#[test]
fn tenant_attachment_association_and_epoch_substitutions_fail_before_effects() {
    for row in ["tenant", "attachment", "segment", "claim"] {
        let base = ContractFixture::new(ContractBackend::Krun, row);
        let mut config = base.reserve_and_adopt();
        let tenant = TenantId::new(format!("{}-other", base.tenant_id.as_str()))
            .expect("substituted tenant should validate");
        let sandbox = SandboxId::new(format!("{}-other", base.sandbox_id.as_str()));
        match row {
            "segment" => {
                config.segment_id = "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAW".to_owned();
            }
            "claim" => {
                config.reservation_claim = reservation_claim("substituted-epoch-claim");
            }
            _ => {}
        }
        let before = std::fs::read(base.attachments.authority_path())
            .expect("network authority bytes should read");
        let state = inspect_early_durable_state(
            &base,
            &config,
            Some(&base.attachments),
            if row == "tenant" {
                &tenant
            } else {
                &base.tenant_id
            },
            if row == "attachment" {
                &sandbox
            } else {
                &base.sandbox_id
            },
            ContractBackend::Krun,
        );
        assert!(
            matches!(
                state,
                OciAttachmentReadinessState::NotReady(
                    OciAttachmentReadinessFailure::InvalidContext(_)
                        | OciAttachmentReadinessFailure::DurableAuthorityRejected(_)
                )
            ),
            "{row} substitution must fail closed with a named reason: {state:?}"
        );
        assert_eq!(
            std::fs::read(base.attachments.authority_path())
                .expect("network authority bytes should reread"),
            before,
            "{row} rejection must preserve exact authority bytes"
        );
        assert_eq!(
            base.allocator
                .operations()
                .iter()
                .filter(|operation| matches!(
                    operation,
                    SegmentAllocatorOperation::Acquire(_, _)
                        | SegmentAllocatorOperation::Release(_, _)
                        | SegmentAllocatorOperation::Quarantine(_, _)
                        | SegmentAllocatorOperation::FinalizeRelease(_, _)
                ))
                .count(),
            0,
            "{row} readiness rejection must not mutate allocator authority"
        );
    }
}

#[test]
fn active_restart_reclaims_dead_listener_lifetime_and_reapplies_pin_without_provider_setup() {
    let fixture = ReadinessFixture::active(ContractBackend::Container, "active-restart");
    let attachment_before = fixture
        .base
        .attachments
        .list()
        .expect("attachment authority should list");
    let provider_setups_before = fixture
        .host
        .operations()
        .iter()
        .filter(|operation| **operation == ContractHostOperation::ProviderSetup)
        .count();
    assert_eq!(
        provider_setups_before, 1,
        "initial attach should execute exactly one provider setup"
    );
    let active_adapter = fixture.base.backend.adapter(ReadinessFixture::input(
        &fixture.base,
        &fixture.config,
        &fixture.bindings,
        &fixture.leases,
        &fixture.assignment,
    ));
    active_adapter
        .attach_with(
            &fixture.base.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.base.claim),
            &fixture.host,
            &mut ContractPhaseObserver::recording(),
            |_| fixture.pin.apply(&fixture.base.layout, &fixture.assignment),
        )
        .expect("the exact retained launch claim should authenticate Active replay");
    assert_eq!(
        fixture
            .host
            .operations()
            .iter()
            .filter(|operation| **operation == ContractHostOperation::ProviderSetup)
            .count(),
        provider_setups_before,
        "exact FreshLaunch replay must not execute another Netavark setup"
    );
    let foreign_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()
        .expect("foreign launch claim should mint");
    let adapter = fixture.base.backend.adapter(ReadinessFixture::input(
        &fixture.base,
        &fixture.config,
        &fixture.bindings,
        &fixture.leases,
        &fixture.assignment,
    ));
    let pin_applications_before = fixture.pin.apply_count();
    adapter
        .attach_with(
            &fixture.base.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&foreign_claim),
            &fixture.host,
            &mut ContractPhaseObserver::recording(),
            |_| fixture.pin.apply(&fixture.base.layout, &fixture.assignment),
        )
        .expect_err("an unrelated launch claim must fail before Active reconciliation effects");
    assert_eq!(
        fixture.pin.apply_count(),
        pin_applications_before,
        "rejected Active authority must fail before pin application"
    );

    let first_record = fixture
        .base
        .ports
        .authority()
        .expect("port authority should open")
        .inspect(fixture.leases[0].lease_id())
        .expect("listener should inspect")
        .expect("listener should remain durable");
    let first_lifetime_generation = first_record
        .active_lifetime()
        .expect("initial listener should retain a live process lifetime")
        .generation()
        .as_u64();
    let dead_owner = fixture
        .base
        .lifetimes
        .take(&fixture.base.tenant_id, &fixture.base.sandbox_id)
        .expect("lifetime registry should inspect")
        .expect("initial attachment should retain its lifetime batch");
    drop(dead_owner);

    let mut restart_input = ReadinessFixture::input(
        &fixture.base,
        &fixture.config,
        &fixture.bindings,
        &fixture.leases,
        &fixture.assignment,
    );
    restart_input.launch_claim = None;
    let restart_adapter = fixture.base.backend.adapter(restart_input);
    let recovered_ips = restart_adapter
        .attach_with(
            &fixture.base.lifecycle(),
            AttachmentAttachAuthority::RestartRetained,
            &fixture.host,
            &mut ContractPhaseObserver::recording(),
            |_| fixture.pin.apply(&fixture.base.layout, &fixture.assignment),
        )
        .expect("exact Active provider presence should recover the dead listener owner");
    assert_eq!(recovered_ips.len(), 1);
    assert_eq!(
        fixture.pin.apply_count(),
        pin_applications_before + 1,
        "restart recovery should reapply the exact pin once"
    );
    assert_eq!(
        fixture
            .host
            .operations()
            .iter()
            .filter(|operation| **operation == ContractHostOperation::ProviderSetup)
            .count(),
        provider_setups_before,
        "Active recovery must not execute a second Netavark setup"
    );
    assert_eq!(
        fixture
            .base
            .attachments
            .list()
            .expect("attachment authority should relist"),
        attachment_before,
        "Active recovery must preserve the exact attachment version and phase"
    );
    let recovered_record = fixture
        .base
        .ports
        .authority()
        .expect("port authority should reopen")
        .inspect(fixture.leases[0].lease_id())
        .expect("recovered listener should inspect")
        .expect("recovered listener should remain durable");
    let recovered_generation = recovered_record
        .active_lifetime()
        .expect("recovered listener should retain its new process lifetime")
        .generation()
        .as_u64();
    assert!(
        recovered_generation > first_lifetime_generation,
        "dead-owner recovery must fence the old process lifetime generation"
    );
    assert!(
        fixture.inspect().is_ready(),
        "the recovered exact attachment should immediately satisfy readiness"
    );

    restart_adapter
        .attach_with(
            &fixture.base.lifecycle(),
            AttachmentAttachAuthority::RestartRetained,
            &fixture.host,
            &mut ContractPhaseObserver::recording(),
            |_| fixture.pin.apply(&fixture.base.layout, &fixture.assignment),
        )
        .expect("same-generation Active replay should remain idempotent");
    let replayed_record = fixture
        .base
        .ports
        .authority()
        .expect("port authority should reopen")
        .inspect(fixture.leases[0].lease_id())
        .expect("replayed listener should inspect")
        .expect("replayed listener should remain durable");
    assert_eq!(
        replayed_record.active_lifetime(),
        recovered_record.active_lifetime(),
        "same-process replay must retain the recovered lifetime generation"
    );
    assert_eq!(
        fixture
            .host
            .operations()
            .iter()
            .filter(|operation| **operation == ContractHostOperation::ProviderSetup)
            .count(),
        provider_setups_before,
        "idempotent replay must not execute Netavark setup"
    );
    assert_eq!(
        fixture.pin.apply_count(),
        pin_applications_before + 2,
        "idempotent replay should reapply, not duplicate, the exact pin ruleset"
    );
}

#[test]
fn same_generation_reopen_produces_the_same_complete_readiness_result() {
    for backend in [ContractBackend::Container, ContractBackend::Krun] {
        let fixture = ReadinessFixture::active(backend, "same-generation-reopen");
        let first = fixture.inspect();
        let second = fixture.inspect();
        assert_eq!(
            second,
            first,
            "{} same-generation reopen must reproduce the exact observation",
            backend.label()
        );
        assert_eq!(
            fixture.pin.apply_count(),
            1,
            "read-only reopen must not reapply the pin"
        );
    }
}

#[test]
fn syntactic_status_json_cannot_substitute_for_the_exact_provider_attempt() {
    let fixture =
        ReadinessFixture::active(ContractBackend::Container, "substituted-provider-status");
    let exact =
        std::fs::read(&fixture.base.layout.status_path).expect("exact provider status should read");
    let foreign = ReadinessFixture::active(ContractBackend::Container, "foreign-provider-status");
    let foreign_projection = std::fs::read(&foreign.base.layout.status_path)
        .expect("foreign exact provider status should read");
    let exact_value: serde_json::Value =
        serde_json::from_slice(&exact).expect("exact provider status should parse");
    let mut wrong_schema = exact_value.clone();
    wrong_schema["schema_version"] = serde_json::json!(2);
    let mut wrong_addresses = exact_value.clone();
    wrong_addresses["assigned_ips"] = serde_json::json!(["127.0.0.254"]);
    let mut unknown_field = exact_value;
    unknown_field["untrusted_hint"] = serde_json::json!("current");

    for (label, substitute) in [
        ("syntactic JSON", b"{}".to_vec()),
        ("foreign exact projection", foreign_projection),
        (
            "unsupported schema",
            serde_json::to_vec(&wrong_schema).expect("wrong schema should serialize"),
        ),
        (
            "wrong assigned addresses",
            serde_json::to_vec(&wrong_addresses).expect("wrong addresses should serialize"),
        ),
        (
            "unknown projection field",
            serde_json::to_vec(&unknown_field).expect("unknown field should serialize"),
        ),
    ] {
        std::fs::write(&fixture.base.layout.status_path, substitute)
            .expect("provider status substitute should write");
        assert!(
            matches!(
                fixture.inspect(),
                OciAttachmentReadinessState::NotReady(
                    OciAttachmentReadinessFailure::ProviderNotReady(_)
                )
            ),
            "{label} must not authenticate the current Netavark setup attempt"
        );
    }
    std::fs::write(&fixture.base.layout.status_path, exact)
        .expect("exact provider status should restore");
    assert!(
        fixture.inspect().is_ready(),
        "restoring exact attempt-bound provider status should restore readiness"
    );
}
