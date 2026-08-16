use std::collections::BTreeSet;

use nimbus_core::{TenantId, WorkloadId};
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, ProposedWorkloadTeardownTransition,
    WORKLOAD_SAGA_RECOVERY_ORDER, WorkloadActivationIntent, WorkloadAdmissionEvidence,
    WorkloadFailureEvidence, WorkloadGeneration, WorkloadInspectionRequirement,
    WorkloadNetworkIntent, WorkloadOwnerEvidenceDigest, WorkloadPhaseDetail,
    WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaIntent,
    WorkloadSagaIntentUpdate, WorkloadSagaPageRequest, WorkloadSagaPhase, WorkloadSagaRecord,
    WorkloadSagaRecoveryCursor, WorkloadSagaStore, WorkloadTeardownClaim, WorkloadTeardownDecision,
    WorkloadTeardownDisposition, WorkloadTeardownEffectResult, WorkloadTeardownStep,
    WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};

use super::super::EngineWorkloadSagaStore;
use super::{compiled_network_plan, engine, provision_fixture, provision_source};

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
        WorkloadSagaPhase::NetworkAttached,
        "inspect-activation-prerequisites",
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
        "quiescent",
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
        "quiescent",
    ),
    process_case(
        "process-cleanup-execution",
        WorkloadSagaPhase::CleanupPending,
        WorkloadSagaPhase::CleanupPending,
        "quiescent",
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
        "advance-without-effect",
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
    assert_eq!(first_page_sizes, vec![4, 4, 4, 4]);
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
    assert_eq!(reopened_page_sizes, vec![4, 4, 4, 4]);
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

    let mut expected_revision = records[0].revision();
    for candidate in provision_fixture::provision_candidates(&records[0]) {
        assert_eq!(
            store
                .compare_and_swap(
                    WorkloadSagaExpected::Revision(expected_revision),
                    candidate.clone(),
                )
                .await,
            Ok(WorkloadSagaCommit::Applied)
        );
        expected_revision = candidate.revision();
    }

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
    recoverable.push(history_through_phase(
        provision_history(
            "recover-owner-reopened-publication",
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::PublishWhenReady,
        ),
        WorkloadSagaPhase::Observed,
    ));

    let quiescent = vec![
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

pub(super) fn provision_history(
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
    let mut history =
        vec![WorkloadSagaRecord::new(key, intent).expect("fixture saga record should initialize")];
    let target = if activation == WorkloadActivationIntent::PrepareOnly {
        WorkloadSagaPhase::NetworkAttached
    } else {
        WorkloadSagaPhase::Observed
    };
    while latest(&history).phase() != target {
        provision_fixture::extend_confirmed_step(&mut history);
    }
    history
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

    while latest(&history).phase() != WorkloadSagaPhase::Recorded {
        extend_confirmed_teardown_step(&mut history);
    }
    history
}

pub(super) fn extend_confirmed_teardown_step(history: &mut Vec<WorkloadSagaRecord>) {
    let current = latest(history);
    match current
        .decide_teardown()
        .expect("fixture teardown decision should validate")
    {
        WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::ResourceFree { step, .. },
        ) => {
            history.push(
                current
                    .record_resource_free_teardown_step(step)
                    .expect("exact resource-free teardown step should persist"),
            );
        }
        WorkloadTeardownDecision::PersistCandidate(ProposedWorkloadTeardownTransition::Claim {
            attempt,
            provider_target,
        }) => {
            let claimed = current
                .claim_teardown(*attempt, provider_target)
                .expect("exact teardown claim should persist");
            let claim = claimed
                .teardown_disposition()
                .and_then(WorkloadTeardownDisposition::claim)
                .expect("claimed teardown record should retain its exact claim")
                .clone();
            let succeeded = claimed
                .apply_teardown_effect_result(&claim, teardown_success_result(&claim))
                .expect("exact teardown success should advance one phase");
            history.push(claimed);
            history.push(succeeded);
        }
        WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::RecordTerminal,
        ) => {
            history.push(
                current
                    .record_terminal_teardown()
                    .expect("exact terminal teardown record should persist"),
            );
        }
        decision => {
            panic!("fixture teardown must have an exact persistence decision: {decision:?}")
        }
    }
}

fn teardown_success_result(claim: &WorkloadTeardownClaim) -> WorkloadTeardownEffectResult {
    WorkloadTeardownEffectResult::Succeeded {
        attempt_id: claim.attempt().attempt_id().clone(),
        dispatch_epoch: claim.dispatch_epoch(),
        provider_target: claim.provider_target().clone(),
        evidence: Box::new(teardown_success_evidence(claim)),
    }
}

fn teardown_success_evidence(claim: &WorkloadTeardownClaim) -> WorkloadTeardownSuccessEvidence {
    match (claim.attempt().step(), claim.attempt().subjects()) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(reference),
        ) => WorkloadTeardownSuccessEvidence::PublicationAbsent {
            reference: reference.clone(),
            evidence: evidence("publication-absent"),
        },
        (WorkloadTeardownStep::DrainExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: reference.clone(),
                evidence: evidence("execution-drained"),
            }
        }
        (WorkloadTeardownStep::StopExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: reference.clone(),
                evidence: evidence("execution-stopped"),
            }
        }
        (WorkloadTeardownStep::DetachNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkDetached {
                reference: reference.clone(),
                evidence: evidence("network-detached"),
            }
        }
        (WorkloadTeardownStep::ReleaseNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkReleased {
                reference: reference.clone(),
                evidence: evidence("network-released"),
            }
        }
        _ => panic!("validated teardown claim has a crossed step and subject"),
    }
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
    let executable = nimbus_workloads::WorkloadExecutableIntent::new(
        nimbus_workloads::WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        format!(
            r#"{{"fixture":"{}-{generation}-{desired_state:?}"}}"#,
            key.workload_id().as_str()
        ),
    )
    .expect("fixture executable is valid");
    let source = provision_source(
        &executable,
        key.workload_id().as_str(),
        generation,
        nimbus_network::NetworkProviderId::for_registration_key("fixture-attachment"),
    );
    WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        desired_state,
        WorkloadGeneration::new(generation),
        executable,
        source,
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
            NodeIdentity::new("node-recovery").expect("fixture node is valid"),
        ),
    )
    .expect("fixture intent is valid")
}

fn evidence(label: &str) -> WorkloadOwnerEvidenceDigest {
    WorkloadOwnerEvidenceDigest::sha256(label)
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
