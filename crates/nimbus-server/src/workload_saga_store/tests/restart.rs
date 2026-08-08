use std::sync::Arc;

use nimbus_workloads::{
    WorkloadEffectReferences, WorkloadExecutionReference, WorkloadNetworkReference,
    WorkloadOwnerEvidenceDigest, WorkloadOwnerObservation, WorkloadPhaseDetail,
    WorkloadRestartAdmissionInput, WorkloadRestartAdmissionUpdate, WorkloadRestartCommandClaim,
    WorkloadRestartEffectResult, WorkloadRestartEpoch, WorkloadRestartEvidenceDigest,
    WorkloadRestartNotBeforeUnixMillis, WorkloadRestartPolicy, WorkloadRestartRequestId,
    WorkloadRestartTrigger, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaPageRequest,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore,
};
use serde_json::{Value, json};

use super::super::EngineWorkloadSagaStore;
use super::super::codec::{decode_workload_saga_record, encode_workload_saga_record};
use super::{document_for, engine, initial_record, provision_fixture};

pub(super) fn observed_record(label: &str, policy: WorkloadRestartPolicy) -> WorkloadSagaRecord {
    observed_history(label, policy).pop().unwrap()
}

pub(super) fn observed_history(
    label: &str,
    policy: WorkloadRestartPolicy,
) -> Vec<WorkloadSagaRecord> {
    let initial = initial_record(label);
    let intent = initial.active_intent();
    let intent = nimbus_workloads::WorkloadSagaIntent::new_with_restart_policy(
        intent.kind(),
        intent.desired_state(),
        intent.generation(),
        intent.executable().clone(),
        intent.source().clone(),
        policy,
        intent.network().clone(),
        intent.activation(),
        intent.publication(),
        intent.admission().clone(),
    )
    .expect("restart fixture intent should validate");
    let mut history = vec![
        WorkloadSagaRecord::new(initial.key().clone(), intent)
            .expect("restart fixture should initialize"),
    ];
    while history.last().unwrap().phase() != WorkloadSagaPhase::Observed {
        provision_fixture::extend_confirmed_step(&mut history);
    }
    history
}

pub(super) async fn persist_history(
    store: &EngineWorkloadSagaStore,
    history: &[WorkloadSagaRecord],
) {
    for (index, record) in history.iter().enumerate() {
        let expected = if index == 0 {
            WorkloadSagaExpected::Missing
        } else {
            WorkloadSagaExpected::Revision(history[index - 1].revision())
        };
        assert_eq!(
            store.compare_and_swap(expected, record.clone()).await,
            Ok(WorkloadSagaCommit::Applied)
        );
    }
}

pub(super) fn explicit_input(
    record: &WorkloadSagaRecord,
    key: &str,
    not_before: u64,
) -> WorkloadRestartAdmissionInput {
    WorkloadRestartAdmissionInput {
        expected_revision: record.revision(),
        trigger: WorkloadRestartTrigger::Explicit,
        inspection_version: None,
        request_id: WorkloadRestartRequestId::for_explicit(
            record.saga_id(),
            record.active_intent().source().source_generation(),
            key,
        )
        .expect("explicit request ID should validate"),
        not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis::new(not_before),
    }
}

fn automatic_input(record: &WorkloadSagaRecord, not_before: u64) -> WorkloadRestartAdmissionInput {
    let inspection = nimbus_workloads::WorkloadInspectionVersion::from_bytes([0x51; 32]);
    WorkloadRestartAdmissionInput {
        expected_revision: record.revision(),
        trigger: WorkloadRestartTrigger::Automatic { exit_code: 7 },
        inspection_version: Some(inspection),
        request_id: WorkloadRestartRequestId::for_automatic(record.saga_id(), inspection),
        not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis::new(not_before),
    }
}

pub(super) fn admit(
    record: &WorkloadSagaRecord,
    input: WorkloadRestartAdmissionInput,
) -> WorkloadSagaRecord {
    let WorkloadRestartAdmissionUpdate::Transition(candidate) =
        record.admit_restart(input).expect("restart should admit")
    else {
        panic!("new restart must create a transition");
    };
    *candidate
}

fn active_claim(record: &WorkloadSagaRecord) -> WorkloadRestartCommandClaim {
    record
        .restart_state()
        .active()
        .expect("restart should be active")
        .disposition()
        .claim()
        .expect("restart command should be claimed")
        .clone()
}

fn extend_successful_command(history: &mut Vec<WorkloadSagaRecord>, label: &str) {
    let current = history.last().unwrap();
    let request_id = current
        .restart_state()
        .active()
        .unwrap()
        .admission()
        .request_id();
    let claimed = current
        .claim_restart_command(request_id)
        .expect("restart command should claim");
    let claim = active_claim(&claimed);
    history.push(claimed.clone());
    history.push(
        claimed
            .apply_restart_effect_result(
                &claim,
                WorkloadRestartEffectResult::Succeeded {
                    evidence: WorkloadRestartEvidenceDigest::sha256(label),
                },
                None,
            )
            .expect("restart command should succeed"),
    );
}

fn extend_restart_to_completion(history: &mut Vec<WorkloadSagaRecord>) {
    let mut record = history.last().unwrap().clone();
    let request_id = record
        .restart_state()
        .active()
        .expect("restart should be active")
        .admission()
        .request_id()
        .clone();
    record = record
        .advance_restart_without_effect(&request_id)
        .expect("requested restart should enter withdrawal");
    history.push(record);
    extend_successful_command(history, "restart-withdrawn");
    extend_successful_command(history, "restart-quiesced");
    record = history.last().unwrap().clone();
    let due = record
        .restart_state()
        .active()
        .unwrap()
        .admission()
        .not_before_unix_millis();
    record = record
        .advance_scheduled_restart(&request_id, due)
        .expect("due restart should advance");
    history.push(record.clone());
    for label in [
        "restart-prepared",
        "restart-attached",
        "restart-prerequisites",
        "restart-activated",
        "restart-ready",
    ] {
        extend_successful_command(history, label);
    }
    record = history
        .last()
        .unwrap()
        .advance_restart_without_effect(&request_id)
        .expect("withheld publication should advance without an ingress effect");
    history.push(record.clone());

    let claimed = record
        .claim_restart_command(&request_id)
        .expect("observation command should claim");
    let claim = active_claim(&claimed);
    history.push(claimed.clone());

    let intent = claimed.active_intent();
    let restart_epoch = claimed
        .restart_state()
        .active()
        .unwrap()
        .admission()
        .restart_epoch();
    let execution = WorkloadExecutionReference::for_restart_epoch(intent, restart_epoch);
    let network = WorkloadNetworkReference::for_intent(intent);
    let references =
        WorkloadEffectReferences::new(Some(network.clone()), Some(execution.clone()), None);
    let observed_detail = WorkloadPhaseDetail::provision(
        WorkloadSagaPhase::Observed,
        intent,
        references,
        vec![
            WorkloadOwnerObservation::NetworkReserved {
                reference: network.clone(),
                evidence: WorkloadOwnerEvidenceDigest::sha256("restart-network-reserved"),
            },
            WorkloadOwnerObservation::ExecutionPrepared {
                reference: execution.clone(),
                evidence: WorkloadOwnerEvidenceDigest::sha256("restart-execution-prepared"),
            },
            WorkloadOwnerObservation::NetworkAttached {
                reference: network.clone(),
                evidence: WorkloadOwnerEvidenceDigest::sha256("restart-network-attached"),
            },
            WorkloadOwnerObservation::ExecutionActivated {
                reference: execution.clone(),
                evidence: WorkloadOwnerEvidenceDigest::sha256("restart-execution-activated"),
            },
            WorkloadOwnerObservation::Ready {
                network,
                execution,
                evidence: WorkloadOwnerEvidenceDigest::sha256("restart-ready"),
            },
        ],
    )
    .expect("new-attempt observed detail should validate");
    history.push(
        claimed
            .apply_restart_effect_result(
                &claim,
                WorkloadRestartEffectResult::Succeeded {
                    evidence: WorkloadRestartEvidenceDigest::sha256("restart-observed"),
                },
                Some(observed_detail),
            )
            .expect("restart should complete"),
    );
}

#[test]
fn restart_record_strict_codec_round_trip() {
    let record = observed_record("restart-codec", WorkloadRestartPolicy::Never);
    let candidate = admit(&record, explicit_input(&record, "codec", u64::MAX));
    let fields = encode_workload_saga_record(&candidate).expect("restart record should encode");
    assert_eq!(fields.len(), 23);
    assert_eq!(
        fields.get("restartWatchCandidate"),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        fields.get("restartPolicy"),
        Some(&serde_json::to_value(candidate.active_intent().restart_policy()).unwrap())
    );
    assert_eq!(
        fields.get("restartState"),
        Some(&serde_json::to_value(candidate.restart_state()).unwrap())
    );
    assert_eq!(
        decode_workload_saga_record(&document_for(&candidate)),
        Ok(candidate)
    );
}

#[test]
fn restart_codec_rejects_missing_unknown_null_and_duplicate_fields() {
    let record = observed_record("restart-codec-shape", WorkloadRestartPolicy::Never);
    let candidate = admit(&record, explicit_input(&record, "shape", 10));
    for mutation in ["missing", "unknown", "null"] {
        let mut document = document_for(&candidate);
        match mutation {
            "missing" => {
                document.fields.remove("restartState");
            }
            "unknown" => {
                document
                    .fields
                    .insert("restartUnknown".to_owned(), json!(true));
            }
            "null" => {
                document
                    .fields
                    .insert("restartState".to_owned(), Value::Null);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            decode_workload_saga_record(&document),
            Err(nimbus_workloads::WorkloadSagaStoreError::Corrupt),
            "{mutation} restart field must fail closed"
        );
    }

    let portable = serde_json::to_string(&candidate).unwrap();
    let duplicate = portable.replacen("\"restart\":", "\"restart\":null,\"restart\":", 1);
    assert!(serde_json::from_str::<WorkloadSagaRecord>(&duplicate).is_err());
}

#[test]
fn restart_codec_rejects_crossed_identity_digest_epoch_and_attempt() {
    let record = observed_record("restart-codec-crossed", WorkloadRestartPolicy::Never);
    let candidate = admit(&record, explicit_input(&record, "crossed", 10));
    for pointer in [
        "/active/admission/requestId",
        "/active/admission/desiredDigest",
        "/active/admission/restartEpoch",
        "/active/admission/attemptId",
    ] {
        let mut document = document_for(&candidate);
        let restart = document.fields.get_mut("restartState").unwrap();
        if let Some(value) = restart.pointer_mut(pointer) {
            *value = json!(if pointer.ends_with("restartEpoch") {
                "9".to_owned()
            } else {
                "00".repeat(32)
            });
        } else {
            panic!("fixture pointer {pointer} should exist");
        }
        assert_eq!(
            decode_workload_saga_record(&document),
            Err(nimbus_workloads::WorkloadSagaStoreError::Corrupt),
            "crossed {pointer} must fail closed"
        );
    }
}

#[tokio::test]
async fn restart_store_round_trip_preserves_deadline_count_and_active_request() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let history = observed_history(
        "restart-store-round-trip",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let record = history.last().unwrap().clone();
    let candidate = admit(&record, automatic_input(&record, u64::MAX));
    persist_history(&store, &history).await;
    assert_eq!(
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(record.revision()),
                candidate.clone(),
            )
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );
    let reopened = store.load(record.key()).await.unwrap().unwrap();
    assert_eq!(reopened, candidate);
    assert_eq!(
        reopened.restart_state().completed_automatic_restart_count(),
        1
    );
    assert_eq!(
        reopened
            .restart_state()
            .active()
            .unwrap()
            .admission()
            .not_before_unix_millis()
            .as_u64(),
        u64::MAX
    );
}

#[tokio::test]
async fn restart_store_round_trip_preserves_complete_admission_history() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let mut history = observed_history(
        "restart-store-history",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let observed = history.last().unwrap().clone();
    let admitted = admit(&observed, automatic_input(&observed, 125));
    let expected_admission = admitted
        .restart_state()
        .active()
        .unwrap()
        .admission()
        .clone();
    history.push(admitted);
    extend_restart_to_completion(&mut history);
    persist_history(&store, &history).await;

    let reopened = store.load(observed.key()).await.unwrap().unwrap();
    let completed = reopened
        .restart_state()
        .last_completed()
        .expect("reopened record should retain completed restart history");
    assert_eq!(completed.admission(), &expected_admission);
    assert_eq!(
        completed.evidence(),
        WorkloadRestartEvidenceDigest::sha256("restart-observed")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_store_cas_contention_admits_one_epoch() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let left = Arc::new(EngineWorkloadSagaStore::new(Arc::clone(&engine)));
    let right = Arc::new(EngineWorkloadSagaStore::new(engine));
    let history = observed_history("restart-contention", WorkloadRestartPolicy::Never);
    let current = history.last().unwrap().clone();
    persist_history(&left, &history).await;
    let first = admit(&current, explicit_input(&current, "left", 0));
    let second = admit(&current, explicit_input(&current, "right", 0));
    let expected = WorkloadSagaExpected::Revision(current.revision());
    let (first_result, second_result) = tokio::join!(
        left.compare_and_swap(expected, first),
        right.compare_and_swap(expected, second)
    );
    assert_eq!(
        [first_result.clone(), second_result.clone()]
            .iter()
            .filter(|result| **result == Ok(WorkloadSagaCommit::Applied))
            .count(),
        1
    );
    assert_eq!(
        [first_result, second_result]
            .iter()
            .filter(|result| matches!(
                result,
                Err(nimbus_workloads::WorkloadSagaStoreError::Conflict { .. })
            ))
            .count(),
        1
    );
    assert_eq!(
        left.load(current.key())
            .await
            .unwrap()
            .unwrap()
            .restart_state()
            .active()
            .unwrap()
            .admission()
            .restart_epoch(),
        WorkloadRestartEpoch::new(1)
    );
}

#[tokio::test]
async fn deadline_survives_engine_reopen() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let history = observed_history("restart-deadline-reopen", WorkloadRestartPolicy::Never);
    let current = history.last().unwrap().clone();
    let candidate = admit(&current, explicit_input(&current, "deadline", 500));
    {
        let store = EngineWorkloadSagaStore::new(engine(&root));
        persist_history(&store, &history).await;
        assert_eq!(
            store
                .compare_and_swap(
                    WorkloadSagaExpected::Revision(current.revision()),
                    candidate.clone(),
                )
                .await,
            Ok(WorkloadSagaCommit::Applied)
        );
    }
    let reopened_store = EngineWorkloadSagaStore::new(engine(&root));
    let reopened = reopened_store.load(current.key()).await.unwrap().unwrap();
    assert_eq!(
        reopened
            .restart_state()
            .active()
            .unwrap()
            .admission()
            .not_before_unix_millis(),
        WorkloadRestartNotBeforeUnixMillis::new(500)
    );
}

#[tokio::test]
async fn count_survives_engine_reopen() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let history = observed_history(
        "restart-count-reopen",
        WorkloadRestartPolicy::Always { max_restarts: 2 },
    );
    let current = history.last().unwrap().clone();
    let candidate = admit(&current, automatic_input(&current, 0));
    {
        let store = EngineWorkloadSagaStore::new(engine(&root));
        persist_history(&store, &history).await;
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(current.revision()),
                candidate,
            )
            .await
            .unwrap();
    }
    let reopened = EngineWorkloadSagaStore::new(engine(&root))
        .load(current.key())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reopened.restart_state().completed_automatic_restart_count(),
        1
    );
}

#[tokio::test]
async fn restart_recovery_query_is_bounded_stable_and_complete() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let mut expected = Vec::new();
    for label in ["restart-page-a", "restart-page-b", "restart-page-c"] {
        let history = observed_history(label, WorkloadRestartPolicy::Never);
        let current = history.last().unwrap().clone();
        let candidate = admit(&current, explicit_input(&current, label, 1_000));
        persist_history(&store, &history).await;
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(current.revision()),
                candidate.clone(),
            )
            .await
            .unwrap();
        expected.push(candidate);
    }
    expected.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
    let first_request = WorkloadSagaPageRequest::new(None, 2).unwrap();
    let first = store.list_recoverable(first_request).await.unwrap();
    assert_eq!(first.records(), &expected[..2]);
    let second_request = WorkloadSagaPageRequest::new(first.next_cursor().cloned(), 2).unwrap();
    let second = store.list_recoverable(second_request).await.unwrap();
    assert_eq!(second.records(), &expected[2..]);
    assert!(second.next_cursor().is_none());
}
