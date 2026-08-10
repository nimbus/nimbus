//! Real Container host-managed attachment teardown proofs.

use std::sync::{Barrier, mpsc};
use std::time::Duration;

use nimbus_network::{NetworkCapabilitySourceDigest, NetworkResourcePhase, PortLeasePhase};

use super::*;
use crate::backends::CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY;
use crate::provider_command::{
    ProviderCommandLockTestProbe, with_provider_command_lock_test_probe,
};
use crate::{
    ProviderCommandClaim, SandboxNetworkTeardownCommand, SandboxNetworkTeardownCommandInput,
    SandboxNetworkTeardownIdentity, SandboxNetworkTeardownIdentityInput,
    SandboxNetworkTeardownObservation, SandboxNetworkTeardownOperation,
};

#[path = "network_teardown/fresh_process.rs"]
mod fresh_process;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NetworkContenderRole {
    Execute,
    Adopt,
}

fn network_command(
    fixture: &TeardownFixture,
    stop: &SandboxExecutionTeardownCommand,
    operation: SandboxNetworkTeardownOperation,
    epoch: u64,
) -> SandboxNetworkTeardownCommand {
    let manifest = fixture.manifest();
    let plan = manifest
        .provision_network_plan
        .as_ref()
        .expect("attached fixture has a compiled network plan");
    let identity = SandboxNetworkTeardownIdentity::new(SandboxNetworkTeardownIdentityInput {
        tenant_id: manifest.spec.tenant_id.clone(),
        sandbox_id: fixture.id.clone(),
        execution_attempt_id: fixture.execution_attempt_id.clone(),
        attachment_id: plan.attachment_id().clone(),
        network_plan: plan.network_plan().clone(),
        provider_registration_key: CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY.to_owned(),
        provider_source_digest: NetworkCapabilitySourceDigest::from_bytes([9; 32]),
    })
    .expect("network identity should validate");
    let stop_claim = stop.provider_claim();
    let claim = ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: stop_claim.authority_id().to_owned(),
        effect_subject: identity.provider_effect_subject(),
        source_attempt_id: None,
        attempt_id: stop_claim.attempt_id().to_owned(),
        dispatch_epoch: epoch,
        workload_generation: stop_claim.workload_generation(),
        restart_ordinal: 0,
        desired_digest: stop_claim.desired_digest().to_owned(),
        source_digest: stop_claim.source_digest().to_owned(),
        network_plan_digest: stop_claim.network_plan_digest().to_owned(),
        provider_target_digest: identity.provider_target_digest(),
        operation: operation.provider_operation(),
    })
    .expect("network provider claim should validate");
    SandboxNetworkTeardownCommand::new(SandboxNetworkTeardownCommandInput {
        identity,
        operation,
        provider_claim: claim,
    })
    .expect("network command should validate")
}

fn execute_network(
    fixture: &TeardownFixture,
    command: &SandboxNetworkTeardownCommand,
) -> crate::ProviderCommandObservation {
    execute_network_with_backend(&fixture.backend, command)
}

fn execute_network_with_backend(
    backend: &ContainerSandboxBackend,
    command: &SandboxNetworkTeardownCommand,
) -> crate::ProviderCommandObservation {
    let journal = backend
        .attempt_idempotency_journal()
        .expect("network journal should open");
    let execution = match journal
        .claim_dispatch_epoch(command.provider_claim())
        .expect("network command should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("fresh network command must receive effect authority")
        }
    };
    backend
        .execute_network_teardown_with_claim(command, execution)
        .expect("network command should publish its result")
}

fn contend_network(
    fixture: &TeardownFixture,
    command: &SandboxNetworkTeardownCommand,
) -> Vec<(NetworkContenderRole, ProviderCommandObservationKind)> {
    let backend = Arc::new(fixture.backend.clone());
    let journal = Arc::new(
        fixture
            .backend
            .attempt_idempotency_journal()
            .expect("one network journal should open"),
    );
    let command = Arc::new(command.clone());
    let start = Arc::new(Barrier::new(3));
    let outcomes = std::thread::scope(|scope| {
        let mut contenders = Vec::new();
        for _ in 0..2 {
            let backend = Arc::clone(&backend);
            let journal = Arc::clone(&journal);
            let command = Arc::clone(&command);
            let start = Arc::clone(&start);
            contenders.push(scope.spawn(move || {
                start.wait();
                match journal
                    .claim_dispatch_epoch(command.provider_claim())
                    .expect("network contender should reach the one journal")
                {
                    ProviderCommandClaimDecision::ExecuteClaimed(execution) => {
                        let observation = backend
                            .execute_network_teardown_with_claim(&command, execution)
                            .expect("winning network contender should publish");
                        (NetworkContenderRole::Execute, observation.kind())
                    }
                    ProviderCommandClaimDecision::AdoptExactAttempt(observation) => {
                        (NetworkContenderRole::Adopt, observation.kind())
                    }
                }
            }));
        }
        start.wait();
        contenders
            .into_iter()
            .map(|contender| contender.join().expect("network contender should join"))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        outcomes
            .iter()
            .filter(|(role, _)| *role == NetworkContenderRole::Execute)
            .count(),
        1,
        "one exact network contender must own provider execution: {outcomes:?}"
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|(role, _)| *role == NetworkContenderRole::Adopt)
            .count(),
        1,
        "the other exact network contender must adopt durable authority: {outcomes:?}"
    );
    assert_eq!(
        journal
            .adopt_exact_attempt(command.provider_claim())
            .expect("terminal network authority should read")
            .expect("terminal network authority should exist")
            .kind(),
        ProviderCommandObservationKind::Succeeded
    );
    outcomes
}

fn runtime_authority(manifest: &ContainerSandboxManifest) -> Vec<u8> {
    serde_json::to_vec(&(
        &manifest.execution_attempt_id,
        &manifest.conmon_launch,
        manifest.last_exit_code,
        manifest.shutdown_requested,
        &manifest.execution_teardown,
    ))
    .expect("runtime authority should serialize")
}

#[test]
fn container_network_teardown_detaches_retained_then_releases_in_order() {
    let fixture = TeardownFixture::attached("network-detach-release");
    let drain = fixture.command(SandboxExecutionTeardownOperation::Drain, "network", 1);
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let stop = fixture.command(SandboxExecutionTeardownOperation::Stop, "network", 1);
    fixture.runtime_for_terminal_stop(&stop);

    let before = fixture.manifest();
    let detach = network_command(&fixture, &stop, SandboxNetworkTeardownOperation::Detach, 1);
    let detached = execute_network(&fixture, &detach);
    let detached_inspection = fixture
        .backend
        .inspect_network_teardown_with_observation(&detach, &detached);
    let failed_manifest = fixture.manifest();
    let pep = failed_manifest
        .egress_proxy
        .as_ref()
        .expect("attached fixture retains its PEP assignment");
    let pep_record = fixture
        .backend
        .port_lease_coordinator_for_manifest(&failed_manifest)
        .expect("PEP port authority should open")
        .port_lease_records_snapshot(std::slice::from_ref(&pep.port_lease), "failed detach PEP")
        .expect("PEP record should inspect");
    assert_eq!(
        detached.kind(),
        ProviderCommandObservationKind::Succeeded,
        "provider={detached:?}; inspection={detached_inspection:?}; state={:?}; pep={pep:?}; record={pep_record:?}",
        failed_manifest.network_teardown
    );

    let retained = fixture.manifest();
    assert!(retained.network_teardown.detached_proof().is_some());
    assert!(!retained.network_layout.netns_path.exists());
    assert!(!retained.network_layout.status_path.exists());
    let ports = fixture
        .backend
        .port_lease_coordinator_for_manifest(&retained)
        .expect("retained port authority should open")
        .port_lease_records_snapshot(&retained.port_leases, "retained test listeners")
        .expect("retained listener records should inspect");
    assert!(ports.iter().all(|record| {
        record.phase() == PortLeasePhase::Reserved
            && record.bind_claim().is_none()
            && record.binding().is_none()
            && record.active_lifetime().is_none()
    }));
    let attachment = fixture
        .backend
        .attachment_authority
        .as_ref()
        .expect("portable attachment authority should exist")
        .get(
            &retained.spec.tenant_id,
            &retained
                .network_config
                .as_ref()
                .expect("network config remains retained")
                .attachment_id,
        )
        .expect("portable attachment should inspect")
        .expect("portable attachment should remain durable");
    assert_eq!(
        attachment.resource().phase(),
        NetworkResourcePhase::Deleting
    );
    assert_eq!(before.network_config, retained.network_config);

    let replay = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("network journal should reopen")
        .claim_dispatch_epoch(detach.provider_claim())
        .expect("exact detach should replay");
    assert!(matches!(
        replay,
        ProviderCommandClaimDecision::AdoptExactAttempt(observation)
            if observation.kind() == ProviderCommandObservationKind::Succeeded
    ));

    let release = network_command(&fixture, &stop, SandboxNetworkTeardownOperation::Release, 1);
    let released = execute_network(&fixture, &release);
    assert_eq!(
        released.kind(),
        ProviderCommandObservationKind::Succeeded,
        "provider={released:?}; state={:?}",
        fixture.manifest().network_teardown
    );
    let terminal = fixture.manifest();
    assert_eq!(
        terminal.network_teardown.release_phase(),
        crate::backends::oci::network::HostManagedAttachmentReleasePhase::Released
    );
    let attachment = fixture
        .backend
        .attachment_authority
        .as_ref()
        .expect("portable attachment authority should exist")
        .get(
            &terminal.spec.tenant_id,
            &terminal
                .network_config
                .as_ref()
                .expect("released manifest retains identity evidence")
                .attachment_id,
        )
        .expect("released attachment should inspect")
        .expect("released attachment tombstone should remain durable");
    assert_eq!(
        attachment.resource().phase(),
        NetworkResourcePhase::Released
    );
}

#[test]
fn container_network_detach_recovers_pep_after_process_owner_death() {
    let fixture = TeardownFixture::attached("network-detach-pep-owner-death");
    let drain = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "network-owner-death",
        1,
    );
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let stop = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "network-owner-death",
        1,
    );
    fixture.runtime_for_terminal_stop(&stop);
    let detach = network_command(&fixture, &stop, SandboxNetworkTeardownOperation::Detach, 1);
    let config = fixture.backend.config.clone();
    let root = fixture.root;
    drop(fixture.backend);

    let reopened = ContainerSandboxBackend::new(config)
        .with_egress_pin_provider(Arc::new(FixedOciEgressPinProvider::ready()));
    let detached = execute_network_with_backend(&reopened, &detach);
    assert_eq!(
        detached.kind(),
        ProviderCommandObservationKind::Succeeded,
        "fresh backend must recover the dead process-bound PEP from durable lifetime authority"
    );
    let manifest = reopened
        .read_manifest(detach.sandbox_id())
        .expect("reopened manifest should read")
        .expect("reopened manifest should remain durable");
    assert!(manifest.network_teardown.detached_proof().is_some());
    let assignment = manifest
        .egress_proxy
        .as_ref()
        .expect("detached manifest retains the PEP assignment");
    let record = reopened
        .port_lease_coordinator_for_manifest(&manifest)
        .expect("reopened port authority should compile")
        .port_lease_records_snapshot(
            std::slice::from_ref(&assignment.port_lease),
            "owner-death retained PEP",
        )
        .expect("retained PEP should inspect")
        .pop()
        .expect("one retained PEP record should exist");
    assert_eq!(record.phase(), PortLeasePhase::Reserved);
    assert!(record.binding().is_none());
    assert!(record.active_lifetime().is_none());
    assert!(record.confirmed_stopped_binding().is_some());
    assert!(
        !crate::backends::oci::egress::egress_trust_anchor_path(
            &crate::backends::oci::egress::egress_trust_anchor_root(
                &reopened.config.network_state_root,
            ),
            &manifest.spec.tenant_id,
            &manifest.handle.id,
        )
        .exists(),
        "dead-owner recovery must remove the exact trust anchor before retained settlement"
    );
    drop(root);
}

#[test]
fn container_network_two_thread_contenders_have_one_detach_and_release_winner() {
    let fixture = TeardownFixture::attached("network-thread-contenders");
    let drain = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "network-thread-contenders",
        1,
    );
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let stop = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "network-thread-contenders",
        1,
    );
    let runtime = fixture.runtime_for_terminal_stop(&stop);
    let stopped_runtime = runtime_authority(&fixture.manifest());

    let detach = network_command(&fixture, &stop, SandboxNetworkTeardownOperation::Detach, 1);
    let detach_outcomes = contend_network(&fixture, &detach);
    assert!(detach_outcomes.iter().all(|(role, kind)| matches!(
        (role, kind),
        (
            NetworkContenderRole::Execute,
            ProviderCommandObservationKind::Succeeded
        ) | (
            NetworkContenderRole::Adopt,
            ProviderCommandObservationKind::Claimed | ProviderCommandObservationKind::Succeeded
        )
    )));
    let retained = fixture.manifest();
    assert!(retained.network_teardown.detached_proof().is_some());
    assert!(!retained.network_layout.netns_path.exists());
    assert!(!retained.network_layout.status_path.exists());
    assert_eq!(runtime_authority(&retained), stopped_runtime);
    assert!(runtime.signals().is_empty());

    let release = network_command(&fixture, &stop, SandboxNetworkTeardownOperation::Release, 1);
    let release_outcomes = contend_network(&fixture, &release);
    assert!(release_outcomes.iter().all(|(role, kind)| matches!(
        (role, kind),
        (
            NetworkContenderRole::Execute,
            ProviderCommandObservationKind::Succeeded
        ) | (
            NetworkContenderRole::Adopt,
            ProviderCommandObservationKind::Claimed | ProviderCommandObservationKind::Succeeded
        )
    )));
    let released = fixture.manifest();
    assert_eq!(
        released.network_teardown.release_phase(),
        crate::backends::oci::network::HostManagedAttachmentReleasePhase::Released
    );
    assert_eq!(runtime_authority(&released), stopped_runtime);
    assert!(runtime.signals().is_empty());
}

#[test]
fn container_network_inspect_is_byte_stable_and_cannot_cross_older_execute() {
    let fixture = TeardownFixture::attached("network-inspect-order");
    let drain = fixture.command(
        SandboxExecutionTeardownOperation::Drain,
        "network-inspect-order",
        1,
    );
    assert!(matches!(
        fixture.backend.execute_execution_teardown(&drain),
        SandboxExecutionTeardownObservation::Succeeded { .. }
    ));
    let stop = fixture.command(
        SandboxExecutionTeardownOperation::Stop,
        "network-inspect-order",
        1,
    );
    let runtime = fixture.runtime_for_terminal_stop(&stop);
    let stopped_runtime = runtime_authority(&fixture.manifest());
    let detach = Arc::new(network_command(
        &fixture,
        &stop,
        SandboxNetworkTeardownOperation::Detach,
        1,
    ));
    let journal = fixture
        .backend
        .attempt_idempotency_journal()
        .expect("one network journal should open");
    let execution = match journal
        .claim_dispatch_epoch(detach.provider_claim())
        .expect("detach should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("fresh detach must receive execution authority")
        }
    };
    let claimed = execution.observation().clone();

    let before_inspection = snapshot_files(fixture.root.path());
    assert!(matches!(
        fixture
            .backend
            .inspect_network_teardown_with_observation(&detach, &claimed),
        SandboxNetworkTeardownObservation::InProgress { .. }
    ));
    assert_eq!(
        snapshot_files(fixture.root.path()),
        before_inspection,
        "exact network Inspect must not change a durable byte"
    );

    let execute_probe =
        super::super::super::runner::RunnerLifecycleLockTestProbe::new(Duration::from_secs(2));
    let inspect_probe =
        super::super::super::runner::RunnerLifecycleLockTestProbe::new(Duration::from_millis(100));
    let execute_backend = fixture
        .backend
        .clone()
        .with_runner_lifecycle_lock_test_probe(execute_probe.clone());
    let inspect_backend = fixture
        .backend
        .clone()
        .with_runner_lifecycle_lock_test_probe(inspect_probe.clone());
    let lifecycle = super::super::super::runner::lock_execute_lifecycle(&fixture.manifest())
        .expect("test should hold the production Execute lifecycle lock");

    let execute_command = Arc::clone(&detach);
    let executor = std::thread::spawn(move || {
        execute_backend.execute_network_teardown_with_claim(&execute_command, execution)
    });
    assert!(
        execute_probe.wait_until_contended(),
        "network Execute must hold its journal stream before waiting for lifecycle authority"
    );

    let inspect_command = Arc::clone(&detach);
    let provider_lock_probe = ProviderCommandLockTestProbe::new(Duration::from_secs(1));
    let inspector_lock_probe = provider_lock_probe.clone();
    let (inspection_tx, inspection_rx) = mpsc::channel();
    let inspector = std::thread::spawn(move || {
        let observation = with_provider_command_lock_test_probe(inspector_lock_probe, || {
            inspect_backend.inspect_network_teardown_with_observation(&inspect_command, &claimed)
        });
        inspection_tx
            .send(observation)
            .expect("inspection result should send");
    });
    assert!(
        provider_lock_probe.wait_until_contended(),
        "Inspect must attempt the exact live provider stream lock"
    );
    assert!(
        !inspect_probe.wait_until_contended(),
        "Inspect must wait at the journal before it can contend for the lifecycle lock"
    );
    assert!(
        matches!(inspection_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "Inspect must not report incomplete progress while the older Execute can publish"
    );

    drop(lifecycle);
    let executed = executor
        .join()
        .expect("network executor should join")
        .expect("network executor should publish");
    assert_eq!(executed.kind(), ProviderCommandObservationKind::Succeeded);
    let inspected = inspection_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("inspection should finish after the older Execute publishes");
    inspector.join().expect("network inspector should join");
    assert!(
        matches!(
            inspected,
            SandboxNetworkTeardownObservation::Ambiguous { .. }
        ),
        "the stale claimed observation must not report NotCompleted after terminal publication: {inspected:?}"
    );
    let terminal = fixture.manifest();
    assert!(terminal.network_teardown.detached_proof().is_some());
    assert_eq!(runtime_authority(&terminal), stopped_runtime);
    assert!(runtime.signals().is_empty());
}

impl TeardownFixture {
    fn runtime_for_terminal_stop(&self, stop: &SandboxExecutionTeardownCommand) -> ScriptedRuntime {
        let journal = self
            .backend
            .attempt_idempotency_journal()
            .expect("execution journal should open");
        let execution = claim_teardown_execution(&journal, stop);
        let runtime = ScriptedRuntime::live(self.backend.clone(), 100);
        runtime.terminal.store(true, Ordering::Release);
        let observation = self
            .backend
            .execute_execution_teardown_inner_with_runtime_and_authorization(
                stop,
                &runtime,
                Some(execution.observation()),
            )
            .expect("scripted terminal stop should complete");
        assert!(matches!(
            observation,
            SandboxExecutionTeardownObservation::Succeeded { .. }
        ));
        persist_teardown_observation(&journal, stop, &observation);
        runtime
    }
}
