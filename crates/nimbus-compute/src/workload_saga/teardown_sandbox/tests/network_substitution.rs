//! Compute acceptance tests for real network teardown substitution.

use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_sandbox::backends::container::{ContainerSandboxBackend, ContainerSandboxBackendConfig};
use nimbus_sandbox::backends::krun::{KrunSandboxBackend, KrunSandboxBackendConfig};
use nimbus_sandbox::backends::test_hooks::{
    PreparedContainerNetworkTeardown, PreparedKrunNetworkTeardown,
};
use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandClaimInput, ProviderCommandExecutionClaim, ProviderCommandJournalError,
    ProviderCommandObservation, ProviderCommandObservationKind, SandboxBackendKind,
    SandboxNetworkTeardownObservation, SandboxOwnerSpec, SandboxProcessSpec, SandboxRootSpec,
    SandboxSpec,
};
use nimbus_workloads::{
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadTeardownEffectResult, WorkloadTeardownStep,
};

use super::{
    ExactWorkloadTeardownCapability, confirmed_teardown_commands,
    confirmed_teardown_commands_with_claimed_record, snapshot_files,
};
use crate::workload_saga::recovery::tests::teardown_success_evidence;
use crate::workload_saga::{
    ContainerAttachmentTeardownAdapter, KrunAttachmentTeardownAdapter, NetworkDetachmentCapability,
    NetworkReleaseCapability, WorkloadTeardownCapabilityRegistry,
    WorkloadTeardownCapabilityRegistryError, WorkloadTeardownExecuteOutcome,
    WorkloadTeardownInspectOutcome, WorkloadTeardownProviderObservation,
    WorkloadTeardownProviderOutcome,
};

fn observed_record(backend: SandboxBackendKind, rootfs: &Path) -> WorkloadSagaRecord {
    let mut record = crate::workload_saga::provision_sandbox::tests::composed_record_with_rootfs(
        backend, rootfs,
    );
    for _ in 0..8 {
        if record.phase() == WorkloadSagaPhase::Observed {
            return record;
        }
        record = crate::workload_saga::test_support::confirmed_provision(&record);
    }
    panic!("network teardown fixture did not reach Observed");
}

fn network_teardown_record(
    observed: &WorkloadSagaRecord,
    backend: SandboxBackendKind,
    target: WorkloadSagaPhase,
) -> WorkloadSagaRecord {
    let successor_generation = observed.active_intent().generation().as_u64() + 1;
    let profile = match backend {
        SandboxBackendKind::Container => "container",
        SandboxBackendKind::Krun => "krun",
    };
    let teardown = super::begin_teardown(
        observed,
        super::stopped_intent(profile, successor_generation),
    );
    super::finish_teardown(teardown, target)
}

async fn commands_for(
    backend: SandboxBackendKind,
    target: WorkloadSagaPhase,
    rootfs: &Path,
) -> (
    super::ConfirmedWorkloadTeardownCommand,
    super::ConfirmedWorkloadTeardownCommand,
) {
    let observed = observed_record(backend, rootfs);
    confirmed_teardown_commands(network_teardown_record(&observed, backend, target)).await
}

fn sandbox_spec(backend: SandboxBackendKind, rootfs: &Path) -> SandboxSpec {
    let label = match backend {
        SandboxBackendKind::Container => "container",
        SandboxBackendKind::Krun => "krun",
    };
    SandboxSpec::new(
        nimbus_core::TenantId::new(format!("tenant-{label}"))
            .expect("fixture tenant should validate"),
        SandboxOwnerSpec::standalone_named(label),
        backend,
        SandboxRootSpec::rootfs(rootfs),
        SandboxProcessSpec::new(["/bin/true"]),
    )
}

fn assert_execute_succeeded(
    observation: WorkloadTeardownProviderObservation,
    command: &super::ConfirmedWorkloadTeardownCommand,
) {
    assert!(observation.matches_command(command));
    let WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(
        success,
    )) = observation.into_outcome()
    else {
        panic!("real {:?} effect must succeed", command.step());
    };
    assert_eq!(success.step(), command.step());
}

fn assert_inspect_satisfied(
    observation: WorkloadTeardownProviderObservation,
    command: &super::ConfirmedWorkloadTeardownCommand,
) {
    assert!(observation.matches_command(command));
    let WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Satisfied(
        success,
    )) = observation.into_outcome()
    else {
        panic!("real {:?} inspection must be satisfied", command.step());
    };
    assert_eq!(success.step(), command.step());
}

fn apply_teardown_success(
    claimed: WorkloadSagaRecord,
    command: &super::ConfirmedWorkloadTeardownCommand,
) -> WorkloadSagaRecord {
    claimed
        .apply_teardown_effect_result(
            command.claim(),
            WorkloadTeardownEffectResult::Succeeded {
                attempt_id: command.attempt_id().clone(),
                dispatch_epoch: command.dispatch_epoch(),
                provider_target: command.provider_target().clone(),
                evidence: Box::new(teardown_success_evidence(
                    command.step(),
                    command.subjects(),
                )),
            },
        )
        .expect("the exact provider success should advance the teardown record")
}

async fn execute_attachment<Adapter>(
    adapter: &Adapter,
    command: &super::ConfirmedWorkloadTeardownCommand,
) -> WorkloadTeardownProviderObservation
where
    Adapter: NetworkDetachmentCapability + NetworkReleaseCapability,
{
    match command.step() {
        WorkloadTeardownStep::DetachNetwork => {
            NetworkDetachmentCapability::execute(adapter, command).await
        }
        WorkloadTeardownStep::ReleaseNetwork => {
            NetworkReleaseCapability::execute(adapter, command).await
        }
        step => panic!("network acceptance fixture produced {step:?}"),
    }
}

async fn inspect_attachment<Adapter>(
    adapter: &Adapter,
    command: &super::ConfirmedWorkloadTeardownCommand,
) -> WorkloadTeardownProviderObservation
where
    Adapter: NetworkDetachmentCapability + NetworkReleaseCapability,
{
    match command.step() {
        WorkloadTeardownStep::DetachNetwork => {
            NetworkDetachmentCapability::inspect(adapter, command).await
        }
        WorkloadTeardownStep::ReleaseNetwork => {
            NetworkReleaseCapability::inspect(adapter, command).await
        }
        step => panic!("network acceptance fixture produced {step:?}"),
    }
}

#[tokio::test]
async fn real_attachment_capabilities_select_exact_backend_and_operation_without_fallback() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let container = Arc::new(
        ContainerAttachmentTeardownAdapter::new(Arc::new(ContainerSandboxBackend::new(
            ContainerSandboxBackendConfig::under_root(root.path().join("container")),
        )))
        .expect("Container attachment adapter should open its journal"),
    );
    let krun = Arc::new(
        KrunAttachmentTeardownAdapter::new(Arc::new(KrunSandboxBackend::new(
            KrunSandboxBackendConfig::under_root(root.path().join("krun")),
        )))
        .expect("Krun attachment adapter should open its journal"),
    );
    let registry = WorkloadTeardownCapabilityRegistry::new(
        [
            container.clone().capabilities(),
            krun.clone().capabilities(),
        ],
        [],
        [],
    )
    .expect("the two real attachment providers should register once");

    for backend in [SandboxBackendKind::Container, SandboxBackendKind::Krun] {
        for (target, expected_step) in [
            (
                WorkloadSagaPhase::WorkloadStopped,
                WorkloadTeardownStep::DetachNetwork,
            ),
            (
                WorkloadSagaPhase::NetworkDetached,
                WorkloadTeardownStep::ReleaseNetwork,
            ),
        ] {
            let (command, _) = commands_for(backend, target, root.path()).await;
            assert_eq!(command.step(), expected_step);
            assert!(matches!(
                (expected_step, registry.select_exact(&command)),
                (
                    WorkloadTeardownStep::DetachNetwork,
                    Ok(ExactWorkloadTeardownCapability::NetworkDetach(_))
                ) | (
                    WorkloadTeardownStep::ReleaseNetwork,
                    Ok(ExactWorkloadTeardownCapability::NetworkRelease(_))
                )
            ));
        }
    }

    let container_only =
        WorkloadTeardownCapabilityRegistry::new([container.capabilities()], [], [])
            .expect("the Container attachment provider should register once");
    let (krun_detach, _) = commands_for(
        SandboxBackendKind::Krun,
        WorkloadSagaPhase::WorkloadStopped,
        root.path(),
    )
    .await;
    assert!(matches!(
        container_only.select_exact(&krun_detach),
        Err(WorkloadTeardownCapabilityRegistryError::MissingExactCapability { .. })
    ));
}

async fn assert_real_missing_state_reopen<Adapter>(
    backend: SandboxBackendKind,
    root: &Path,
    make_adapter: impl Fn() -> (Adapter, ProviderCommandAttemptJournal),
) where
    Adapter: NetworkDetachmentCapability + NetworkReleaseCapability,
{
    for (label, target) in [
        ("detach", WorkloadSagaPhase::WorkloadStopped),
        ("release", WorkloadSagaPhase::NetworkDetached),
    ] {
        let case_root = root.join(label);
        let (execute, inspect) = commands_for(backend, target, &case_root).await;
        let validated =
            super::super::attachment::validate_sandbox_network_teardown_command(&execute, backend)
                .expect("the exact network teardown command should lower");
        let (adapter, journal) = make_adapter();

        let executed = execute_attachment(&adapter, &execute).await;
        assert!(executed.matches_command(&execute));
        assert!(matches!(
            executed.into_outcome(),
            WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
        ));
        assert!(
            journal
                .adopt_exact_attempt(validated.sandbox_command().provider_claim())
                .expect("the exact real-provider stream should remain readable")
                .is_none(),
            "missing provider state must fail before the first journal mutation"
        );
        drop(adapter);
        drop(journal);

        let (reopened, reopened_journal) = make_adapter();
        let before_inspect = snapshot_files(root);
        let inspected = inspect_attachment(&reopened, &inspect).await;
        assert!(inspected.matches_command(&inspect));
        assert!(matches!(
            inspected.into_outcome(),
            WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Ambiguous)
        ));
        assert_eq!(
            snapshot_files(root),
            before_inspect,
            "real {backend:?} {label} Inspect must not change a durable byte"
        );
        assert_eq!(
            reopened_journal
                .adopt_exact_attempt(validated.sandbox_command().provider_claim())
                .expect("the reopened provider stream should remain readable"),
            None,
            "Inspect must not create a provider result for missing state"
        );

        let replay = execute_attachment(&reopened, &execute).await;
        assert!(replay.matches_command(&execute));
        assert!(matches!(
            replay.into_outcome(),
            WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
        ));
        assert_eq!(
            reopened_journal
                .adopt_exact_attempt(validated.sandbox_command().provider_claim())
                .expect("the replayed provider stream should remain readable"),
            None,
            "a repeated fail-before rejection must not create provider authority"
        );
    }
}

#[tokio::test]
async fn real_container_attachment_execute_and_inspect_reopen_exact_missing_state() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let config = ContainerSandboxBackendConfig::under_root(root.path());
    assert_real_missing_state_reopen(SandboxBackendKind::Container, root.path(), || {
        let backend = Arc::new(ContainerSandboxBackend::new(config.clone()));
        let journal = backend
            .attempt_idempotency_journal()
            .expect("Container provider journal should open");
        let adapter = ContainerAttachmentTeardownAdapter::new(backend)
            .expect("Container attachment adapter should open its journal");
        (adapter, journal)
    })
    .await;
}

#[tokio::test]
async fn real_krun_attachment_execute_and_inspect_reopen_exact_missing_state() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let config = KrunSandboxBackendConfig::under_root(root.path());
    assert_real_missing_state_reopen(SandboxBackendKind::Krun, root.path(), || {
        let backend = Arc::new(KrunSandboxBackend::new(config.clone()));
        let journal = backend
            .attempt_idempotency_journal()
            .expect("Krun provider journal should open");
        let adapter = KrunAttachmentTeardownAdapter::new(backend)
            .expect("Krun attachment adapter should open its journal");
        (adapter, journal)
    })
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn real_container_attachment_detach_and_release_succeed_after_execution_stopped() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    assert_real_attachment_success(
        SandboxBackendKind::Container,
        root.path(),
        |stopped, detached, plan| {
            let pep_reservation = TcpListener::bind("127.0.0.1:0")
                .expect("Container fixture PEP port should reserve");
            let pep_port = pep_reservation
                .local_addr()
                .expect("Container fixture PEP address should resolve")
                .port();
            PreparedContainerNetworkTeardown::new(
                root.path(),
                stopped,
                detached,
                plan,
                pep_port,
                move || drop(pep_reservation),
            )
            .expect("exact Container network fixture should prepare")
        },
        |fixture| Arc::new(fixture.reopen()),
        |backend| {
            ContainerAttachmentTeardownAdapter::new(backend)
                .expect("Container attachment adapter should reopen")
        },
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn real_krun_attachment_detach_and_release_succeed_after_execution_stopped() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    assert_real_attachment_success(
        SandboxBackendKind::Krun,
        root.path(),
        |stopped, detached, plan| {
            let pep_reservation =
                TcpListener::bind("127.0.0.1:0").expect("Krun fixture PEP port should reserve");
            let pep_port = pep_reservation
                .local_addr()
                .expect("Krun fixture PEP address should resolve")
                .port();
            PreparedKrunNetworkTeardown::new(
                root.path(),
                stopped,
                detached,
                plan,
                pep_port,
                move || drop(pep_reservation),
            )
            .expect("exact Krun network fixture should prepare")
        },
        |fixture| Arc::new(fixture.reopen()),
        |backend| {
            KrunAttachmentTeardownAdapter::new(backend)
                .expect("Krun attachment adapter should reopen")
        },
    )
    .await;
}

#[cfg(unix)]
async fn assert_real_attachment_success<Fixture, Backend, Adapter>(
    backend_kind: SandboxBackendKind,
    root: &Path,
    prepare: impl FnOnce(
        &nimbus_sandbox::SandboxExecutionTeardownCommand,
        &nimbus_sandbox::SandboxNetworkTeardownCommand,
        nimbus_sandbox::SandboxProvisionNetworkPlan,
    ) -> Fixture,
    reopen: impl Fn(&Fixture) -> Arc<Backend>,
    make_adapter: impl Fn(Arc<Backend>) -> Adapter,
) where
    Backend: Send + Sync + 'static,
    Adapter: NetworkDetachmentCapability + NetworkReleaseCapability,
{
    let rootfs = root.join("fixture-rootfs");
    let observed = observed_record(backend_kind, &rootfs);
    let spec = sandbox_spec(backend_kind, &rootfs);
    let Ok(plan) = crate::workload_saga::provision_sandbox::sandbox_network_plan_for(
        observed.active_intent().generation(),
        observed.active_intent().network().compiled_plan(),
        &spec,
    ) else {
        panic!("the exact compiled network plan should lower for the executable");
    };

    let successor_generation = observed.active_intent().generation().as_u64() + 1;
    let profile = match backend_kind {
        SandboxBackendKind::Container => "container",
        SandboxBackendKind::Krun => "krun",
    };
    let teardown = super::begin_teardown(
        &observed,
        super::stopped_intent(profile, successor_generation),
    );
    let stopped_input = super::finish_teardown(teardown, WorkloadSagaPhase::Drained);
    let (stop, _, stopped_claimed) =
        confirmed_teardown_commands_with_claimed_record(stopped_input).await;
    let stopped = super::super::validate_sandbox_teardown_command(&stop, backend_kind)
        .expect("the exact StopExecution command should lower");
    let detached_input = apply_teardown_success(stopped_claimed, &stop);
    assert_eq!(detached_input.phase(), WorkloadSagaPhase::WorkloadStopped);
    let (detach, detach_inspect, detached_claimed) =
        confirmed_teardown_commands_with_claimed_record(detached_input).await;
    let detached =
        super::super::attachment::validate_sandbox_network_teardown_command(&detach, backend_kind)
            .expect("the exact DetachNetwork command should lower");
    let released_input = apply_teardown_success(detached_claimed, &detach);
    assert_eq!(released_input.phase(), WorkloadSagaPhase::NetworkDetached);
    let (release, release_inspect, _) =
        confirmed_teardown_commands_with_claimed_record(released_input).await;

    let fixture = prepare(stopped.sandbox_command(), detached.sandbox_command(), plan);
    let adapter = make_adapter(reopen(&fixture));
    assert_execute_succeeded(execute_attachment(&adapter, &detach).await, &detach);
    drop(adapter);

    let adapter = make_adapter(reopen(&fixture));
    let before_detach_inspect = snapshot_files(root);
    assert_inspect_satisfied(
        inspect_attachment(&adapter, &detach_inspect).await,
        &detach_inspect,
    );
    assert_eq!(
        snapshot_files(root),
        before_detach_inspect,
        "real {backend_kind:?} detach Inspect must not change a durable byte"
    );
    assert_execute_succeeded(execute_attachment(&adapter, &release).await, &release);
    drop(adapter);

    let adapter = make_adapter(reopen(&fixture));
    let before_release_inspect = snapshot_files(root);
    assert_inspect_satisfied(
        inspect_attachment(&adapter, &release_inspect).await,
        &release_inspect,
    );
    assert_eq!(
        snapshot_files(root),
        before_release_inspect,
        "real {backend_kind:?} release Inspect must not change a durable byte"
    );
}

#[tokio::test]
async fn network_execute_exact_duplicates_publish_one_result_per_operation() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    for (label, target) in [
        ("detach", WorkloadSagaPhase::WorkloadStopped),
        ("release", WorkloadSagaPhase::NetworkDetached),
    ] {
        let (execute, _) = commands_for(SandboxBackendKind::Container, target, root.path()).await;
        let validated = super::super::attachment::validate_sandbox_network_teardown_command(
            &execute,
            SandboxBackendKind::Container,
        )
        .expect("the exact Container network command should lower");
        let journal = ProviderCommandAttemptJournal::open(root.path().join(label), "network-test")
            .expect("network provider journal should open");
        let writer = journal.clone();
        let phase = super::super::ProviderTeardownPhaseAdapter::new(journal.clone());
        let effects = AtomicUsize::new(0);

        let first = phase.execute_network(
            &execute,
            &validated,
            |execution| {
                effects.fetch_add(1, Ordering::SeqCst);
                writer.record_observation(
                    execution.claim(),
                    ProviderCommandObservationKind::Succeeded,
                    b"one exact network effect",
                )
            },
            |_| panic!("a fresh Execute must not reconcile before its effect"),
        );
        let replay = phase.execute_network(
            &execute,
            &validated,
            |_| panic!("an exact terminal duplicate must not execute another effect"),
            |_| panic!("an exact terminal duplicate must not inspect the backend"),
        );

        assert_eq!(replay, first);
        let WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(
            success,
        )) = first
        else {
            panic!("the exact network effect should publish success");
        };
        assert_eq!(success.step(), execute.step());
        assert_eq!(effects.load(Ordering::SeqCst), 1);
        assert_eq!(
            journal
                .adopt_exact_attempt(validated.sandbox_command().provider_claim())
                .expect("the exact result should read")
                .expect("the exact result should exist")
                .kind(),
            ProviderCommandObservationKind::Succeeded
        );
    }
}

#[tokio::test]
async fn network_execute_reconciles_adopted_progress_and_publishes_the_result() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    for (label, target) in [
        ("detach", WorkloadSagaPhase::WorkloadStopped),
        ("release", WorkloadSagaPhase::NetworkDetached),
    ] {
        let (execute, _) = commands_for(SandboxBackendKind::Container, target, root.path()).await;
        let validated = super::super::attachment::validate_sandbox_network_teardown_command(
            &execute,
            SandboxBackendKind::Container,
        )
        .expect("the exact Container network command should lower");
        let journal = ProviderCommandAttemptJournal::open(root.path().join(label), "network-test")
            .expect("network provider journal should open");
        let claim = validated.sandbox_command().provider_claim();
        assert!(matches!(
            journal
                .claim_dispatch_epoch(claim)
                .expect("the exact network command should claim"),
            ProviderCommandClaimDecision::ExecuteClaimed(_)
        ));
        journal
            .record_observation(
                claim,
                ProviderCommandObservationKind::InProgress,
                b"provider effect may have completed",
            )
            .expect("nonterminal provider progress should publish");
        let phase = super::super::ProviderTeardownPhaseAdapter::new(journal.clone());
        let inspections = AtomicUsize::new(0);

        let reconciled = phase.execute_network(
            &execute,
            &validated,
            |_| panic!("adopted nonterminal progress must reconcile before another effect"),
            |_| {
                inspections.fetch_add(1, Ordering::SeqCst);
                SandboxNetworkTeardownObservation::Succeeded {
                    evidence: b"backend reconciliation proved success".to_vec(),
                }
            },
        );

        assert!(matches!(
            reconciled,
            WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(_))
        ));
        assert_eq!(inspections.load(Ordering::SeqCst), 1);
        assert_eq!(
            journal
                .adopt_exact_attempt(claim)
                .expect("the reconciled result should read")
                .expect("the reconciled result should exist")
                .kind(),
            ProviderCommandObservationKind::Succeeded,
            "only Execute may publish the reconciled provider result"
        );
    }
}

fn adjacent_claim(source: &ProviderCommandClaim) -> ProviderCommandClaim {
    ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: source.authority_id().to_owned(),
        effect_subject: source.effect_subject().to_owned(),
        source_attempt_id: source.source_attempt_id().map(str::to_owned),
        attempt_id: source.attempt_id().to_owned(),
        dispatch_epoch: source.dispatch_epoch() + 1,
        workload_generation: source.workload_generation(),
        restart_ordinal: source.restart_ordinal(),
        desired_digest: source.desired_digest().to_owned(),
        source_digest: source.source_digest().to_owned(),
        network_plan_digest: source.network_plan_digest().to_owned(),
        provider_target_digest: source.provider_target_digest().to_owned(),
        operation: source.operation(),
    })
    .expect("the adjacent provider claim should validate")
}

fn assert_stale_callback_cannot_mutate_successor(
    journal: &ProviderCommandAttemptJournal,
    root: &Path,
    claim: &ProviderCommandClaim,
    callback: impl FnOnce(
        ProviderCommandExecutionClaim,
    ) -> Result<ProviderCommandObservation, ProviderCommandJournalError>,
) {
    let stale_execution = match journal
        .claim_dispatch_epoch(claim)
        .expect("the old network command should claim")
    {
        ProviderCommandClaimDecision::ExecuteClaimed(execution) => execution,
        ProviderCommandClaimDecision::AdoptExactAttempt(_) => {
            panic!("a fresh old command should own its effect")
        }
    };
    journal
        .record_observation(
            claim,
            ProviderCommandObservationKind::RetryAuthorized,
            b"recovery authorized the adjacent network command",
        )
        .expect("the old command should authorize one adjacent retry");
    let successor = adjacent_claim(claim);
    assert!(matches!(
        journal
            .claim_dispatch_epoch(&successor)
            .expect("the adjacent network command should claim"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    let before = snapshot_files(root);

    assert!(matches!(
        callback(stale_execution),
        Err(ProviderCommandJournalError::StaleDispatchEpoch {
            current,
            candidate,
        }) if current == successor.dispatch_epoch() && candidate == claim.dispatch_epoch()
    ));
    assert_eq!(snapshot_files(root), before);
    assert_eq!(
        journal
            .adopt_exact_attempt(&successor)
            .expect("the successor should read")
            .expect("the successor should remain present")
            .kind(),
        ProviderCommandObservationKind::Claimed
    );
}

#[tokio::test]
async fn stale_real_backend_callbacks_cannot_mutate_successor_claims() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    for (label, target) in [
        ("detach", WorkloadSagaPhase::WorkloadStopped),
        ("release", WorkloadSagaPhase::NetworkDetached),
    ] {
        let container_root = root.path().join(format!("container-{label}"));
        let (container_command, _) =
            commands_for(SandboxBackendKind::Container, target, &container_root).await;
        let container_validated =
            super::super::attachment::validate_sandbox_network_teardown_command(
                &container_command,
                SandboxBackendKind::Container,
            )
            .expect("the exact Container network command should lower");
        let container = ContainerSandboxBackend::new(ContainerSandboxBackendConfig::under_root(
            &container_root,
        ));
        let container_journal = container
            .attempt_idempotency_journal()
            .expect("Container provider journal should open");
        assert_stale_callback_cannot_mutate_successor(
            &container_journal,
            &container_root,
            container_validated.sandbox_command().provider_claim(),
            |stale| {
                container.execute_network_teardown_with_claim(
                    container_validated.sandbox_command(),
                    stale,
                )
            },
        );

        let krun_root = root.path().join(format!("krun-{label}"));
        let (krun_command, _) = commands_for(SandboxBackendKind::Krun, target, &krun_root).await;
        let krun_validated = super::super::attachment::validate_sandbox_network_teardown_command(
            &krun_command,
            SandboxBackendKind::Krun,
        )
        .expect("the exact Krun network command should lower");
        let krun = KrunSandboxBackend::new(KrunSandboxBackendConfig::under_root(&krun_root));
        let krun_journal = krun
            .attempt_idempotency_journal()
            .expect("Krun provider journal should open");
        assert_stale_callback_cannot_mutate_successor(
            &krun_journal,
            &krun_root,
            krun_validated.sandbox_command().provider_claim(),
            |stale| {
                krun.execute_network_teardown_with_claim(krun_validated.sandbox_command(), stale)
            },
        );
    }
}
