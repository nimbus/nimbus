use std::net::Ipv4Addr;
use std::sync::Mutex;

use nimbus_network::{
    DurableNetworkAttachmentState, LocalNetworkAttachmentAuthority, NetworkResourcePhase,
    NetworkTransitionEvidence,
};

use super::*;
use crate::backends::oci::network::attachment_lifecycle::recovery::AttachmentProviderObservation;
use crate::backends::oci::network::attachment_lifecycle::state::OciAttachmentDurableState;
use crate::backends::oci::network::netavark::{
    PreparedNetavarkSetup, PreparedNetavarkTeardown,
    execute_prepared_container_network_setup_for_test,
    execute_prepared_container_network_teardown_for_test, prepare_container_network_setup,
    prepare_container_network_teardown,
};

const PHASES: [NetworkResourcePhase; 11] = [
    NetworkResourcePhase::Reserved,
    NetworkResourcePhase::Provisioning,
    NetworkResourcePhase::Ready,
    NetworkResourcePhase::Publishing,
    NetworkResourcePhase::Active,
    NetworkResourcePhase::Withdrawing,
    NetworkResourcePhase::Draining,
    NetworkResourcePhase::Deleting,
    NetworkResourcePhase::CleanupPending,
    NetworkResourcePhase::Released,
    NetworkResourcePhase::Failed,
];

#[derive(Debug, Clone, Copy)]
enum ObservationKind {
    Absent,
    Present,
    Unknown,
}

impl ObservationKind {
    const ALL: [Self; 3] = [Self::Absent, Self::Present, Self::Unknown];

    fn observation(self) -> AttachmentProviderObservation {
        match self {
            Self::Absent => AttachmentProviderObservation::Absent,
            Self::Present => AttachmentProviderObservation::Present {
                assigned_ips: vec![Ipv4Addr::new(127, 92, 0, 2)],
            },
            Self::Unknown => AttachmentProviderObservation::Unknown {
                reason: "injected exact inspection conflict".to_owned(),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryOperation {
    Inspect,
    ProviderAttemptPrepared,
    NamespaceCreated,
    ProviderSetup,
    BackendPublication,
    BackendWithdrawal,
    ProviderTeardown,
    NamespaceRemoved,
}

struct RecoveryHostEffects<'a> {
    observation: ObservationKind,
    operations: Mutex<Vec<RecoveryOperation>>,
    allocator: &'a RecordingSegmentAllocator,
    allocator_before_inspection: Vec<SegmentAllocatorOperation>,
}

impl<'a> RecoveryHostEffects<'a> {
    fn new(
        fixture: &'a ContractFixture,
        observation: ObservationKind,
        allocator_before_inspection: Vec<SegmentAllocatorOperation>,
    ) -> Self {
        Self {
            observation,
            operations: Mutex::new(Vec::new()),
            allocator: &fixture.allocator,
            allocator_before_inspection,
        }
    }

    fn record(&self, operation: RecoveryOperation) {
        self.operations
            .lock()
            .expect("durable recovery trace lock should not be poisoned")
            .push(operation);
    }

    fn operations(&self) -> Vec<RecoveryOperation> {
        self.operations
            .lock()
            .expect("durable recovery trace lock should not be poisoned")
            .clone()
    }
}

impl AttachmentHostEffects for RecoveryHostEffects<'_> {
    fn inspect_provider(
        &self,
        _ipam: &OciIpamAuthority,
        _context: &OciAttachmentContext<'_>,
    ) -> AttachmentProviderObservation {
        let allocator_operations = self.allocator.operations();
        assert_eq!(
            &allocator_operations[..self.allocator_before_inspection.len()],
            self.allocator_before_inspection,
            "pre-existing attachment authority trace must remain stable"
        );
        assert!(
            allocator_operations[self.allocator_before_inspection.len()..]
                .iter()
                .all(|operation| matches!(
                    operation,
                    SegmentAllocatorOperation::InspectAttachment(..)
                )),
            "provider inspection must follow only read-only attachment authentication and precede \
             segment quarantine, release, or reacquisition: {allocator_operations:?}"
        );
        self.record(RecoveryOperation::Inspect);
        self.observation.observation()
    }

    fn create_namespace(&self, _context: &OciAttachmentContext<'_>) -> Result<()> {
        self.record(RecoveryOperation::NamespaceCreated);
        Ok(())
    }

    fn prepare_provider_setup(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
    ) -> Result<PreparedNetavarkSetup> {
        let prepared = prepare_container_network_setup(ipam, &context.operation())?;
        self.record(RecoveryOperation::ProviderAttemptPrepared);
        Ok(prepared)
    }

    fn setup_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
        prepared: PreparedNetavarkSetup,
    ) -> Result<Vec<Ipv4Addr>> {
        self.record(RecoveryOperation::ProviderSetup);
        execute_prepared_container_network_setup_for_test(ipam, &context.operation(), prepared)
    }

    fn teardown_provider(
        &self,
        ipam: &OciIpamAuthority,
        context: &OciAttachmentContext<'_>,
        prepared: PreparedNetavarkTeardown,
    ) -> Result<()> {
        self.record(RecoveryOperation::ProviderTeardown);
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
        self.record(RecoveryOperation::NamespaceRemoved);
        Ok(())
    }
}

fn seed_phase(
    fixture: &ContractFixture,
    config: &OciNetworkConfig,
    phase: NetworkResourcePhase,
) -> DurableNetworkAttachmentState {
    let adapter = fixture.host_adapter(fixture.backend, config, &[], &[]);
    let association = fixture
        .allocator
        .inspect_attachment_reservation(
            &fixture.tenant_id,
            &default_network_attachment_id(&fixture.sandbox_id),
            &config.reservation_claim,
        )
        .expect("allocator association should inspect")
        .association()
        .expect("adopted attachment should have an association")
        .clone();
    let durable = OciAttachmentDurableState::compile(
        Some(&fixture.attachments),
        &adapter.context,
        association,
    )
    .expect("durable attachment state should compile");
    let reserved = durable.reserve().expect("attachment should reserve");
    if phase == NetworkResourcePhase::Reserved {
        return reserved;
    }
    if phase == NetworkResourcePhase::Released {
        return durable
            .transition(
                &reserved,
                NetworkResourcePhase::Released,
                NetworkTransitionEvidence::ConfirmedNoEffect,
            )
            .expect("no-effect attachment should release");
    }
    if phase == NetworkResourcePhase::Failed {
        return durable
            .transition(
                &reserved,
                NetworkResourcePhase::Failed,
                NetworkTransitionEvidence::ConfirmedNoEffect,
            )
            .expect("no-effect attachment should fail terminally");
    }

    let provisioning = durable
        .transition(
            &reserved,
            NetworkResourcePhase::Provisioning,
            NetworkTransitionEvidence::Progress,
        )
        .expect("attachment should enter provisioning");
    if phase == NetworkResourcePhase::Provisioning {
        return provisioning;
    }
    let with_handle = durable
        .record_stable_handle(&provisioning)
        .expect("provider handle should persist");
    let ready = durable
        .transition(
            &with_handle,
            NetworkResourcePhase::Ready,
            NetworkTransitionEvidence::Progress,
        )
        .expect("attachment should become ready");
    if phase == NetworkResourcePhase::Ready {
        return ready;
    }
    let publishing = durable
        .transition(
            &ready,
            NetworkResourcePhase::Publishing,
            NetworkTransitionEvidence::Progress,
        )
        .expect("attachment should begin publication");
    if phase == NetworkResourcePhase::Publishing {
        return publishing;
    }
    let active = durable
        .transition(
            &publishing,
            NetworkResourcePhase::Active,
            NetworkTransitionEvidence::Progress,
        )
        .expect("attachment should become active");
    if phase == NetworkResourcePhase::Active {
        return active;
    }
    if phase == NetworkResourcePhase::CleanupPending {
        return durable
            .transition(
                &active,
                NetworkResourcePhase::CleanupPending,
                NetworkTransitionEvidence::AmbiguousEffect,
            )
            .expect("attachment should retain ambiguous cleanup");
    }
    let withdrawing = durable
        .transition(
            &active,
            NetworkResourcePhase::Withdrawing,
            NetworkTransitionEvidence::Progress,
        )
        .expect("attachment should begin withdrawal");
    if phase == NetworkResourcePhase::Withdrawing {
        return withdrawing;
    }
    if phase == NetworkResourcePhase::Deleting {
        return durable
            .transition(
                &withdrawing,
                NetworkResourcePhase::Deleting,
                NetworkTransitionEvidence::Progress,
            )
            .expect("attachment should begin deletion");
    }
    durable
        .transition(
            &withdrawing,
            NetworkResourcePhase::Draining,
            NetworkTransitionEvidence::Progress,
        )
        .expect("attachment should begin draining")
}

fn is_attach_phase(phase: NetworkResourcePhase) -> bool {
    matches!(
        phase,
        NetworkResourcePhase::Reserved
            | NetworkResourcePhase::Provisioning
            | NetworkResourcePhase::Ready
            | NetworkResourcePhase::Publishing
            | NetworkResourcePhase::Active
    )
}

fn should_succeed(phase: NetworkResourcePhase, observation: ObservationKind) -> bool {
    matches!(
        (is_attach_phase(phase), phase, observation),
        (
            true,
            NetworkResourcePhase::Reserved | NetworkResourcePhase::Provisioning,
            ObservationKind::Absent,
        ) | (
            true,
            NetworkResourcePhase::Provisioning
                | NetworkResourcePhase::Ready
                | NetworkResourcePhase::Publishing
                | NetworkResourcePhase::Active,
            ObservationKind::Present,
        ) | (
            false,
            NetworkResourcePhase::Withdrawing
                | NetworkResourcePhase::Draining
                | NetworkResourcePhase::Deleting
                | NetworkResourcePhase::CleanupPending,
            ObservationKind::Absent | ObservationKind::Present,
        ) | (
            false,
            NetworkResourcePhase::Released | NetworkResourcePhase::Failed,
            ObservationKind::Absent,
        )
    )
}

fn expected_failure_phase(
    phase: NetworkResourcePhase,
    observation: ObservationKind,
) -> NetworkResourcePhase {
    if matches!(observation, ObservationKind::Unknown)
        && matches!(
            phase,
            NetworkResourcePhase::Provisioning
                | NetworkResourcePhase::Ready
                | NetworkResourcePhase::Publishing
                | NetworkResourcePhase::Active
                | NetworkResourcePhase::Withdrawing
                | NetworkResourcePhase::Draining
                | NetworkResourcePhase::Deleting
        )
    {
        NetworkResourcePhase::CleanupPending
    } else {
        phase
    }
}

fn run_phase_matrix(backend: ContractBackend) {
    for phase in PHASES {
        for observation in ObservationKind::ALL {
            let row = format!("durable-{}-{:?}-{:?}", backend.label(), phase, observation);
            let fixture = ContractFixture::new(backend, &row);
            let config = fixture.reserve_and_adopt();
            let seeded = seed_phase(&fixture, &config, phase);
            let expected_version = seeded.resource().version().clone();
            let expected_provider = seeded.selected_provider_id().clone();
            let allocator_before_inspection = fixture.allocator.operations();

            let reopened = LocalNetworkAttachmentAuthority::open(fixture._temp_dir.path())
                .expect("fresh durable attachment authority should reopen");
            let lifecycle = OciAttachmentLifecycle::new(
                &fixture.allocator,
                Some(&reopened),
                &fixture.ipam,
                &fixture.ports,
                &fixture.lifetimes,
            );
            let adapter = fixture.host_adapter(backend, &config, &[], &[]);
            let host = RecoveryHostEffects::new(&fixture, observation, allocator_before_inspection);
            let mut observer = ContractPhaseObserver::recording();

            let result = if is_attach_phase(phase) {
                adapter
                    .attach_with(
                        &lifecycle,
                        AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
                        &host,
                        &mut observer,
                        |_| {
                            host.record(RecoveryOperation::BackendPublication);
                            Ok(())
                        },
                    )
                    .map(|_| ())
            } else {
                adapter
                    .detach_host_managed_with(
                        &lifecycle,
                        AttachmentTeardownMode::Final,
                        &host,
                        |_| {
                            host.record(RecoveryOperation::BackendWithdrawal);
                            Ok(())
                        },
                    )
                    .map_err(|failure| failure.error)
            };

            assert_eq!(
                result.is_ok(),
                should_succeed(phase, observation),
                "{backend:?} {phase:?} {observation:?} returned {result:?}"
            );
            let operations = host.operations();
            assert_eq!(
                operations.first(),
                Some(&RecoveryOperation::Inspect),
                "{backend:?} {phase:?} {observation:?} must inspect before every effect: \
                 {operations:?}"
            );
            if matches!(observation, ObservationKind::Present) {
                assert!(
                    !operations.contains(&RecoveryOperation::NamespaceCreated)
                        && !operations.contains(&RecoveryOperation::ProviderSetup),
                    "{backend:?} {phase:?} exact presence must never recreate the provider: \
                     {operations:?}"
                );
            }
            if matches!(observation, ObservationKind::Unknown)
                || matches!(
                    phase,
                    NetworkResourcePhase::Released | NetworkResourcePhase::Failed
                )
            {
                assert_eq!(
                    operations,
                    vec![RecoveryOperation::Inspect],
                    "{backend:?} {phase:?} {observation:?} must remain effect-free"
                );
            }

            let final_record = reopened
                .get(
                    &fixture.tenant_id,
                    &default_network_attachment_id(&fixture.sandbox_id),
                )
                .expect("durable attachment should inspect")
                .expect("durable attachment should remain recorded");
            assert_eq!(
                final_record.resource().version(),
                &expected_version,
                "{backend:?} {phase:?} {observation:?} must retain exact generation fencing"
            );
            assert_eq!(
                final_record.selected_provider_id(),
                &expected_provider,
                "{backend:?} {phase:?} {observation:?} must retain selected provider"
            );
            if result.is_ok() && !phase.is_terminal() {
                let expected = if is_attach_phase(phase) {
                    NetworkResourcePhase::Active
                } else {
                    NetworkResourcePhase::Released
                };
                assert_eq!(
                    final_record.resource().phase(),
                    expected,
                    "{backend:?} {phase:?} {observation:?} must converge"
                );
            } else if result.is_err() {
                assert_eq!(
                    final_record.resource().phase(),
                    expected_failure_phase(phase, observation),
                    "{backend:?} {phase:?} {observation:?} must fail with the expected fence"
                );
            }
        }
    }
}

#[test]
fn container_all_phases_inspect_before_recovery_effects() {
    run_phase_matrix(ContractBackend::Container);
}

#[test]
fn krun_all_phases_inspect_before_recovery_effects() {
    run_phase_matrix(ContractBackend::Krun);
}

#[test]
fn corrupt_store_fails_before_provider_inspection_for_both_backend_routes() {
    for backend in [ContractBackend::Container, ContractBackend::Krun] {
        let fixture = ContractFixture::new(backend, "durable-corrupt-store");
        let config = fixture.reserve_and_adopt();
        std::fs::write(fixture.attachments.authority_path(), b"{")
            .expect("test should truncate its isolated authority");
        assert!(
            LocalNetworkAttachmentAuthority::open(fixture._temp_dir.path()).is_err(),
            "the corrupt durable authority must not reopen"
        );

        let lifecycle = OciAttachmentLifecycle::new(
            &fixture.allocator,
            None,
            &fixture.ipam,
            &fixture.ports,
            &fixture.lifetimes,
        );
        let host = RecoveryHostEffects::new(
            &fixture,
            ObservationKind::Absent,
            fixture.allocator.operations(),
        );
        let mut observer = ContractPhaseObserver::recording();
        let error = fixture
            .host_adapter(backend, &config, &[], &[])
            .attach_with(
                &lifecycle,
                AttachmentAttachAuthority::FreshLaunch(&fixture.claim),
                &host,
                &mut observer,
                |_| Ok(()),
            )
            .expect_err("corrupt authority must fail before recovery");

        assert!(
            error.to_string().contains("network authority")
                || error.to_string().contains("durable attachment authority"),
            "{backend:?} should expose an authority diagnostic: {error}"
        );
        assert!(
            host.operations().is_empty(),
            "{backend:?} must not inspect or execute provider effects after store corruption"
        );
        assert!(
            !fixture.layout.netns_path.exists(),
            "{backend:?} must fail before namespace effects"
        );
    }
}
