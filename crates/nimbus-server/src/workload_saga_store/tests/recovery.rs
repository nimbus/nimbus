use std::collections::BTreeSet;

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::PublishedEndpointId;
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, WORKLOAD_SAGA_RECOVERY_ORDER,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadDesiredDigest,
    WorkloadEffectReferences, WorkloadFailureEvidence, WorkloadGeneration,
    WorkloadInspectionRequirement, WorkloadNetworkIntent, WorkloadOwnerEvidenceDigest,
    WorkloadOwnerObservation, WorkloadPhaseDetail, WorkloadPublicationIntent,
    WorkloadPublicationReference, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaIntent,
    WorkloadSagaIntentUpdate, WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaRecoveryCursor, WorkloadSagaStore, WorkloadTerminalEvidenceDigest,
    WorkloadTerminalObservation,
};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use super::super::EngineWorkloadSagaStore;
use super::{compiled_network_plan, engine};

struct RecoveryFixtures {
    recoverable: Vec<Vec<WorkloadSagaRecord>>,
    quiescent: Vec<Vec<WorkloadSagaRecord>>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ProcessMatrixExpectation {
    pub(super) label: &'static str,
    pub(super) phase: WorkloadSagaPhase,
    pub(super) target: WorkloadSagaPhase,
    pub(super) action: &'static str,
}

pub(super) const PROCESS_MATRIX_EXPECTATIONS: &[ProcessMatrixExpectation] = &[
    process_case(
        "process-intent",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadSagaPhase::NetworkReserved,
        "reserve-network",
    ),
    process_case(
        "process-network-reserved",
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        "prepare-workload",
    ),
    process_case(
        "process-workload-prepared",
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        "attach-network",
    ),
    process_case(
        "process-network-attached",
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        "activate-workload",
    ),
    process_case(
        "process-workload-activated",
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        "inspect-readiness",
    ),
    process_case(
        "process-ready",
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Published,
        "publish",
    ),
    process_case(
        "process-published",
        WorkloadSagaPhase::Published,
        WorkloadSagaPhase::Observed,
        "observe-publication",
    ),
    process_case(
        "process-observed",
        WorkloadSagaPhase::Observed,
        WorkloadSagaPhase::Observed,
        "quiescent",
    ),
    process_case(
        "process-withdrawal",
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        "withdraw-publication",
    ),
    process_case(
        "process-withdrawn",
        WorkloadSagaPhase::Withdrawn,
        WorkloadSagaPhase::Drained,
        "drain-workload",
    ),
    process_case(
        "process-drained",
        WorkloadSagaPhase::Drained,
        WorkloadSagaPhase::WorkloadStopped,
        "stop-workload",
    ),
    process_case(
        "process-workload-stopped",
        WorkloadSagaPhase::WorkloadStopped,
        WorkloadSagaPhase::NetworkDetached,
        "detach-network",
    ),
    process_case(
        "process-network-detached",
        WorkloadSagaPhase::NetworkDetached,
        WorkloadSagaPhase::NetworkReleased,
        "release-network",
    ),
    process_case(
        "process-network-released",
        WorkloadSagaPhase::NetworkReleased,
        WorkloadSagaPhase::Recorded,
        "record-terminal-evidence",
    ),
    process_case(
        "process-recorded-stopped-successor",
        WorkloadSagaPhase::Recorded,
        WorkloadSagaPhase::Recorded,
        "promote-successor-stopped",
    ),
    process_case(
        "process-cleanup-complete",
        WorkloadSagaPhase::CleanupPending,
        WorkloadSagaPhase::CleanupPending,
        "inspect-cleanup",
    ),
    process_case(
        "process-attached-prepare-only",
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::NetworkAttached,
        "quiescent",
    ),
    process_case(
        "process-ready-withheld",
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Observed,
        "advance-without-effect",
    ),
    process_case(
        "process-recorded-quiescent",
        WorkloadSagaPhase::Recorded,
        WorkloadSagaPhase::Recorded,
        "quiescent",
    ),
    process_case(
        "process-recorded-running-successor",
        WorkloadSagaPhase::Recorded,
        WorkloadSagaPhase::IntentCommitted,
        "promote-successor-running",
    ),
    process_case(
        "process-cleanup-network",
        WorkloadSagaPhase::CleanupPending,
        WorkloadSagaPhase::CleanupPending,
        "inspect-cleanup",
    ),
    process_case(
        "process-cleanup-execution",
        WorkloadSagaPhase::CleanupPending,
        WorkloadSagaPhase::CleanupPending,
        "inspect-cleanup",
    ),
    process_case(
        "process-successor-from-intent",
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        "advance-without-effect",
    ),
    process_case(
        "process-successor-from-network-reserved",
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        "advance-without-effect",
    ),
    process_case(
        "process-successor-from-workload-prepared",
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        "advance-without-effect",
    ),
    process_case(
        "process-successor-from-network-attached",
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        "advance-without-effect",
    ),
    process_case(
        "process-successor-from-workload-activated",
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        "advance-without-effect",
    ),
    process_case(
        "process-successor-from-ready",
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        "withdraw-publication",
    ),
    process_case(
        "process-successor-from-published",
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        "withdraw-publication",
    ),
    process_case(
        "process-successor-from-observed",
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        "withdraw-publication",
    ),
];

const fn process_case(
    label: &'static str,
    phase: WorkloadSagaPhase,
    target: WorkloadSagaPhase,
    action: &'static str,
) -> ProcessMatrixExpectation {
    ProcessMatrixExpectation {
        label,
        phase,
        target,
        action,
    }
}

#[test]
fn all_phases_use_portable_record_recovery_eligibility() {
    let fixtures = recovery_fixtures();
    let phases = fixtures
        .recoverable
        .iter()
        .chain(&fixtures.quiescent)
        .map(|history| latest(history).phase())
        .collect::<BTreeSet<_>>();
    let expected_phases = WORKLOAD_SAGA_RECOVERY_ORDER
        .into_iter()
        .collect::<BTreeSet<_>>();

    assert_eq!(phases, expected_phases);
    assert!(
        fixtures
            .recoverable
            .iter()
            .all(|history| latest(history).requires_recovery())
    );
    assert!(
        fixtures
            .quiescent
            .iter()
            .all(|history| !latest(history).requires_recovery())
    );

    let recorded_successor = fixtures
        .recoverable
        .iter()
        .map(|history| latest(history))
        .find(|record| record.phase() == WorkloadSagaPhase::Recorded)
        .expect("recoverable fixtures must include Recorded plus a successor");
    assert!(recorded_successor.successor_intent().is_some());
}

#[tokio::test]
async fn durable_recovery_pages_are_bounded_stable_cursor_strict_and_complete() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let writer_engine = engine(&root);
    let writer_lifetime = std::sync::Arc::downgrade(&writer_engine);
    let store = EngineWorkloadSagaStore::new(std::sync::Arc::clone(&writer_engine));
    let fixtures = recovery_fixtures();

    for history in fixtures.recoverable.iter().chain(&fixtures.quiescent) {
        persist_history(&store, history).await;
    }

    let mut expected = fixtures
        .recoverable
        .iter()
        .map(|history| latest(history).clone())
        .collect::<Vec<_>>();
    expected.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));

    let (first, first_page_sizes) = collect_recovery_pages(&store, 4).await;
    let (second, second_page_sizes) = collect_recovery_pages(&store, 4).await;
    assert_eq!(first_page_sizes, vec![4, 4, 4, 3]);
    assert_eq!(second_page_sizes, first_page_sizes);
    assert_eq!(first, expected);
    assert_eq!(second, expected, "repeated recovery reads must be stable");

    let returned = first
        .iter()
        .map(|record| record.saga_id().clone())
        .collect::<BTreeSet<_>>();
    for history in &fixtures.quiescent {
        assert!(
            !returned.contains(latest(history).saga_id()),
            "quiescent {:?} record must be excluded",
            latest(history).phase()
        );
    }
    assert!(first.iter().any(|record| {
        record.phase() == WorkloadSagaPhase::Recorded && record.successor_intent().is_some()
    }));

    drop(store);
    drop(writer_engine);
    assert!(
        writer_lifetime.upgrade().is_none(),
        "every writer Engine handle must be gone before recovery reopens durable truth"
    );
    let reopened = EngineWorkloadSagaStore::new(engine(&root));
    let (after_reopen, reopened_page_sizes) = collect_recovery_pages(&reopened, 4).await;
    assert_eq!(reopened_page_sizes, vec![4, 4, 4, 3]);
    assert_eq!(
        after_reopen, expected,
        "a fresh Engine must decode every tagged durable phase without snapshot handoff"
    );
}

#[tokio::test]
async fn recovery_cursor_does_not_repeat_a_saga_that_advances_between_pages() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let mut records = [
        "cursor-transition-a",
        "cursor-transition-b",
        "cursor-transition-c",
    ]
    .map(|label| {
        provision_history(
            label,
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        )
        .into_iter()
        .next()
        .expect("provision history starts at IntentCommitted")
    });
    records.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
    for record in &records {
        persist_history(&store, std::slice::from_ref(record)).await;
    }

    let first = store
        .list_recoverable(WorkloadSagaPageRequest::new(None, 2).unwrap())
        .await
        .expect("first recovery page should load");
    assert_eq!(first.records(), &records[..2]);
    let after = first
        .next_cursor()
        .cloned()
        .expect("a full first page should carry a cursor");

    let advanced = advance_provision(&records[0], WorkloadSagaPhase::NetworkReserved, None);
    assert_eq!(
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(records[0].revision()),
                advanced,
            )
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );

    let second = store
        .list_recoverable(WorkloadSagaPageRequest::new(Some(after), 2).unwrap())
        .await
        .expect("second recovery page should load");
    assert_eq!(second.records(), &records[2..]);
    assert!(
        second
            .records()
            .iter()
            .all(|record| record.saga_id() != records[0].saga_id()),
        "an immutable recovery cursor must not return a saga twice after its phase advances"
    );
}

async fn collect_recovery_pages(
    store: &EngineWorkloadSagaStore,
    limit: u16,
) -> (Vec<WorkloadSagaRecord>, Vec<usize>) {
    let mut after = None;
    let mut records = Vec::new();
    let mut page_sizes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut previous = None;

    for _ in 0..32 {
        let request = WorkloadSagaPageRequest::new(after.clone(), limit)
            .expect("fixture page request is valid");
        let page = store
            .list_recoverable(request)
            .await
            .expect("recovery page should load");
        assert!(page.records().len() <= usize::from(limit));

        for record in page.records() {
            assert!(record.requires_recovery());
            let cursor = WorkloadSagaRecoveryCursor::for_record(record)
                .expect("recoverable record has a cursor");
            if let Some(previous) = previous.as_ref() {
                assert!(cursor_key(&cursor) > cursor_key(previous));
            }
            assert!(
                seen.insert(record.saga_id().clone()),
                "recovery pages must not duplicate a saga"
            );
            previous = Some(cursor);
        }

        let next = page.next_cursor().cloned();
        if let Some(next) = next.as_ref() {
            assert_eq!(page.records().len(), usize::from(limit));
            assert_eq!(Some(next), previous.as_ref());
        }
        page_sizes.push(page.records().len());
        records.extend(page.into_records());
        after = next;
        if after.is_none() {
            return (records, page_sizes);
        }
    }

    panic!("recovery pagination did not terminate within the fixture bound");
}

fn cursor_key(cursor: &WorkloadSagaRecoveryCursor) -> &str {
    cursor.saga_id().as_str()
}

async fn persist_history(store: &EngineWorkloadSagaStore, history: &[WorkloadSagaRecord]) {
    for (index, record) in history.iter().enumerate() {
        let expected = index
            .checked_sub(1)
            .map_or(WorkloadSagaExpected::Missing, |previous| {
                WorkloadSagaExpected::Revision(history[previous].revision())
            });
        assert_eq!(
            store.compare_and_swap(expected, record.clone()).await,
            Ok(WorkloadSagaCommit::Applied),
            "fixture transition into {:?} must persist",
            record.phase()
        );
    }
}

fn recovery_fixtures() -> RecoveryFixtures {
    let mut recoverable = Vec::new();
    for (index, phase) in [
        WorkloadSagaPhase::IntentCommitted,
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Published,
    ]
    .into_iter()
    .enumerate()
    {
        recoverable.push(history_through_phase(
            provision_history(
                &format!("recover-provision-{index}"),
                WorkloadActivationIntent::ActivateWhenAttached,
                WorkloadPublicationIntent::PublishWhenReady,
            ),
            phase,
        ));
    }

    for (index, phase) in [
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        WorkloadSagaPhase::Drained,
        WorkloadSagaPhase::WorkloadStopped,
        WorkloadSagaPhase::NetworkDetached,
        WorkloadSagaPhase::NetworkReleased,
        WorkloadSagaPhase::Recorded,
    ]
    .into_iter()
    .enumerate()
    {
        recoverable.push(history_through_phase(
            teardown_history(&format!("recover-teardown-{index}")),
            phase,
        ));
    }
    recoverable.push(cleanup_pending_history("recover-cleanup"));

    let quiescent = vec![
        history_through_phase(
            provision_history(
                "quiescent-observed",
                WorkloadActivationIntent::ActivateWhenAttached,
                WorkloadPublicationIntent::PublishWhenReady,
            ),
            WorkloadSagaPhase::Observed,
        ),
        provision_history(
            "quiescent-prepare-only",
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        ),
        vec![stopped_record("quiescent-recorded")],
    ];

    RecoveryFixtures {
        recoverable,
        quiescent,
    }
}

fn provision_history(
    label: &str,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
) -> Vec<WorkloadSagaRecord> {
    let key = workload_key(label);
    let intent = workload_intent(
        &key,
        1,
        DesiredWorkloadState::Running,
        activation,
        publication,
    );
    let publication_reference =
        (publication == WorkloadPublicationIntent::PublishWhenReady).then(|| {
            WorkloadPublicationReference::new([publication_endpoint_id(label)], &intent)
                .expect("fixture publication reference is valid")
        });
    let mut history =
        vec![WorkloadSagaRecord::new(key, intent).expect("fixture saga record should initialize")];

    for phase in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
    ] {
        history.push(advance_provision(
            latest(&history),
            phase,
            publication_reference.as_ref(),
        ));
    }
    if activation == WorkloadActivationIntent::PrepareOnly {
        return history;
    }

    for phase in [
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
    ] {
        history.push(advance_provision(
            latest(&history),
            phase,
            publication_reference.as_ref(),
        ));
    }
    if publication == WorkloadPublicationIntent::PublishWhenReady {
        history.push(advance_provision(
            latest(&history),
            WorkloadSagaPhase::Published,
            publication_reference.as_ref(),
        ));
    }
    history.push(advance_provision(
        latest(&history),
        WorkloadSagaPhase::Observed,
        publication_reference.as_ref(),
    ));
    history
}

fn advance_provision(
    record: &WorkloadSagaRecord,
    phase: WorkloadSagaPhase,
    publication: Option<&WorkloadPublicationReference>,
) -> WorkloadSagaRecord {
    let publication = if record.active_intent().publication()
        == WorkloadPublicationIntent::PublishWhenReady
        && matches!(
            phase,
            WorkloadSagaPhase::Ready | WorkloadSagaPhase::Published | WorkloadSagaPhase::Observed
        ) {
        publication.cloned()
    } else {
        None
    };
    let references = WorkloadEffectReferences::provision(record.active_intent(), publication)
        .expect("fixture provision references are valid");
    let observations =
        provision_observations(phase, &references, record.active_intent().publication());
    let detail =
        WorkloadPhaseDetail::provision(phase, record.active_intent(), references, observations)
            .expect("fixture provision detail is valid");
    record
        .advance(phase, detail, None)
        .expect("fixture provision transition is valid")
}

fn provision_observations(
    phase: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
    publication: WorkloadPublicationIntent,
) -> Vec<WorkloadOwnerObservation> {
    let network = references
        .network()
        .expect("provision fixture has a network reference")
        .clone();
    let execution = references
        .execution()
        .expect("provision fixture has an execution reference")
        .clone();
    let rank = match phase {
        WorkloadSagaPhase::NetworkReserved => 1,
        WorkloadSagaPhase::WorkloadPrepared => 2,
        WorkloadSagaPhase::NetworkAttached => 3,
        WorkloadSagaPhase::WorkloadActivated => 4,
        WorkloadSagaPhase::Ready => 5,
        WorkloadSagaPhase::Published => 6,
        WorkloadSagaPhase::Observed
            if publication == WorkloadPublicationIntent::PublishWhenReady =>
        {
            6
        }
        WorkloadSagaPhase::Observed => 5,
        _ => panic!("fixture phase is not a provision phase"),
    };
    let mut observations = Vec::new();
    if rank >= 1 {
        observations.push(WorkloadOwnerObservation::NetworkReserved {
            reference: network.clone(),
            evidence: evidence("network-reserved"),
        });
    }
    if rank >= 2 {
        observations.push(WorkloadOwnerObservation::ExecutionPrepared {
            reference: execution.clone(),
            evidence: evidence("execution-prepared"),
        });
    }
    if rank >= 3 {
        observations.push(WorkloadOwnerObservation::NetworkAttached {
            reference: network.clone(),
            evidence: evidence("network-attached"),
        });
    }
    if rank >= 4 {
        observations.push(WorkloadOwnerObservation::ExecutionActivated {
            reference: execution.clone(),
            evidence: evidence("execution-activated"),
        });
    }
    if rank >= 5 {
        observations.push(WorkloadOwnerObservation::Ready {
            network,
            execution,
            evidence: evidence("ready"),
        });
    }
    if rank >= 6 {
        observations.push(WorkloadOwnerObservation::PublicationPresent {
            reference: references
                .publication()
                .expect("published fixture has a publication reference")
                .clone(),
            evidence: evidence("publication-present"),
        });
    }
    observations
}

fn teardown_history(label: &str) -> Vec<WorkloadSagaRecord> {
    teardown_history_with_successor(label, DesiredWorkloadState::Stopped)
}

fn teardown_history_with_successor(
    label: &str,
    successor_state: DesiredWorkloadState,
) -> Vec<WorkloadSagaRecord> {
    let mut history = provision_history(
        label,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let current = latest(&history);
    let successor = workload_intent(
        current.key(),
        2,
        successor_state,
        if successor_state == DesiredWorkloadState::Running {
            WorkloadActivationIntent::ActivateWhenAttached
        } else {
            WorkloadActivationIntent::PrepareOnly
        },
        WorkloadPublicationIntent::Withheld,
    );
    let WorkloadSagaIntentUpdate::Transition(withdrawal) = current
        .apply_intent(successor)
        .expect("successor should queue")
    else {
        panic!("higher stopped intent must start withdrawal");
    };
    history.push(*withdrawal);

    for phase in [
        WorkloadSagaPhase::Withdrawn,
        WorkloadSagaPhase::Drained,
        WorkloadSagaPhase::WorkloadStopped,
        WorkloadSagaPhase::NetworkDetached,
        WorkloadSagaPhase::NetworkReleased,
    ] {
        history.push(advance_teardown(latest(&history), phase));
    }
    let WorkloadPhaseDetail::Teardown(detail) = latest(&history).phase_detail() else {
        panic!("network release fixture has teardown detail");
    };
    let terminal_digest =
        WorkloadTerminalEvidenceDigest::for_observations(detail.terminal_observations())
            .expect("terminal observations should digest");
    let recorded = latest(&history)
        .advance(
            WorkloadSagaPhase::Recorded,
            WorkloadPhaseDetail::recorded(latest(&history).active_intent(), terminal_digest),
            None,
        )
        .expect("recorded transition should validate");
    history.push(recorded);
    history
}

fn advance_teardown(record: &WorkloadSagaRecord, phase: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let WorkloadPhaseDetail::Teardown(current) = record.phase_detail() else {
        panic!("teardown fixture has teardown detail");
    };
    let references = current.retained_references().clone();
    let detail = WorkloadPhaseDetail::teardown(
        phase,
        record.active_intent(),
        current.origin(),
        references.clone(),
        terminal_observations(phase, &references),
    )
    .expect("fixture teardown detail is valid");
    record
        .advance(phase, detail, None)
        .expect("fixture teardown transition is valid")
}

fn terminal_observations(
    phase: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
) -> Vec<WorkloadTerminalObservation> {
    let rank = match phase {
        WorkloadSagaPhase::Withdrawn => 1,
        WorkloadSagaPhase::Drained => 2,
        WorkloadSagaPhase::WorkloadStopped => 3,
        WorkloadSagaPhase::NetworkDetached => 4,
        WorkloadSagaPhase::NetworkReleased => 5,
        _ => panic!("fixture phase is not a teardown phase"),
    };
    let mut observations = Vec::new();
    if rank >= 1 {
        observations.push(WorkloadTerminalObservation::PublicationAbsent {
            reference: references
                .publication()
                .expect("withdrawal fixture has a publication reference")
                .clone(),
            evidence: evidence("publication-absent"),
        });
    }
    if rank >= 2 {
        observations.push(WorkloadTerminalObservation::ExecutionDrained {
            reference: references
                .execution()
                .expect("withdrawal fixture has an execution reference")
                .clone(),
            evidence: evidence("execution-drained"),
        });
    }
    if rank >= 3 {
        observations.push(WorkloadTerminalObservation::ExecutionStopped {
            reference: references
                .execution()
                .expect("withdrawal fixture has an execution reference")
                .clone(),
            evidence: evidence("execution-stopped"),
        });
    }
    if rank >= 4 {
        observations.push(WorkloadTerminalObservation::NetworkDetached {
            reference: references
                .network()
                .expect("withdrawal fixture has a network reference")
                .clone(),
            evidence: evidence("network-detached"),
        });
    }
    if rank >= 5 {
        observations.push(WorkloadTerminalObservation::NetworkReleased {
            reference: references
                .network()
                .expect("withdrawal fixture has a network reference")
                .clone(),
            evidence: evidence("network-released"),
        });
    }
    observations
}

fn cleanup_pending_history(label: &str) -> Vec<WorkloadSagaRecord> {
    let mut history = history_through_phase(
        provision_history(
            label,
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::PublishWhenReady,
        ),
        WorkloadSagaPhase::Ready,
    );
    let current = latest(&history);
    let references = current.phase_detail().references();
    let mut inspections = Vec::new();
    if let Some(reference) = references.network() {
        inspections.push(WorkloadInspectionRequirement::Network {
            reference: reference.clone(),
            expected_phase: current.phase(),
        });
    }
    if let Some(reference) = references.execution() {
        inspections.push(WorkloadInspectionRequirement::Execution {
            reference: reference.clone(),
            expected_phase: current.phase(),
        });
    }
    if let Some(reference) = references.publication() {
        inspections.push(WorkloadInspectionRequirement::Publication {
            reference: reference.clone(),
            expected_phase: current.phase(),
        });
    }
    let detail = WorkloadPhaseDetail::cleanup_pending(
        current.active_intent(),
        current.phase(),
        references,
        inspections,
    )
    .expect("fixture cleanup detail is valid");
    let cleanup = current
        .advance(
            WorkloadSagaPhase::CleanupPending,
            detail,
            Some(
                WorkloadFailureEvidence::new("provider_timeout", evidence("provider-timeout"))
                    .expect("fixture failure evidence is valid"),
            ),
        )
        .expect("fixture cleanup transition is valid");
    history.push(cleanup);
    history
}

fn stopped_record(label: &str) -> WorkloadSagaRecord {
    let key = workload_key(label);
    let intent = workload_intent(
        &key,
        1,
        DesiredWorkloadState::Stopped,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    );
    WorkloadSagaRecord::new(key, intent).expect("stopped fixture should initialize")
}

fn workload_key(label: &str) -> nimbus_workloads::WorkloadSagaKey {
    nimbus_workloads::WorkloadSagaKey::new(
        TenantId::new(format!("tenant-{label}")).expect("fixture tenant is valid"),
        WorkloadId::new(format!("workload-{label}")).expect("fixture workload is valid"),
    )
}

fn workload_intent(
    key: &nimbus_workloads::WorkloadSagaKey,
    generation: u64,
    desired_state: DesiredWorkloadState,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
) -> WorkloadSagaIntent {
    WorkloadSagaIntent::new(
        DesiredWorkloadKind::Sandbox,
        desired_state,
        WorkloadGeneration::new(generation),
        WorkloadDesiredDigest::sha256(format!(
            "{}-{generation}-{desired_state:?}",
            key.workload_id().as_str()
        )),
        WorkloadNetworkIntent::new(compiled_network_plan(
            key.tenant_id(),
            key.workload_id().as_str(),
            generation,
            activation,
            publication,
        )),
        activation,
        publication,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "1".repeat(64))
                .try_into()
                .expect("fixture decision id is valid"),
            format!("twu_{}", "2".repeat(64))
                .try_into()
                .expect("fixture workload uid is valid"),
            Some(NodeIdentity::new("node-recovery").expect("fixture node is valid")),
        ),
    )
    .expect("fixture intent is valid")
}

fn evidence(label: &str) -> WorkloadOwnerEvidenceDigest {
    WorkloadOwnerEvidenceDigest::sha256(label)
}

fn publication_endpoint_id(label: &str) -> PublishedEndpointId {
    let digest = Sha256::digest(label.as_bytes());
    let mut identity = [0_u8; 16];
    identity.copy_from_slice(&digest[..16]);
    PublishedEndpointId::try_from(format!(
        "{}_{}",
        PublishedEndpointId::PREFIX,
        Ulid::from(u128::from_be_bytes(identity))
    ))
    .expect("derived fixture endpoint identity is valid")
}

fn history_through_phase(
    mut history: Vec<WorkloadSagaRecord>,
    phase: WorkloadSagaPhase,
) -> Vec<WorkloadSagaRecord> {
    let index = history
        .iter()
        .position(|record| record.phase() == phase)
        .expect("fixture history contains requested phase");
    history.truncate(index + 1);
    history
}

pub(super) fn process_matrix_histories() -> Vec<Vec<WorkloadSagaRecord>> {
    let mut histories = Vec::with_capacity(PROCESS_MATRIX_EXPECTATIONS.len());
    for (label, phase) in [
        ("process-intent", WorkloadSagaPhase::IntentCommitted),
        (
            "process-network-reserved",
            WorkloadSagaPhase::NetworkReserved,
        ),
        (
            "process-workload-prepared",
            WorkloadSagaPhase::WorkloadPrepared,
        ),
        (
            "process-network-attached",
            WorkloadSagaPhase::NetworkAttached,
        ),
        (
            "process-workload-activated",
            WorkloadSagaPhase::WorkloadActivated,
        ),
        ("process-ready", WorkloadSagaPhase::Ready),
        ("process-published", WorkloadSagaPhase::Published),
        ("process-observed", WorkloadSagaPhase::Observed),
    ] {
        histories.push(history_through_phase(
            provision_history(
                label,
                WorkloadActivationIntent::ActivateWhenAttached,
                WorkloadPublicationIntent::PublishWhenReady,
            ),
            phase,
        ));
    }

    for (label, phase) in [
        ("process-withdrawal", WorkloadSagaPhase::WithdrawalCommitted),
        ("process-withdrawn", WorkloadSagaPhase::Withdrawn),
        ("process-drained", WorkloadSagaPhase::Drained),
        (
            "process-workload-stopped",
            WorkloadSagaPhase::WorkloadStopped,
        ),
        (
            "process-network-detached",
            WorkloadSagaPhase::NetworkDetached,
        ),
        (
            "process-network-released",
            WorkloadSagaPhase::NetworkReleased,
        ),
        (
            "process-recorded-stopped-successor",
            WorkloadSagaPhase::Recorded,
        ),
    ] {
        histories.push(history_through_phase(teardown_history(label), phase));
    }

    histories.push(cleanup_pending_history("process-cleanup-complete"));
    histories.push(provision_history(
        "process-attached-prepare-only",
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    ));
    histories.push(history_through_phase(
        provision_history(
            "process-ready-withheld",
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        ),
        WorkloadSagaPhase::Ready,
    ));
    histories.push(vec![stopped_record("process-recorded-quiescent")]);
    histories.push(teardown_history_with_successor(
        "process-recorded-running-successor",
        DesiredWorkloadState::Running,
    ));
    histories.push(cleanup_pending_at_phase(
        "process-cleanup-network",
        WorkloadSagaPhase::NetworkReserved,
    ));
    histories.push(cleanup_pending_at_phase(
        "process-cleanup-execution",
        WorkloadSagaPhase::WorkloadPrepared,
    ));

    for (label, phase) in [
        (
            "process-successor-from-intent",
            WorkloadSagaPhase::IntentCommitted,
        ),
        (
            "process-successor-from-network-reserved",
            WorkloadSagaPhase::NetworkReserved,
        ),
        (
            "process-successor-from-workload-prepared",
            WorkloadSagaPhase::WorkloadPrepared,
        ),
        (
            "process-successor-from-network-attached",
            WorkloadSagaPhase::NetworkAttached,
        ),
        (
            "process-successor-from-workload-activated",
            WorkloadSagaPhase::WorkloadActivated,
        ),
        ("process-successor-from-ready", WorkloadSagaPhase::Ready),
        (
            "process-successor-from-published",
            WorkloadSagaPhase::Published,
        ),
        (
            "process-successor-from-observed",
            WorkloadSagaPhase::Observed,
        ),
    ] {
        histories.push(successor_withdrawal_history(label, phase));
    }

    assert_eq!(histories.len(), PROCESS_MATRIX_EXPECTATIONS.len());
    histories
}

pub(super) fn process_matrix_key(label: &str) -> nimbus_workloads::WorkloadSagaKey {
    workload_key(label)
}

fn cleanup_pending_at_phase(
    label: &str,
    last_safe_phase: WorkloadSagaPhase,
) -> Vec<WorkloadSagaRecord> {
    let mut history = history_through_phase(
        provision_history(
            label,
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::PublishWhenReady,
        ),
        last_safe_phase,
    );
    let current = latest(&history);
    let references = current.phase_detail().references();
    let inspections = [
        references
            .network()
            .map(|reference| WorkloadInspectionRequirement::Network {
                reference: reference.clone(),
                expected_phase: last_safe_phase,
            }),
        references
            .execution()
            .map(|reference| WorkloadInspectionRequirement::Execution {
                reference: reference.clone(),
                expected_phase: last_safe_phase,
            }),
        references
            .publication()
            .map(|reference| WorkloadInspectionRequirement::Publication {
                reference: reference.clone(),
                expected_phase: last_safe_phase,
            }),
    ]
    .into_iter()
    .flatten()
    .collect();
    let detail = WorkloadPhaseDetail::cleanup_pending(
        current.active_intent(),
        last_safe_phase,
        references,
        inspections,
    )
    .expect("process cleanup detail is valid");
    history.push(
        current
            .advance(
                WorkloadSagaPhase::CleanupPending,
                detail,
                Some(
                    WorkloadFailureEvidence::new(
                        "process_unknown_effect",
                        evidence("process-unknown-effect"),
                    )
                    .expect("process failure evidence is valid"),
                ),
            )
            .expect("process cleanup transition is valid"),
    );
    history
}

fn successor_withdrawal_history(label: &str, origin: WorkloadSagaPhase) -> Vec<WorkloadSagaRecord> {
    let mut history = history_through_phase(
        provision_history(
            label,
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::PublishWhenReady,
        ),
        origin,
    );
    let current = latest(&history);
    let successor = workload_intent(
        current.key(),
        2,
        DesiredWorkloadState::Running,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    let WorkloadSagaIntentUpdate::Transition(withdrawal) = current
        .apply_intent(successor)
        .expect("higher generation must begin active-generation withdrawal")
    else {
        panic!("higher generation must produce a withdrawal transition");
    };
    assert_eq!(withdrawal.phase(), WorkloadSagaPhase::WithdrawalCommitted);
    history.push(*withdrawal);
    history
}

fn latest(history: &[WorkloadSagaRecord]) -> &WorkloadSagaRecord {
    history.last().expect("fixture history is not empty")
}
