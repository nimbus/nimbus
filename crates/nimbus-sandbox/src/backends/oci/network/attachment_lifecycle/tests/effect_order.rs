//! NNC5.2a fail-before proofs for durable attempt ordering.

use nimbus_network::{
    LocalNetworkStateStore, NetworkAttachmentReservationState, NetworkAttachmentSegmentAssociation,
    NetworkLeaseEpoch, NetworkSegmentId,
};

use super::*;
use crate::backends::oci::network::attachment_lifecycle::recovery::AttachmentProviderObservation;
use crate::backends::oci::network::dto::NetavarkProviderOperation;
use crate::backends::oci::network::netavark::{
    PreparedNetavarkSetup, PreparedNetavarkTeardown,
    execute_prepared_container_network_teardown_for_test, prepare_container_network_setup,
    prepare_container_network_teardown,
};

struct AttemptBeforeEffectHost;

impl AttachmentHostEffects for AttemptBeforeEffectHost {
    fn inspect_provider(
        &self,
        _ipam: &OciIpamAuthority,
        _context: &OciAttachmentContext<'_>,
    ) -> AttachmentProviderObservation {
        AttachmentProviderObservation::Absent
    }

    fn create_namespace(&self, context: &OciAttachmentContext<'_>) -> Result<()> {
        let attachment_id = default_network_attachment_id(context.sandbox_id);
        let attachment = nimbus_network::LocalNetworkAttachmentAuthority::open(
            &context.layout.network_state_root,
        )
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "NNC5.2a durable attachment authority must reopen before the first effect: \
                         {error}"
            ),
        })?
        .get(context.tenant_id, &attachment_id)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "NNC5.2a durable attachment association must inspect before the first \
                         effect: {error}"
            ),
        })?
        .ok_or_else(|| SandboxError::OperationFailed {
            message: "NNC5.2a durable attachment association is missing before the first \
                              effect"
                .to_owned(),
        })?;
        if attachment.association().reservation_claim() != &context.config.reservation_claim
            || attachment.association().segment_id().as_str() != context.config.segment_id
            || attachment.resource().phase() != nimbus_network::NetworkResourcePhase::Provisioning
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "NNC5.2a exact attachment association and Provisioning phase must be durable \
                     before namespace creation, got {:?}/{:?}",
                    attachment.association(),
                    attachment.resource().phase()
                ),
            });
        }
        let operation = inspect_netavark_provider_operation(
            &direct_test_ipam_authority(context.layout),
            context.layout,
            context.config,
            context.sandbox_id,
        )?;
        if !matches!(operation, NetavarkProviderOperation::SetupPrepared { .. }) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "NNC5.2a provider attempt must be durable before namespace creation, got {}",
                    operation.label()
                ),
            });
        }
        Ok(())
    }

    fn prepare_provider_setup(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<PreparedNetavarkSetup> {
        prepare_container_network_setup(ipam, &context.operation())
    }

    fn setup_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
        prepared: PreparedNetavarkSetup,
    ) -> Result<Vec<std::net::Ipv4Addr>> {
        let assigned_ips = prepared.assigned_ips().to_vec();
        begin_netavark_setup_execution(
            ipam,
            context.layout,
            context.config,
            context.sandbox_id,
            prepared.claim(),
        )?;
        complete_netavark_setup(ipam, context.layout, prepared.claim())?;
        Ok(assigned_ips)
    }

    fn teardown_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
        prepared: PreparedNetavarkTeardown,
    ) -> Result<()> {
        execute_prepared_container_network_teardown_for_test(ipam, context.layout, prepared)
    }

    fn prepare_provider_teardown(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<PreparedNetavarkTeardown> {
        prepare_container_network_teardown(ipam, &context.operation())
    }

    fn remove_namespace(&self, _context: &OciAttachmentContext<'_>) -> Result<()> {
        Ok(())
    }
}

struct ForbiddenHostEffects;

impl AttachmentHostEffects for ForbiddenHostEffects {
    fn inspect_provider(
        &self,
        _ipam: &OciIpamAuthority,
        _context: &OciAttachmentContext<'_>,
    ) -> AttachmentProviderObservation {
        panic!("association substitution must fail before provider inspection")
    }

    fn create_namespace(&self, _context: &OciAttachmentContext<'_>) -> Result<()> {
        panic!("association substitution must fail before namespace creation")
    }

    fn prepare_provider_setup(
        &self,
        _ipam: &OciIpamAuthority,
        _context: &OciAttachmentContext<'_>,
    ) -> Result<PreparedNetavarkSetup> {
        panic!("association substitution must fail before setup preparation")
    }

    fn setup_provider(
        &self,
        _ipam: &OciIpamAuthority,
        _context: &OciAttachmentContext<'_>,
        _prepared: PreparedNetavarkSetup,
    ) -> Result<Vec<std::net::Ipv4Addr>> {
        panic!("association substitution must fail before provider setup")
    }

    fn prepare_provider_teardown(
        &self,
        _ipam: &OciIpamAuthority,
        _context: &OciAttachmentContext<'_>,
    ) -> Result<PreparedNetavarkTeardown> {
        panic!("association substitution must fail before teardown preparation")
    }

    fn teardown_provider(
        &self,
        _ipam: &OciIpamAuthority,
        _context: &OciAttachmentContext<'_>,
        _prepared: PreparedNetavarkTeardown,
    ) -> Result<()> {
        panic!("association substitution must fail before provider teardown")
    }

    fn remove_namespace(&self, _context: &OciAttachmentContext<'_>) -> Result<()> {
        panic!("association substitution must fail before namespace cleanup")
    }
}

fn first_effect_requires_persisted_association_and_provider_attempt(backend: ContractBackend) {
    let fixture = ContractFixture::new(backend, "nnc52a-first-effect-order");
    let config = fixture.reserve_and_adopt();
    assert_eq!(
        fixture.attachment_state(&fixture.claim),
        NetworkAttachmentReservationState::Adopted,
        "the allocator association must already be adopted"
    );

    let mut observer = ContractPhaseObserver::recording();
    fixture
        .host_adapter(backend, &config, &[], &[])
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &AttemptBeforeEffectHost,
            &mut observer,
            |_| Ok(()),
        )
        .expect(
            "exact durable association and provider attempt must precede the first host effect",
        );
}

#[test]
fn container_first_effect_requires_persisted_association_and_provider_attempt() {
    first_effect_requires_persisted_association_and_provider_attempt(ContractBackend::Container);
}

#[test]
fn krun_first_effect_requires_persisted_association_and_provider_attempt() {
    first_effect_requires_persisted_association_and_provider_attempt(ContractBackend::Krun);
}

#[derive(Debug, Clone, Copy)]
enum AssociationSubstitution {
    Claim,
    Segment,
    Epoch,
}

#[test]
fn association_substitution_fails_before_inspection_or_effects_for_both_backends() {
    for backend in [ContractBackend::Container, ContractBackend::Krun] {
        for substitution in [
            AssociationSubstitution::Claim,
            AssociationSubstitution::Segment,
            AssociationSubstitution::Epoch,
        ] {
            let fixture =
                ContractFixture::new(backend, &format!("nnc52a-association-{substitution:?}"));
            let config = fixture.reserve_and_adopt();
            let mut initial = ContractPhaseObserver::recording();
            fixture
                .host_adapter(backend, &config, &[], &[])
                .attach_with(
                    &fixture.lifecycle(),
                    AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
                    &ContractHostEffects::default(),
                    &mut initial,
                    |_| Ok(()),
                )
                .expect("the exact association must establish the baseline attachment");

            let attachment_id = default_network_attachment_id(&fixture.sandbox_id);
            let exact = fixture
                .allocator
                .inspect_attachment_reservation(&fixture.tenant_id, &attachment_id, &fixture.claim)
                .expect("the exact allocator association should inspect")
                .association()
                .expect("the adopted attachment must carry its association")
                .clone();
            let substituted = match substitution {
                AssociationSubstitution::Claim => NetworkAttachmentSegmentAssociation::new(
                    reservation_claim("nnc52a-foreign-association"),
                    exact.segment_id().clone(),
                    exact.lease_epoch(),
                ),
                AssociationSubstitution::Segment => NetworkAttachmentSegmentAssociation::new(
                    exact.reservation_claim().clone(),
                    "netsegment_01ARZ3NDEKTSV4RRFFQ69G5FAW"
                        .parse::<NetworkSegmentId>()
                        .expect("foreign segment identity should validate"),
                    exact.lease_epoch(),
                ),
                AssociationSubstitution::Epoch => NetworkAttachmentSegmentAssociation::new(
                    exact.reservation_claim().clone(),
                    exact.segment_id().clone(),
                    NetworkLeaseEpoch::new(exact.lease_epoch().as_u64() + 1),
                ),
            };
            fixture
                .allocator
                .substitute_observed_association_for_test(substituted);
            let authority_path =
                LocalNetworkStateStore::authority_path_for(fixture._temp_dir.path());
            let authority_before =
                std::fs::read(&authority_path).expect("baseline authority bytes should read");
            let allocator_before = fixture.allocator.operations();
            let mut rejected = ContractPhaseObserver::recording();

            let error = fixture
                .host_adapter(backend, &config, &[], &[])
                .attach_with(
                    &fixture.lifecycle(),
                    AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
                    &ForbiddenHostEffects,
                    &mut rejected,
                    |_| panic!("association substitution must fail before backend publication"),
                )
                .expect_err("a substituted association must fail closed");
            assert!(
                matches!(error, SandboxError::OperationFailed { .. }),
                "{backend:?}/{substitution:?} must return a typed operation failure: {error}"
            );
            assert_eq!(
                std::fs::read(&authority_path).expect("rejected authority bytes should reread"),
                authority_before,
                "{backend:?}/{substitution:?} must preserve allocator, IPAM, and attachment bytes"
            );
            let allocator_after = fixture.allocator.operations();
            assert_eq!(
                &allocator_after[..allocator_before.len()],
                allocator_before.as_slice()
            );
            assert!(
                matches!(
                    allocator_after.get(allocator_before.len()),
                    Some(SegmentAllocatorOperation::InspectAttachment(..))
                ) && allocator_after.len() == allocator_before.len() + 1,
                "{backend:?}/{substitution:?} may perform only exact read-only allocator \
                 inspection before rejection: {allocator_after:?}"
            );
        }
    }
}

#[test]
fn container_machine_forwarding_cannot_bypass_attachment_attempt_authority() {
    let fixture = ContractFixture::new(
        ContractBackend::Container,
        "nnc52a-machine-forwarded-attempt-order",
    );
    let config = fixture.reserve_and_adopt();
    let forwarder = OciMachinePortForwarderConfig::for_provider_instance(
        "127.0.0.1",
        1,
        "/nnc52a-machine-forwarder",
        "nnc52a-machine-forwarder-instance",
        NetworkResourceGeneration::new(1),
    )
    .expect("machine forwarder identity should validate");
    let adapter = fixture.machine_adapter(&config, &forwarder, &[], &[]);
    let attach_host = ContractHostEffects::default();
    let mut attach_observer = ContractPhaseObserver::recording();
    adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &attach_host,
            &mut attach_observer,
            |_| {
                assert!(matches!(
                    inspect_netavark_provider_operation(
                        &fixture.ipam,
                        &fixture.layout,
                        &config,
                        &fixture.sandbox_id,
                    )
                    .expect("machine publication setup attempt should inspect"),
                    NetavarkProviderOperation::Ready { .. }
                ));
                let attachment = fixture
                    .attachments
                    .get(
                        &fixture.tenant_id,
                        &default_network_attachment_id(&fixture.sandbox_id),
                    )
                    .expect("machine publication attachment should inspect")
                    .expect("machine publication attachment should be durable");
                assert_eq!(
                    attachment.resource().phase(),
                    nimbus_network::NetworkResourcePhase::Publishing,
                    "machine publication may begin only after portable provider readiness"
                );
                Ok(())
            },
        )
        .expect("machine-forwarded attach must use the shared attempt authority");
    assert_eq!(
        attach_host.operations(),
        vec![
            ContractHostOperation::ProviderAttemptPrepared,
            ContractHostOperation::NamespaceCreated,
            ContractHostOperation::ProviderSetup,
        ],
        "machine publication must follow the shared association/setup attempt lifecycle"
    );

    let first_host = ContractHostEffects::default();
    let failure = adapter
        .detach_machine_forwarded_with(
            &fixture.lifecycle(),
            AttachmentTeardownMode::Restart,
            &first_host,
            || {
                assert!(matches!(
                    inspect_netavark_provider_operation(
                        &fixture.ipam,
                        &fixture.layout,
                        &config,
                        &fixture.sandbox_id,
                    )
                    .expect("machine cleanup attempt should inspect"),
                    NetavarkProviderOperation::TeardownPrepared { .. }
                ));
                let attachment = fixture
                    .attachments
                    .get(
                        &fixture.tenant_id,
                        &default_network_attachment_id(&fixture.sandbox_id),
                    )
                    .expect("machine cleanup attachment should inspect")
                    .expect("machine cleanup attachment should be durable");
                assert_eq!(
                    attachment.resource().phase(),
                    nimbus_network::NetworkResourcePhase::Deleting,
                    "machine cleanup may begin only after portable deleting authority"
                );
                Err(SandboxError::OperationFailed {
                    message: "injected machine cleanup ambiguity".to_owned(),
                })
            },
            |()| panic!("ambiguous machine cleanup must not complete provider teardown"),
        )
        .expect_err("ambiguous machine cleanup must retain exact retry authority");
    assert_eq!(
        failure.stage(),
        AttachmentDetachFailureStage::BeforeProviderDetach
    );
    let failure_message = failure.into_error().to_string();
    assert!(
        failure_message.contains("injected machine cleanup ambiguity"),
        "machine cleanup failure must preserve its primary diagnostic: {failure_message}"
    );
    assert_eq!(
        first_host.operations(),
        vec![ContractHostOperation::ProviderTeardownPrepared],
        "machine ambiguity must not execute or duplicate the prepared provider effect"
    );
    assert!(matches!(
        inspect_netavark_provider_operation(
            &fixture.ipam,
            &fixture.layout,
            &config,
            &fixture.sandbox_id,
        )
        .expect("retained machine teardown attempt should inspect"),
        NetavarkProviderOperation::TeardownPrepared { .. }
    ));
    assert_eq!(
        fixture
            .attachments
            .get(
                &fixture.tenant_id,
                &default_network_attachment_id(&fixture.sandbox_id),
            )
            .expect("retained machine attachment should inspect")
            .expect("retained machine attachment should remain durable")
            .resource()
            .phase(),
        nimbus_network::NetworkResourcePhase::CleanupPending
    );

    let retry_host = ContractHostEffects::default();
    adapter
        .detach_machine_forwarded_with(
            &fixture.lifecycle(),
            AttachmentTeardownMode::Restart,
            &retry_host,
            || Ok(()),
            |()| Ok(()),
        )
        .expect("machine retry must resume the same durable teardown attempt");
    assert_eq!(
        retry_host.operations(),
        vec![
            ContractHostOperation::ProviderTeardownPrepared,
            ContractHostOperation::ProviderTeardown,
            ContractHostOperation::NamespaceRemoved,
        ],
        "machine retry must prepare, execute, and confirm the one shared provider attempt"
    );
}

#[test]
fn container_and_krun_delete_effects_require_the_exact_persisted_attempt() {
    for backend in [ContractBackend::Container, ContractBackend::Krun] {
        let fixture = ContractFixture::new(backend, "nnc52a-delete-requires-persisted-attempt");
        let config = fixture.reserve_and_adopt();
        let adapter = fixture.host_adapter(backend, &config, &[], &[]);
        let mut attach_observer = ContractPhaseObserver::recording();
        adapter
            .attach_with(
                &fixture.lifecycle(),
                AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
                &ContractHostEffects::default(),
                &mut attach_observer,
                |_| Ok(()),
            )
            .expect("delete-order fixture should attach");

        let delete_host = ContractHostEffects::default();
        adapter
            .detach_host_managed_with(
                &fixture.lifecycle(),
                AttachmentTeardownMode::Restart,
                &delete_host,
                |_| {
                    assert_eq!(
                        delete_host.operations(),
                        vec![ContractHostOperation::ProviderTeardownPrepared],
                        "{backend:?} cleanup callback must follow durable attempt preparation"
                    );
                    assert!(matches!(
                        inspect_netavark_provider_operation(
                            &fixture.ipam,
                            &fixture.layout,
                            &config,
                            &fixture.sandbox_id,
                        )
                        .expect("prepared delete attempt should inspect"),
                        NetavarkProviderOperation::TeardownPrepared { .. }
                    ));
                    let attachment = fixture
                        .attachments
                        .get(
                            &fixture.tenant_id,
                            &default_network_attachment_id(&fixture.sandbox_id),
                        )
                        .expect("deleting attachment should inspect")
                        .expect("deleting attachment should remain durable");
                    assert_eq!(
                        attachment.resource().phase(),
                        nimbus_network::NetworkResourcePhase::Deleting,
                        "{backend:?} delete callback requires portable deleting authority"
                    );
                    Ok(())
                },
            )
            .expect("the exact prepared delete attempt should execute once");
        assert_eq!(
            delete_host.operations(),
            vec![
                ContractHostOperation::ProviderTeardownPrepared,
                ContractHostOperation::ProviderTeardown,
                ContractHostOperation::NamespaceRemoved,
            ],
            "{backend:?} must not execute delete outside the shared prepared-attempt seam"
        );
    }
}

#[test]
fn prepared_provider_attempt_is_reused_after_fresh_reopen_without_duplicate_effect() {
    let fixture =
        ContractFixture::new(ContractBackend::Container, "nnc52a-prepared-attempt-reopen");
    let config = fixture.reserve_and_adopt();
    let first_host = ContractHostEffects::default();
    let mut crash_cut =
        ContractPhaseObserver::failing_at(AttachmentAttachPhase::ProviderAttemptAuthenticated);

    let error = fixture
        .host_adapter(ContractBackend::Container, &config, &[], &[])
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &first_host,
            &mut crash_cut,
            |_| Ok(()),
        )
        .expect_err("the deterministic cut must follow the durable setup attempt");
    assert!(
        error.to_string().contains("ProviderAttemptAuthenticated"),
        "the fail-before cut must identify its exact durable boundary: {error}"
    );
    assert_eq!(
        first_host.operations(),
        vec![ContractHostOperation::ProviderAttemptPrepared],
        "the cut must precede namespace and provider effects"
    );
    let original_attempt = match inspect_netavark_provider_operation(
        &fixture.ipam,
        &fixture.layout,
        &config,
        &fixture.sandbox_id,
    )
    .expect("the prepared setup attempt must inspect")
    {
        NetavarkProviderOperation::SetupPrepared { operation_attempt } => operation_attempt,
        operation => panic!(
            "the crash cut must retain one provisioning attempt, got {}",
            operation.label()
        ),
    };

    let reopened_attachments = LocalNetworkAttachmentAuthority::open(fixture._temp_dir.path())
        .expect("a fresh attachment authority should reopen");
    let reopened_ipam = direct_test_ipam_authority(&fixture.layout);
    let reopened_ports = OciPortLeaseCoordinator::new(fixture._temp_dir.path(), 32_000..=32_099);
    let reopened_lifetimes = NetavarkPortLifetimeRegistry::default();
    let reopened_lifecycle = OciAttachmentLifecycle::new(
        &fixture.allocator,
        Some(&reopened_attachments),
        &reopened_ipam,
        &reopened_ports,
        &reopened_lifetimes,
    );
    let retry_host = ContractHostEffects::default();
    let mut retry = ContractPhaseObserver::recording();
    fixture
        .host_adapter(ContractBackend::Container, &config, &[], &[])
        .attach_with(
            &reopened_lifecycle,
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &retry_host,
            &mut retry,
            |_| Ok(()),
        )
        .expect("a fresh owner must resume the exact prepared attempt");

    assert_eq!(
        retry_host.operations(),
        vec![
            ContractHostOperation::ProviderAttemptPrepared,
            ContractHostOperation::NamespaceCreated,
            ContractHostOperation::ProviderSetup,
        ],
        "reopen must execute the retained attempt exactly once"
    );
    let completed_attempt = match inspect_netavark_provider_operation(
        &reopened_ipam,
        &fixture.layout,
        &config,
        &fixture.sandbox_id,
    )
    .expect("completed provider authority must inspect")
    {
        NetavarkProviderOperation::Ready { setup_attempt } => setup_attempt,
        operation => panic!(
            "the resumed attempt must become ready, got {}",
            operation.label()
        ),
    };
    assert_eq!(
        completed_attempt, original_attempt,
        "fresh reopen must not mint a duplicate setup attempt"
    );
}

#[test]
fn final_detach_reopens_after_allocator_finalization_before_portable_completion() {
    for backend in [ContractBackend::Container, ContractBackend::Krun] {
        let fixture = ContractFixture::new(backend, "nnc52a-finalize-before-portable-completion");
        let config = fixture.reserve_and_adopt();
        let adapter = fixture.host_adapter(backend, &config, &[], &[]);
        let mut attach_observer = ContractPhaseObserver::recording();
        adapter
            .attach_with(
                &fixture.lifecycle(),
                AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
                &ContractHostEffects::default(),
                &mut attach_observer,
                |_| Ok(()),
            )
            .expect("crash-window fixture should attach");
        adapter
            .detach_host_managed_with(
                &fixture.lifecycle(),
                AttachmentTeardownMode::Restart,
                &ContractHostEffects::default(),
                |_| Ok(()),
            )
            .expect("restart detach should retain allocation while confirming provider absence");

        let attachment_id = default_network_attachment_id(&fixture.sandbox_id);
        let association = fixture
            .allocator
            .inspect_attachment_reservation(&fixture.tenant_id, &attachment_id, &fixture.claim)
            .expect("retained association should inspect")
            .association()
            .expect("restart-retained allocation should keep its exact association")
            .clone();
        let durable = state::OciAttachmentDurableState::compile(
            Some(&fixture.attachments),
            &adapter.context,
            association,
        )
        .expect("portable attachment state should compile");
        let record = durable
            .inspect()
            .expect("portable attachment should inspect")
            .expect("portable attachment should remain durable");
        let deleting =
            recovery::prepare_detach(&durable, record, AttachmentProviderObservation::Absent)
                .expect("confirmed provider absence should prepare final deletion")
                .record;
        assert_eq!(
            deleting.resource().phase(),
            nimbus_network::NetworkResourcePhase::Deleting
        );

        fixture
            .allocator
            .quarantine(&fixture.tenant_id, &attachment_id, Some(&fixture.claim))
            .expect("exact association should quarantine");
        crate::backends::oci::network::deallocate_container_ips_after_confirmed_detach(
            &fixture.ipam,
            &fixture.layout,
            &fixture.sandbox_id,
            &fixture.claim,
        )
        .expect("exact provider-absent IPAM should become a terminal witness");
        let cleanup = match fixture
            .allocator
            .release(&fixture.tenant_id, &attachment_id, Some(&fixture.claim))
            .expect("exact quarantined hold should release")
        {
            nimbus_network::NetworkSegmentReleaseOutcome::CleanupPending(cleanup) => cleanup,
            outcome => panic!("last held attachment must produce exact cleanup, got {outcome:?}"),
        };
        fixture
            .allocator
            .finalize_release(&cleanup)
            .expect("exact allocation finalization should succeed");
        assert_eq!(
            fixture
                .allocator
                .inspect_attachment_reservation(&fixture.tenant_id, &attachment_id, &fixture.claim,)
                .expect("post-finalization allocator state should inspect")
                .state(),
            NetworkAttachmentReservationState::Absent,
            "the crash cut is after capacity release but before portable completion"
        );

        let reopened =
            nimbus_network::LocalNetworkAttachmentAuthority::open(fixture._temp_dir.path())
                .expect("fresh portable owner should reopen the deleting record");
        let lifecycle = OciAttachmentLifecycle::new(
            &fixture.allocator,
            Some(&reopened),
            &fixture.ipam,
            &fixture.ports,
            &fixture.lifetimes,
        );
        let restart_failure = adapter
            .detach_host_managed_with(
                &lifecycle,
                AttachmentTeardownMode::Restart,
                &ForbiddenHostEffects,
                |_| panic!("allocator-absent restart must fail before cleanup callbacks"),
            )
            .expect_err("allocator absence may reopen only the exact final-detach crash interval");
        assert_eq!(
            restart_failure.stage(),
            AttachmentDetachFailureStage::BeforeProviderDetach
        );
        assert!(
            restart_failure
                .into_error()
                .to_string()
                .contains("cannot complete Restart detach"),
            "restart must not turn released capacity back into a provisionable attachment"
        );
        let recovery_host = ContractHostEffects::default();
        let callback_failure = adapter
            .detach_host_managed_with(
                &lifecycle,
                AttachmentTeardownMode::Final,
                &recovery_host,
                |_| {
                    Err(SandboxError::OperationFailed {
                        message: "injected allocator-absent host cleanup retry".to_owned(),
                    })
                },
            )
            .expect_err("transient host cleanup failure must retain retryable finalization");
        assert_eq!(
            callback_failure.stage(),
            AttachmentDetachFailureStage::BeforeProviderDetach
        );
        assert_eq!(
            reopened
                .get(&fixture.tenant_id, &attachment_id)
                .expect("failed host recovery should inspect")
                .expect("failed host recovery must retain portable evidence")
                .resource()
                .phase(),
            nimbus_network::NetworkResourcePhase::CleanupPending
        );
        adapter
            .detach_host_managed_with(
                &lifecycle,
                AttachmentTeardownMode::Final,
                &recovery_host,
                |_| Ok(()),
            )
            .expect(
                "allocator absence plus exact portable and IPAM terminal evidence must complete",
            );
        assert!(
            recovery_host.operations().is_empty(),
            "reopen must complete portable state without replaying provider effects"
        );
        assert_eq!(
            reopened
                .get(&fixture.tenant_id, &attachment_id)
                .expect("completed portable attachment should inspect")
                .expect("terminal portable evidence should remain durable")
                .resource()
                .phase(),
            nimbus_network::NetworkResourcePhase::Released
        );
    }
}

#[test]
fn machine_forwarded_final_detach_reopens_after_allocator_finalization() {
    let fixture = ContractFixture::new(
        ContractBackend::Container,
        "nnc52a-machine-finalize-before-portable-completion",
    );
    let config = fixture.reserve_and_adopt();
    let forwarder = OciMachinePortForwarderConfig::for_provider_instance(
        "127.0.0.1",
        1,
        "/nnc52a-machine-finalization-forwarder",
        "nnc52a-machine-finalization-instance",
        NetworkResourceGeneration::new(1),
    )
    .expect("machine forwarder identity should validate");
    let adapter = fixture.machine_adapter(&config, &forwarder, &[], &[]);
    let mut attach_observer = ContractPhaseObserver::recording();
    adapter
        .attach_with(
            &fixture.lifecycle(),
            AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
            &ContractHostEffects::default(),
            &mut attach_observer,
            |_| Ok(()),
        )
        .expect("machine crash-window fixture should attach");
    adapter
        .detach_machine_forwarded_with(
            &fixture.lifecycle(),
            AttachmentTeardownMode::Restart,
            &ContractHostEffects::default(),
            || Ok(()),
            |()| Ok(()),
        )
        .expect("machine restart detach should retain allocation after provider absence");

    let attachment_id = default_network_attachment_id(&fixture.sandbox_id);
    let association = fixture
        .allocator
        .inspect_attachment_reservation(&fixture.tenant_id, &attachment_id, &fixture.claim)
        .expect("retained machine association should inspect")
        .association()
        .expect("machine restart-retained allocation should keep its exact association")
        .clone();
    let durable = state::OciAttachmentDurableState::compile(
        Some(&fixture.attachments),
        &adapter.context,
        association,
    )
    .expect("machine portable attachment state should compile");
    let record = durable
        .inspect()
        .expect("machine portable attachment should inspect")
        .expect("machine portable attachment should remain durable");
    let deleting =
        recovery::prepare_detach(&durable, record, AttachmentProviderObservation::Absent)
            .expect("confirmed machine provider absence should prepare final deletion")
            .record;
    assert_eq!(
        deleting.resource().phase(),
        nimbus_network::NetworkResourcePhase::Deleting
    );

    fixture
        .allocator
        .quarantine(&fixture.tenant_id, &attachment_id, Some(&fixture.claim))
        .expect("exact machine association should quarantine");
    crate::backends::oci::network::deallocate_container_ips_after_confirmed_detach(
        &fixture.ipam,
        &fixture.layout,
        &fixture.sandbox_id,
        &fixture.claim,
    )
    .expect("exact machine provider-absent IPAM should become a terminal witness");
    let cleanup = match fixture
        .allocator
        .release(&fixture.tenant_id, &attachment_id, Some(&fixture.claim))
        .expect("exact machine quarantined hold should release")
    {
        nimbus_network::NetworkSegmentReleaseOutcome::CleanupPending(cleanup) => cleanup,
        outcome => {
            panic!("last machine-held attachment must produce exact cleanup, got {outcome:?}")
        }
    };
    fixture
        .allocator
        .finalize_release(&cleanup)
        .expect("exact machine allocation finalization should succeed");

    let reopened = nimbus_network::LocalNetworkAttachmentAuthority::open(fixture._temp_dir.path())
        .expect("fresh portable owner should reopen the machine deleting record");
    let lifecycle = OciAttachmentLifecycle::new(
        &fixture.allocator,
        Some(&reopened),
        &fixture.ipam,
        &fixture.ports,
        &fixture.lifetimes,
    );
    let restart_failure = adapter
        .detach_machine_forwarded_with(
            &lifecycle,
            AttachmentTeardownMode::Restart,
            &ForbiddenHostEffects,
            || panic!("allocator-absent machine restart must fail before cleanup callbacks"),
            |()| panic!("allocator-absent machine restart must fail before completion callbacks"),
        )
        .expect_err("allocator absence may reopen only the exact final-detach crash interval");
    assert_eq!(
        restart_failure.stage(),
        AttachmentDetachFailureStage::BeforeProviderDetach
    );
    assert!(
        restart_failure
            .into_error()
            .to_string()
            .contains("cannot complete Restart detach")
    );

    let recovery_host = ContractHostEffects::default();
    let before_callbacks = std::cell::Cell::new(0);
    let after_callbacks = std::cell::Cell::new(0);
    let before_failure = adapter
        .detach_machine_forwarded_with(
            &lifecycle,
            AttachmentTeardownMode::Final,
            &recovery_host,
            || {
                before_callbacks.set(before_callbacks.get() + 1);
                Err(SandboxError::OperationFailed {
                    message: "injected allocator-absent machine pre-detach retry".to_owned(),
                })
            },
            |()| panic!("failed machine pre-detach callback must not reach completion callback"),
        )
        .expect_err("transient machine pre-detach failure must retain retryable finalization");
    assert_eq!(
        before_failure.stage(),
        AttachmentDetachFailureStage::BeforeProviderDetach
    );
    assert_eq!(
        reopened
            .get(&fixture.tenant_id, &attachment_id)
            .expect("failed machine pre-detach recovery should inspect")
            .expect("failed machine pre-detach recovery must retain portable evidence")
            .resource()
            .phase(),
        nimbus_network::NetworkResourcePhase::CleanupPending
    );
    let after_failure = adapter
        .detach_machine_forwarded_with(
            &lifecycle,
            AttachmentTeardownMode::Final,
            &recovery_host,
            || {
                before_callbacks.set(before_callbacks.get() + 1);
                Ok(())
            },
            |()| {
                after_callbacks.set(after_callbacks.get() + 1);
                Err(SandboxError::OperationFailed {
                    message: "injected allocator-absent machine completion retry".to_owned(),
                })
            },
        )
        .expect_err("transient machine completion failure must retain retryable finalization");
    assert_eq!(
        after_failure.stage(),
        AttachmentDetachFailureStage::CleanupPending
    );
    adapter
        .detach_machine_forwarded_with(
            &lifecycle,
            AttachmentTeardownMode::Final,
            &recovery_host,
            || {
                before_callbacks.set(before_callbacks.get() + 1);
                Ok(())
            },
            |()| {
                after_callbacks.set(after_callbacks.get() + 1);
                Ok(())
            },
        )
        .expect("machine final detach must complete the exact cross-authority crash interval");
    assert!(
        recovery_host.operations().is_empty(),
        "machine reopen must not replay provider effects"
    );
    assert_eq!(
        (before_callbacks.get(), after_callbacks.get()),
        (3, 2),
        "each machine-owned cleanup reconciler must run once per explicit retry"
    );
    assert_eq!(
        reopened
            .get(&fixture.tenant_id, &attachment_id)
            .expect("completed machine portable attachment should inspect")
            .expect("terminal machine portable evidence should remain durable")
            .resource()
            .phase(),
        nimbus_network::NetworkResourcePhase::Released
    );
}
