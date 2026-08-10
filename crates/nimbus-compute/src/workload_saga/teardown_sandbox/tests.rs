use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_sandbox::SandboxBackendKind;
use nimbus_sandbox::backends::container::{ContainerSandboxBackend, ContainerSandboxBackendConfig};
use nimbus_workloads::{
    ProposedWorkloadTeardownTransition, WorkloadProvisionInspectionResult, WorkloadSagaPhase,
    WorkloadSagaRecord, WorkloadTeardownDecision, WorkloadTeardownProviderTarget,
};

use super::*;
use crate::workload_saga::provision_sandbox::{
    ContainerProvisionAdapter, validate_sandbox_provision_command,
};
use crate::workload_saga::recovery::tests::{begin_teardown, finish_teardown, stopped_intent};
use crate::workload_saga::teardown_decision::materialize_teardown_candidate;
use crate::workload_saga::teardown_registry::ExactWorkloadTeardownCapability;
use crate::workload_saga::teardown_test_support::DurableTeardownStore;
use crate::workload_saga::{
    NetworkReservationCapability, WorkloadSagaConfirmation, WorkloadSagaCoordinator,
    WorkloadTeardownCapabilityRegistry,
};

#[path = "tests/krun.rs"]
mod krun;
#[path = "tests/network_substitution.rs"]
mod network_substitution;

#[test]
fn container_execution_provider_identity_is_exact() {
    assert_eq!(
        sandbox_execution_provider_id(SandboxBackendKind::Container),
        WorkloadExecutionProviderId::for_registration_key(CONTAINER_EXECUTION_PROVIDER_KEY),
    );
}

#[cfg(unix)]
fn absent_runtime_script(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;

    let path = root.join("explicit-runtime-absence.sh");
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf '%s\\n' \"container \\`$2\\` does not exist: open \\`/run/crun/$2/status\\`: No such file or directory\" >&2\nexit 1\n",
    )
    .expect("runtime absence script should write");
    let mut permissions = std::fs::metadata(&path)
        .expect("runtime absence script should exist")
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions)
        .expect("runtime absence script should be executable");
    path
}

fn observed_container_record(rootfs: &Path) -> WorkloadSagaRecord {
    let mut record = crate::workload_saga::provision_sandbox::tests::composed_record_with_rootfs(
        SandboxBackendKind::Container,
        rootfs,
    );
    for _ in 0..8 {
        if record.phase() == WorkloadSagaPhase::Observed {
            return record;
        }
        record = crate::workload_saga::test_support::confirmed_provision(&record);
    }
    panic!("Container fixture did not reach Observed");
}

fn teardown_record_at(
    observed: &WorkloadSagaRecord,
    target: WorkloadSagaPhase,
) -> WorkloadSagaRecord {
    let successor_generation = observed.active_intent().generation().as_u64() + 1;
    let teardown = begin_teardown(observed, stopped_intent("container", successor_generation));
    finish_teardown(teardown, target)
}

async fn confirmed_teardown_commands(
    loaded: WorkloadSagaRecord,
) -> (
    ConfirmedWorkloadTeardownCommand,
    ConfirmedWorkloadTeardownCommand,
) {
    let (execute, inspect, _) = confirmed_teardown_commands_with_claimed_record(loaded).await;
    (execute, inspect)
}

async fn confirmed_teardown_commands_with_claimed_record(
    loaded: WorkloadSagaRecord,
) -> (
    ConfirmedWorkloadTeardownCommand,
    ConfirmedWorkloadTeardownCommand,
    WorkloadSagaRecord,
) {
    let WorkloadTeardownDecision::PersistCandidate(
        proposed @ ProposedWorkloadTeardownTransition::Claim { .. },
    ) = loaded
        .decide_teardown()
        .expect("fixture phase should be reducible")
    else {
        panic!("fixture phase must require a provider claim");
    };
    let candidate =
        materialize_teardown_candidate(&loaded, &proposed).expect("claim should materialize");
    let confirmed = WorkloadSagaCoordinator::new(DurableTeardownStore::with_record(loaded.clone()))
        .confirm_teardown_transition(&loaded, candidate.clone())
        .await
        .expect("teardown claim should confirm");
    assert_eq!(
        confirmed.confirmation(),
        WorkloadSagaConfirmation::AppliedByThisCall
    );
    let execute = confirmed
        .command()
        .expect("direct confirmation should produce an Execute command")
        .clone();
    let replay = WorkloadSagaCoordinator::new(DurableTeardownStore::with_record(candidate.clone()))
        .confirm_teardown_transition(&loaded, candidate.clone())
        .await
        .expect("durable replay should confirm inspection");
    let inspect = replay
        .command()
        .expect("durable replay should produce an Inspect command")
        .clone();
    assert_eq!(inspect.mode(), WorkloadTeardownCommandMode::Inspect);
    (execute, inspect, candidate)
}

#[cfg(unix)]
#[tokio::test]
async fn real_container_adapter_substitutes_drain_and_stop_with_reopened_journal_replay() {
    let root = tempfile::tempdir().expect("temporary root should exist");
    let mut config = ContainerSandboxBackendConfig::under_root(root.path())
        .with_network_state_root(root.path().join("network"));
    config.runtime_path = absent_runtime_script(root.path());
    let backend = Arc::new(ContainerSandboxBackend::new(config.clone()));
    let initial = crate::workload_saga::provision_sandbox::tests::composed_record_with_rootfs(
        SandboxBackendKind::Container,
        root.path(),
    );
    let reserve =
        crate::workload_saga::provision_provider::tests::command_for_record(initial.clone()).await;
    let Ok(validated) = validate_sandbox_provision_command(&reserve, SandboxBackendKind::Container)
    else {
        panic!("exact Container reserve command should authenticate");
    };
    assert_eq!(
        validated.sandbox_id().as_str(),
        reserve.execution().execution_id().as_str()
    );
    let provision = ContainerProvisionAdapter::new(Arc::clone(&backend))
        .expect("Container provision adapter should open its journal");
    assert!(matches!(
        NetworkReservationCapability::execute(&provision, &reserve).await,
        WorkloadProvisionInspectionResult::Succeeded { .. }
    ));

    let observed = observed_container_record(root.path());
    assert_eq!(
        observed.phase_detail().references().execution(),
        Some(reserve.execution())
    );
    let network_before = snapshot_files(&config.network_state_root);
    let adapter = Arc::new(
        ContainerTeardownAdapter::new(Arc::clone(&backend))
            .expect("Container teardown adapter should reuse the provider journal"),
    );
    let registry =
        WorkloadTeardownCapabilityRegistry::new([], [Arc::clone(&adapter).capabilities()], [])
            .expect("exact Container teardown roles should register once");

    let (drain, _drain_inspect) =
        confirmed_teardown_commands(teardown_record_at(&observed, WorkloadSagaPhase::Withdrawn))
            .await;
    assert!(matches!(
        drain.provider_target(),
        WorkloadTeardownProviderTarget::Execution { provider_id, .. }
            if provider_id == &sandbox_execution_provider_id(SandboxBackendKind::Container)
    ));
    let ExactWorkloadTeardownCapability::ExecutionDrain(drain_capability) = registry
        .select_exact(&drain)
        .expect("drain selects exactly")
    else {
        panic!("drain selected the wrong capability role");
    };
    let drain_observation = drain_capability.execute(&drain).await;
    assert!(drain_observation.matches_command(&drain));
    assert!(matches!(
        drain_observation.into_outcome(),
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(_))
    ));
    assert_eq!(snapshot_files(&config.network_state_root), network_before);

    let (stop, stop_inspect) =
        confirmed_teardown_commands(teardown_record_at(&observed, WorkloadSagaPhase::Drained))
            .await;
    let ExactWorkloadTeardownCapability::ExecutionStop(stop_capability) =
        registry.select_exact(&stop).expect("stop selects exactly")
    else {
        panic!("stop selected the wrong capability role");
    };
    let stop_observation = stop_capability.execute(&stop).await;
    assert!(stop_observation.matches_command(&stop));
    assert!(matches!(
        stop_observation.into_outcome(),
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(_))
    ));
    assert_eq!(snapshot_files(&config.network_state_root), network_before);

    let reopened = Arc::new(ContainerSandboxBackend::new(config));
    let recovered = ContainerTeardownAdapter::new(reopened)
        .expect("fresh adapter should reopen the same provider journal");
    let replay = WorkloadExecutionStopCapability::inspect(&recovered, &stop_inspect).await;
    assert!(replay.matches_command(&stop_inspect));
    assert!(matches!(
        replay.into_outcome(),
        WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Satisfied(_))
    ));
}

fn snapshot_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, current: &Path, output: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let entries = match std::fs::read_dir(current) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => panic!("snapshot directory {} failed: {error}", current.display()),
        };
        let mut entries = entries
            .map(|entry| entry.expect("snapshot entry should read"))
            .collect::<Vec<_>>();
        entries.sort_by_key(std::fs::DirEntry::path);
        for entry in entries {
            let path = entry.path();
            let metadata = entry.metadata().expect("snapshot metadata should read");
            if metadata.is_dir() {
                visit(root, &path, output);
            } else if metadata.is_file() && path.extension().is_none_or(|ext| ext != "lock") {
                output.insert(
                    path.strip_prefix(root)
                        .expect("snapshot path should stay below root")
                        .to_path_buf(),
                    std::fs::read(&path).expect("snapshot file should read"),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn record_execute_observation(
    journal: &ProviderCommandAttemptJournal,
    execution: ProviderCommandExecutionClaim,
    observation: SandboxExecutionTeardownObservation,
) -> Result<ProviderCommandObservation, ProviderCommandJournalError> {
    let kind = match &observation {
        SandboxExecutionTeardownObservation::Succeeded { .. } => {
            ProviderCommandObservationKind::Succeeded
        }
        SandboxExecutionTeardownObservation::DefiniteFailure { .. } => {
            ProviderCommandObservationKind::DefiniteFailure
        }
        SandboxExecutionTeardownObservation::InProgress { .. } => {
            ProviderCommandObservationKind::InProgress
        }
        SandboxExecutionTeardownObservation::Absent { .. }
        | SandboxExecutionTeardownObservation::RetryAuthorized { .. }
        | SandboxExecutionTeardownObservation::Ambiguous { .. } => {
            ProviderCommandObservationKind::Ambiguous
        }
    };
    journal.record_observation_with_failure_code(
        execution.claim(),
        kind,
        observation.failure_code(),
        observation.evidence(),
    )
}

#[test]
fn provider_journal_errors_keep_the_frozen_teardown_failure_vocabulary() {
    let cases = [
        (
            ProviderCommandJournalError::InvalidClaim {
                message: "invalid fixture".to_owned(),
            },
            "sandbox_teardown_command_invalid",
        ),
        (
            ProviderCommandJournalError::StaleWorkloadGeneration {
                current: 8,
                candidate: 7,
            },
            "sandbox_teardown_command_stale",
        ),
        (
            ProviderCommandJournalError::StaleDispatchEpoch {
                current: 8,
                candidate: 7,
            },
            "sandbox_teardown_command_stale",
        ),
        (
            ProviderCommandJournalError::SkippedDispatchEpoch {
                current: 7,
                candidate: 9,
            },
            "sandbox_teardown_epoch_invalid",
        ),
        (
            ProviderCommandJournalError::CrossedClaim,
            "sandbox_teardown_epoch_invalid",
        ),
        (
            ProviderCommandJournalError::RetryWithoutAuthority,
            "sandbox_teardown_epoch_invalid",
        ),
        (
            ProviderCommandJournalError::PriorEffectUnresolved,
            "sandbox_teardown_epoch_invalid",
        ),
    ];

    for (error, expected_code) in cases {
        let outcome = journal_error_outcome(WorkloadTeardownCommandMode::Execute, &error);
        let WorkloadTeardownProviderOutcome::Execute(
            WorkloadTeardownExecuteOutcome::DefiniteFailure(failure),
        ) = outcome
        else {
            panic!("deterministic journal error must fail closed: {error}");
        };
        assert_eq!(failure.code(), expected_code, "error={error}");
    }

    for error in [
        ProviderCommandJournalError::Corrupt {
            message: "corrupt fixture".to_owned(),
        },
        ProviderCommandJournalError::Store {
            message: "store fixture".to_owned(),
        },
    ] {
        assert!(matches!(
            journal_error_outcome(WorkloadTeardownCommandMode::Execute, &error),
            WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
        ));
    }
}

#[tokio::test]
async fn provider_phase_adopts_exact_results_and_fences_crossed_or_stale_effects() {
    let observed = observed_container_record(Path::new("/tmp/nnc65d1-provider-phase"));
    let (command, _) =
        confirmed_teardown_commands(teardown_record_at(&observed, WorkloadSagaPhase::Withdrawn))
            .await;
    let validated = validate_sandbox_teardown_command(&command, SandboxBackendKind::Container)
        .expect("exact teardown command should lower");

    let exact_root = tempfile::tempdir().expect("exact journal root should exist");
    let exact_journal = ProviderCommandAttemptJournal::open(exact_root.path(), "container-runtime")
        .expect("exact journal should open");
    let exact_writer = exact_journal.clone();
    let exact_phase = ProviderTeardownPhaseAdapter::new(exact_journal);
    let effects = AtomicUsize::new(0);
    let first = exact_phase.execute(&command, &validated, |execution| {
        effects.fetch_add(1, Ordering::SeqCst);
        record_execute_observation(
            &exact_writer,
            execution,
            SandboxExecutionTeardownObservation::Succeeded {
                evidence: b"exact provider result".to_vec(),
            },
        )
    });
    let replay = exact_phase.execute(&command, &validated, |_| {
        panic!("exact durable result replay must not execute the provider effect")
    });
    assert_eq!(replay, first);
    assert_eq!(effects.load(Ordering::SeqCst), 1);

    for (label, durable_claim, expected_code) in [
        (
            "crossed",
            claim_with(
                validated.sandbox_command().provider_claim(),
                None,
                Some("4".repeat(64)),
            ),
            "sandbox_teardown_epoch_invalid",
        ),
        (
            "stale",
            claim_with(
                validated.sandbox_command().provider_claim(),
                Some(
                    validated
                        .sandbox_command()
                        .provider_claim()
                        .workload_generation()
                        + 1,
                ),
                None,
            ),
            "sandbox_teardown_command_stale",
        ),
    ] {
        let root = tempfile::tempdir().expect("fenced journal root should exist");
        let journal = ProviderCommandAttemptJournal::open(root.path(), "container-runtime")
            .expect("fenced journal should open");
        assert!(matches!(
            journal
                .claim_dispatch_epoch(&durable_claim)
                .expect("conflicting durable claim should publish"),
            ProviderCommandClaimDecision::ExecuteClaimed(_)
        ));
        let phase = ProviderTeardownPhaseAdapter::new(journal);
        let outcome = phase.execute(&command, &validated, |_| {
            panic!("{label} claim must fail before the provider effect")
        });
        let WorkloadTeardownProviderOutcome::Execute(
            WorkloadTeardownExecuteOutcome::DefiniteFailure(failure),
        ) = outcome
        else {
            panic!("{label} claim must return a definite failure");
        };
        assert_eq!(failure.code(), expected_code, "case={label}");
    }
}

#[tokio::test]
async fn stop_retry_authority_is_inspect_only_and_maps_to_not_completed() {
    let observed = observed_container_record(Path::new("/tmp/nnc65d1-retry-authority"));
    let (execute, inspect) =
        confirmed_teardown_commands(teardown_record_at(&observed, WorkloadSagaPhase::Drained))
            .await;
    let validated_execute =
        validate_sandbox_teardown_command(&execute, SandboxBackendKind::Container)
            .expect("exact stop Execute command should lower");
    let validated_inspect =
        validate_sandbox_teardown_command(&inspect, SandboxBackendKind::Container)
            .expect("exact stop Inspect command should lower");

    let execute_root = tempfile::tempdir().expect("execute journal root should exist");
    let execute_journal =
        ProviderCommandAttemptJournal::open(execute_root.path(), "container-runtime")
            .expect("execute journal should open");
    let execute_reader = execute_journal.clone();
    let execute_writer = execute_journal.clone();
    let execute_phase = ProviderTeardownPhaseAdapter::new(execute_journal);
    assert!(matches!(
        execute_phase.execute(&execute, &validated_execute, |execution| {
            record_execute_observation(
                &execute_writer,
                execution,
                SandboxExecutionTeardownObservation::RetryAuthorized {
                    evidence: b"backend unexpectedly returned retry authority from Execute"
                        .to_vec(),
                },
            )
        }),
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
    ));
    assert_eq!(
        execute_reader
            .adopt_exact_attempt(validated_execute.sandbox_command().provider_claim())
            .expect("execute observation should read")
            .expect("execute observation should exist")
            .kind(),
        ProviderCommandObservationKind::Ambiguous,
        "Execute must not mint next-epoch retry authority"
    );

    let inspect_root = tempfile::tempdir().expect("inspect journal root should exist");
    let inspect_journal =
        ProviderCommandAttemptJournal::open(inspect_root.path(), "container-runtime")
            .expect("inspect journal should open");
    assert!(matches!(
        inspect_journal
            .claim_dispatch_epoch(validated_inspect.sandbox_command().provider_claim())
            .expect("the exact stop epoch should claim"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    let inspect_reader = inspect_journal.clone();
    let inspect_phase = ProviderTeardownPhaseAdapter::new(inspect_journal);
    assert!(matches!(
        inspect_phase.inspect(&inspect, &validated_inspect, |_| {
            SandboxExecutionTeardownObservation::RetryAuthorized {
                evidence: b"same exact process remains live after reconciliation deadline".to_vec(),
            }
        }),
        WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::NotCompleted(_))
    ));
    assert_eq!(
        inspect_reader
            .adopt_exact_attempt(validated_inspect.sandbox_command().provider_claim())
            .expect("inspect observation should read")
            .expect("inspect observation should exist")
            .kind(),
        ProviderCommandObservationKind::RetryAuthorized
    );
}

#[tokio::test]
async fn execute_keeps_its_published_result_when_inspection_advances_the_journal() {
    let observed = observed_container_record(Path::new("/tmp/nnc65d1-published-result"));
    let (execute, _) =
        confirmed_teardown_commands(teardown_record_at(&observed, WorkloadSagaPhase::Drained))
            .await;
    let validated = validate_sandbox_teardown_command(&execute, SandboxBackendKind::Container)
        .expect("exact stop Execute command should lower");
    let root = tempfile::tempdir().expect("execute journal root should exist");
    let journal = ProviderCommandAttemptJournal::open(root.path(), "container-runtime")
        .expect("execute journal should open");
    let writer = journal.clone();
    let reader = journal.clone();
    let phase = ProviderTeardownPhaseAdapter::new(journal);

    let outcome = phase.execute(&execute, &validated, |execution| {
        let claim = execution.claim().clone();
        let published = record_execute_observation(
            &writer,
            execution,
            SandboxExecutionTeardownObservation::InProgress {
                evidence: b"the exact TERM effect is in progress".to_vec(),
            },
        )?;
        writer.record_observation(
            &claim,
            ProviderCommandObservationKind::RetryAuthorized,
            b"inspection authenticated the adjacent retry",
        )?;
        Ok(published)
    });

    assert!(matches!(
        outcome,
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
    ));
    assert_eq!(
        reader
            .adopt_exact_attempt(validated.sandbox_command().provider_claim())
            .expect("latest journal observation should read")
            .expect("latest journal observation should exist")
            .kind(),
        ProviderCommandObservationKind::RetryAuthorized
    );
}

fn claim_with(
    source: &ProviderCommandClaim,
    workload_generation: Option<u64>,
    desired_digest: Option<String>,
) -> ProviderCommandClaim {
    ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: source.authority_id().to_owned(),
        effect_subject: source.effect_subject().to_owned(),
        source_attempt_id: source.source_attempt_id().map(str::to_owned),
        attempt_id: source.attempt_id().to_owned(),
        dispatch_epoch: source.dispatch_epoch(),
        workload_generation: workload_generation.unwrap_or_else(|| source.workload_generation()),
        restart_ordinal: source.restart_ordinal(),
        desired_digest: desired_digest.unwrap_or_else(|| source.desired_digest().to_owned()),
        source_digest: source.source_digest().to_owned(),
        network_plan_digest: source.network_plan_digest().to_owned(),
        provider_target_digest: source.provider_target_digest().to_owned(),
        operation: source.operation(),
    })
    .expect("derived teardown claim should validate")
}

#[tokio::test]
async fn network_inspect_unclaimed_is_not_completed_and_byte_stable() {
    let observed = observed_container_record(Path::new("/tmp/nnc65d3-network-unclaimed"));
    let (_, inspect) = confirmed_teardown_commands(teardown_record_at(
        &observed,
        WorkloadSagaPhase::WorkloadStopped,
    ))
    .await;
    let validated = attachment::validate_sandbox_network_teardown_command(
        &inspect,
        SandboxBackendKind::Container,
    )
    .expect("exact Container detach inspection should lower");
    let root = tempfile::tempdir().expect("network journal root should exist");
    let journal = ProviderCommandAttemptJournal::open(root.path(), "container-network")
        .expect("network journal should open");
    let phase = ProviderTeardownPhaseAdapter::new(journal);
    let before = snapshot_files(root.path());

    let outcome = phase.inspect_network(&inspect, &validated, |_| {
        panic!("an unclaimed provider command must not inspect the backend")
    });

    assert_eq!(
        outcome,
        WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::NotCompleted(
            WorkloadOwnerEvidenceDigest::sha256(
                b"sandbox network provider command was never claimed",
            )
        ),)
    );
    assert_eq!(snapshot_files(root.path()), before);
}

#[tokio::test]
async fn network_inspect_success_is_query_only_and_cannot_publish_provider_result() {
    let observed = observed_container_record(Path::new("/tmp/nnc65d3-network-query-success"));
    let (_, inspect) = confirmed_teardown_commands(teardown_record_at(
        &observed,
        WorkloadSagaPhase::WorkloadStopped,
    ))
    .await;
    let validated = attachment::validate_sandbox_network_teardown_command(
        &inspect,
        SandboxBackendKind::Container,
    )
    .expect("exact Container detach inspection should lower");
    let root = tempfile::tempdir().expect("network journal root should exist");
    let journal = ProviderCommandAttemptJournal::open(root.path(), "container-network")
        .expect("network journal should open");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(validated.sandbox_command().provider_claim())
            .expect("network command should claim"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    let reader = journal.clone();
    let phase = ProviderTeardownPhaseAdapter::new(journal);
    let before = snapshot_files(root.path());
    let evidence = b"backend reports that the attachment is absent";

    let outcome = phase.inspect_network(&inspect, &validated, |_| {
        SandboxNetworkTeardownObservation::Succeeded {
            evidence: evidence.to_vec(),
        }
    });

    let WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::InProgress(
        observed_evidence,
    )) = outcome
    else {
        panic!("backend proof without provider-journal success must remain in progress");
    };
    assert_eq!(
        observed_evidence,
        WorkloadOwnerEvidenceDigest::sha256(evidence),
        "backend proof is evidence for reconciliation, not provider success"
    );
    assert_eq!(snapshot_files(root.path()), before);
    assert_eq!(
        reader
            .adopt_exact_attempt(validated.sandbox_command().provider_claim())
            .expect("claimed network command should remain readable")
            .expect("claimed network command should remain present")
            .kind(),
        ProviderCommandObservationKind::Claimed,
        "Inspect must not publish backend success to the provider journal"
    );
}

#[tokio::test]
async fn network_inspect_replays_exact_terminal_journal_without_backend_query() {
    let observed = observed_container_record(Path::new("/tmp/nnc65d3-network-terminal-replay"));
    let (_, inspect) = confirmed_teardown_commands(teardown_record_at(
        &observed,
        WorkloadSagaPhase::WorkloadStopped,
    ))
    .await;
    let validated = attachment::validate_sandbox_network_teardown_command(
        &inspect,
        SandboxBackendKind::Container,
    )
    .expect("exact Container detach inspection should lower");
    let root = tempfile::tempdir().expect("network journal root should exist");
    let journal = ProviderCommandAttemptJournal::open(root.path(), "container-network")
        .expect("network journal should open");
    assert!(matches!(
        journal
            .claim_dispatch_epoch(validated.sandbox_command().provider_claim())
            .expect("network command should claim"),
        ProviderCommandClaimDecision::ExecuteClaimed(_)
    ));
    let terminal = journal
        .record_observation(
            validated.sandbox_command().provider_claim(),
            ProviderCommandObservationKind::Succeeded,
            b"durable exact detach result",
        )
        .expect("terminal network result should publish");
    let expected = provider_outcome(&inspect, &terminal);
    let phase = ProviderTeardownPhaseAdapter::new(journal);
    let before = snapshot_files(root.path());

    let replay = phase.inspect_network(&inspect, &validated, |_| {
        panic!("a terminal provider result must replay without a backend query")
    });

    assert_eq!(replay, expected);
    assert_eq!(snapshot_files(root.path()), before);
}

#[tokio::test]
async fn network_teardown_lowering_preserves_exact_attachment_and_execution_fences() {
    let observed = observed_container_record(Path::new("/tmp/nnc65d3-network-lowering"));
    let (detach, _) = confirmed_teardown_commands(teardown_record_at(
        &observed,
        WorkloadSagaPhase::WorkloadStopped,
    ))
    .await;
    let detach = attachment::validate_sandbox_network_teardown_command(
        &detach,
        SandboxBackendKind::Container,
    )
    .expect("exact Container detach command should lower");
    let detach = detach.sandbox_command();
    let compiled = observed.active_intent().network().compiled_plan();
    let expected_attachment = compiled
        .content()
        .attachment()
        .expect("host-managed plan should have one attachment");
    let expected_selection = compiled
        .content()
        .capability_selection_evidence()
        .expect("host-managed plan should retain selection evidence");
    let retained_references = observed.phase_detail().references();
    let execution = retained_references
        .execution()
        .expect("observed workload should retain its execution");

    assert_eq!(detach.tenant_id(), observed.key().tenant_id());
    assert_eq!(
        detach.sandbox_id().as_str(),
        execution.execution_id().as_str()
    );
    assert_eq!(
        detach.execution_attempt_id().as_str(),
        execution.attempt_id().as_str()
    );
    assert_eq!(detach.attachment_id(), expected_attachment.attachment_id());
    assert_eq!(detach.network_plan(), compiled.plan());
    assert_eq!(
        detach.provider_registration_key(),
        nimbus_sandbox::backends::CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY
    );
    assert_eq!(
        detach.provider_source_digest(),
        expected_selection.source_digest()
    );
    assert_eq!(
        detach.operation(),
        nimbus_sandbox::SandboxNetworkTeardownOperation::Detach
    );
    assert_eq!(
        detach.provider_claim().operation(),
        ProviderCommandOperation::DetachNetwork
    );

    let (release_command, _) = confirmed_teardown_commands(teardown_record_at(
        &observed,
        WorkloadSagaPhase::NetworkDetached,
    ))
    .await;
    let release = attachment::validate_sandbox_network_teardown_command(
        &release_command,
        SandboxBackendKind::Container,
    )
    .expect("exact Container release command should lower");
    assert_eq!(
        release.sandbox_command().operation(),
        nimbus_sandbox::SandboxNetworkTeardownOperation::Release
    );
    assert_eq!(
        release.sandbox_command().provider_claim().operation(),
        ProviderCommandOperation::ReleaseNetwork
    );
    assert_eq!(
        release.sandbox_command().provider_claim().effect_subject(),
        detach.provider_claim().effect_subject(),
        "detach and release use independent operations over one exact subject"
    );

    let crossed = attachment::validate_sandbox_network_teardown_command(
        &release_command,
        SandboxBackendKind::Krun,
    )
    .expect_err("a Container attachment command cannot lower through Krun");
    assert_eq!(crossed.code(), "sandbox_teardown_command_crossed");
}
