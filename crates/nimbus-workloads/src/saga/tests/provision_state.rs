use super::*;

#[test]
fn running_provision_record_requires_provision_disposition() {
    let record = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .expect("running record should validate");
    assert_eq!(
        record.provision_disposition(),
        Some(&WorkloadProvisionDisposition::Ready)
    );

    let mut encoded = serde_json::to_value(&record).expect("record should encode");
    encoded
        .as_object_mut()
        .expect("record is an object")
        .remove("provisionDisposition");
    assert!(serde_json::from_value::<WorkloadSagaRecord>(encoded).is_err());
}

#[test]
fn non_provision_record_has_no_provision_disposition() {
    let record = WorkloadSagaRecord::new(key("tenant-a", "workload-a"), stopped_intent(1))
        .expect("stopped record should validate");
    assert!(record.provision_disposition().is_none());
}

#[test]
fn effectful_provision_phase_cannot_bypass_persisted_attempt_protocol() {
    let record = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .expect("running record should validate");
    let detail = provision_detail(
        WorkloadSagaPhase::NetworkReserved,
        record.active_intent(),
        None,
    );

    assert_eq!(
        record
            .advance(WorkloadSagaPhase::NetworkReserved, detail.clone(), None,)
            .unwrap_err(),
        WorkloadSagaError::InvalidTransition(
            "effectful provision advance requires the exact persisted attempt protocol"
        )
    );
    assert_eq!(
        record
            .dispatch_to_success(WorkloadSagaPhase::NetworkReserved, detail.clone(),)
            .unwrap_err(),
        WorkloadSagaError::InvalidTransition("dispatch success requires an exact unresolved claim")
    );

    let confirmed = super::test_support::confirmed_provision(
        &record,
        WorkloadSagaPhase::NetworkReserved,
        detail,
    );
    assert_eq!(confirmed.phase(), WorkloadSagaPhase::NetworkReserved);
    assert_eq!(
        confirmed.provision_disposition(),
        Some(&WorkloadProvisionDisposition::Ready)
    );
}

#[test]
fn activation_prerequisite_attempt_cannot_complete_activation() {
    let mut attached = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .expect("running record should validate");
    for phase in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
    ] {
        attached = advance_provision(&attached, phase, None);
    }
    let network = WorkloadNetworkReference::for_intent(attached.active_intent());
    let execution = WorkloadExecutionReference::for_intent(attached.active_intent());
    let inspection = provision_attempt_fixture(
        &attached,
        WorkloadProvisionStep::InspectActivationPrerequisites,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadProvisionSubjects::Readiness { network, execution },
        None,
    );
    let pending = persist_attempt_fixture(&attached, inspection);

    assert_eq!(
        pending
            .dispatch_to_success(
                WorkloadSagaPhase::WorkloadActivated,
                provision_detail(
                    WorkloadSagaPhase::WorkloadActivated,
                    pending.active_intent(),
                    None,
                ),
            )
            .unwrap_err(),
        WorkloadSagaError::InvalidTransition(
            "activation-prerequisite success requires a distinct activation dispatch"
        )
    );
}

#[test]
fn activation_attempt_requires_retained_prerequisite_inspection() {
    let mut attached = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .expect("running record should validate");
    for phase in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
    ] {
        attached = advance_provision(&attached, phase, None);
    }
    let network = WorkloadNetworkReference::for_intent(attached.active_intent());
    let execution = WorkloadExecutionReference::for_intent(attached.active_intent());
    let unpersisted_inspection = provision_attempt_fixture(
        &attached,
        WorkloadProvisionStep::InspectActivationPrerequisites,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadProvisionSubjects::Readiness {
            network: network.clone(),
            execution: execution.clone(),
        },
        None,
    );
    let prerequisite = WorkloadProvisionPrerequisiteEvidence::new(
        unpersisted_inspection.attempt_id().clone(),
        WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
            network,
            execution: execution.clone(),
            evidence: evidence("unpersisted-activation-prerequisite"),
        },
    )
    .expect("unpersisted prerequisite remains structurally valid");
    let activation = provision_attempt_fixture(
        &attached,
        WorkloadProvisionStep::ActivateWorkload,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadProvisionSubjects::Execution(execution),
        Some(prerequisite),
    );

    let provider_target = provider_target_fixture(&activation);
    assert_eq!(
        attached
            .ready_to_initial_dispatch(activation, provider_target)
            .unwrap_err(),
        WorkloadSagaError::InvalidTransition("provision disposition transition is not legal")
    );
}

#[test]
fn activation_prerequisite_subjects_must_match_retained_inspection() {
    let mut attached = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .expect("running record should validate");
    for phase in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
    ] {
        attached = advance_provision(&attached, phase, None);
    }
    let network = WorkloadNetworkReference::for_intent(attached.active_intent());
    let execution = WorkloadExecutionReference::for_intent(attached.active_intent());
    let inspection = provision_attempt_fixture(
        &attached,
        WorkloadProvisionStep::InspectActivationPrerequisites,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadProvisionSubjects::Readiness {
            network: network.clone(),
            execution: execution.clone(),
        },
        None,
    );
    let pending = persist_attempt_fixture(&attached, inspection.clone());
    let crossed = running_intent(2, WorkloadPublicationIntent::Withheld);
    let prerequisite = WorkloadProvisionPrerequisiteEvidence::new(
        inspection.attempt_id().clone(),
        WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
            network: WorkloadNetworkReference::for_intent(&crossed),
            execution: WorkloadExecutionReference::for_intent(&crossed),
            evidence: evidence("crossed-activation-prerequisite"),
        },
    )
    .expect("crossed fixture remains structurally typed");
    let activation = provision_attempt_fixture(
        &pending,
        WorkloadProvisionStep::ActivateWorkload,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadProvisionSubjects::Execution(execution),
        Some(prerequisite),
    );

    let provider_target = provider_target_fixture(&activation);
    assert_eq!(
        pending
            .dispatch_to_activation(activation, provider_target)
            .unwrap_err(),
        WorkloadSagaError::InvalidEvidence(
            "activation prerequisite is crossed with durable lifecycle references"
        )
    );
}

#[test]
fn promoted_generation_requires_exact_initial_provision_disposition() {
    let active = record_at_ready(WorkloadPublicationIntent::Withheld);
    let WorkloadSagaIntentUpdate::Transition(queued) = active
        .apply_intent(running_intent(2, WorkloadPublicationIntent::Withheld))
        .expect("higher generation should queue")
    else {
        panic!("higher generation should produce a transition");
    };
    let recorded = finish_teardown(&queued);
    let promoted = recorded
        .promote_successor()
        .expect("canonical promotion should validate");
    let mut encoded = serde_json::to_value(&promoted).expect("promotion should encode");
    encoded["provisionDisposition"] = json!(null);
    rehash_encoded_record(&mut encoded);

    assert!(
        serde_json::from_value::<WorkloadSagaRecord>(encoded).is_err(),
        "promotion must first persist exact initial readiness before issuing an attempt"
    );
}

#[test]
fn workload_kind_must_match_provision_source_variant() {
    for (intent, crossed_kind) in [
        (
            intent_with(
                "tenant-a",
                "workload-a",
                1,
                DesiredWorkloadState::Running,
                WorkloadActivationIntent::ActivateWhenAttached,
                WorkloadPublicationIntent::Withheld,
                1,
            ),
            DesiredWorkloadKind::Service,
        ),
        (
            intent_with(
                "tenant-a",
                "workload-a",
                2,
                DesiredWorkloadState::Running,
                WorkloadActivationIntent::ActivateWhenAttached,
                WorkloadPublicationIntent::Withheld,
                2,
            ),
            DesiredWorkloadKind::Sandbox,
        ),
    ] {
        assert!(
            WorkloadSagaIntent::new(
                crossed_kind,
                intent.desired_state(),
                intent.generation(),
                intent.executable().clone(),
                intent.source().clone(),
                intent.network().clone(),
                intent.activation(),
                intent.publication(),
                intent.admission().clone(),
            )
            .is_err(),
            "desired workload kind must match the closed provision source variant"
        );
    }
}

#[test]
fn observed_publish_when_ready_requires_publication_observation() {
    let ready = record_at_ready(WorkloadPublicationIntent::PublishWhenReady);
    let publication = ready
        .phase_detail()
        .references()
        .publication()
        .expect("ready fixture should retain publication")
        .clone();
    let published = advance_provision(&ready, WorkloadSagaPhase::Published, Some(&publication));
    let published_detail = published.phase_detail().clone();

    assert!(
        WorkloadPhaseDetail::provision(
            WorkloadSagaPhase::Observed,
            published.active_intent(),
            published_detail.references(),
            match published_detail {
                WorkloadPhaseDetail::Provision(detail) => detail.observations().to_vec(),
                _ => panic!("published fixture should carry provision detail"),
            },
        )
        .is_err(),
        "observed publication must add exact observation evidence"
    );
}

#[test]
fn provision_disposition_requires_exact_attempt_revision_history() {
    fn reject_offset(record: &WorkloadSagaRecord, revision: u64) {
        let mut encoded = serde_json::to_value(record).expect("record should encode");
        encoded["revision"] = json!(revision.to_string());
        encoded["lastTransition"]["resultingRevision"] = json!(revision.to_string());
        rehash_encoded_record(&mut encoded);
        let error = serde_json::from_value::<WorkloadSagaRecord>(encoded)
            .expect_err("nonexact attempt history must fail closed");
        assert!(error.to_string().contains(
            "provision dispatch claim revision does not exactly bind disposition history"
        ));
    }

    let initial = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .expect("running record should validate");
    let initial = advance_provision(&initial, WorkloadSagaPhase::NetworkReserved, None);
    let detail = provision_detail(
        WorkloadSagaPhase::WorkloadPrepared,
        initial.active_intent(),
        None,
    );
    let candidates = super::test_support::provision_candidates(
        &initial,
        WorkloadSagaPhase::WorkloadPrepared,
        detail,
    );
    let pending = candidates
        .first()
        .expect("confirmed edge should first retain a pending attempt");
    assert_eq!(pending.revision(), WorkloadSagaRevision::new(2));
    reject_offset(pending, 3);

    let claim = pending
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("pending record should retain its dispatch claim")
        .clone();
    let inspection = pending
        .dispatch_to_inspection()
        .expect("inspection history should validate");
    assert_eq!(inspection.revision(), WorkloadSagaRevision::new(3));
    reject_offset(&inspection, 2);
    reject_offset(&inspection, 4);

    let failure = WorkloadFailureEvidence::new("provider_failed", evidence("provider-failed"))
        .expect("failure evidence should validate");
    let direct_failure = pending
        .dispatch_to_definite_failure(failure.clone())
        .expect("direct failure history should validate");
    let inspected_failure = inspection
        .dispatch_to_definite_failure(failure)
        .expect("post-inspection failure history should validate");
    assert_eq!(direct_failure.revision(), WorkloadSagaRevision::new(3));
    assert_eq!(inspected_failure.revision(), WorkloadSagaRevision::new(4));
    assert_eq!(
        direct_failure
            .provision_disposition()
            .and_then(WorkloadProvisionDisposition::claim),
        Some(&claim)
    );
    reject_offset(&direct_failure, 2);
    reject_offset(&inspected_failure, 5);
}

#[test]
fn exact_absence_retries_same_attempt_at_one_higher_dispatch_epoch() {
    let initial = WorkloadSagaRecord::new(
        key("tenant-a", "workload-retry"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .expect("running record should validate");
    let reserved = advance_provision(&initial, WorkloadSagaPhase::NetworkReserved, None);
    let attempt = provision_attempt_fixture(
        &reserved,
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadProvisionSubjects::Execution(WorkloadExecutionReference::for_intent(
            reserved.active_intent(),
        )),
        None,
    );
    let pending = persist_attempt_fixture(&reserved, attempt);
    let first_claim = pending
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("pending record should retain a claim")
        .clone();
    let inspection = pending
        .dispatch_to_inspection()
        .expect("uncertain dispatch should require inspection");
    let absence = WorkloadProvisionAbsenceEvidence::for_inspection(
        &inspection,
        &first_claim,
        evidence("provider-confirmed-absence"),
    )
    .expect("absence should bind the exact inspected transition");
    let retry = inspection
        .inspection_to_retry_dispatch(absence)
        .expect("exact absence should authorize retry");
    let retry_claim = retry
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("retry record should retain a claim");

    assert_eq!(
        retry_claim.attempt().attempt_id(),
        first_claim.attempt().attempt_id()
    );
    assert_eq!(retry_claim.dispatch_epoch().as_u64(), 1);
    assert_eq!(retry_claim.claimed_revision(), retry.revision());

    let retry_inspection = retry
        .dispatch_to_inspection()
        .expect("retry dispatch must enter its own inspection state");
    let crossed_absence = WorkloadProvisionAbsenceEvidence::for_inspection(
        &retry_inspection,
        retry_claim,
        evidence("crossed-retry-absence"),
    )
    .expect("crossed absence fixture should bind its own transition");
    assert_eq!(
        inspection
            .inspection_to_retry_dispatch(crossed_absence)
            .unwrap_err(),
        WorkloadSagaError::InvalidEvidence(
            "retry absence evidence is crossed with the inspected dispatch claim"
        )
    );
}

#[test]
fn retry_from_dispatch_pending_is_rejected_without_revision_change() {
    let initial = WorkloadSagaRecord::new(
        key("tenant-a", "workload-pending-retry"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .expect("running record should validate");
    let reserved = advance_provision(&initial, WorkloadSagaPhase::NetworkReserved, None);
    let attempt = provision_attempt_fixture(
        &reserved,
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadProvisionSubjects::Execution(WorkloadExecutionReference::for_intent(
            reserved.active_intent(),
        )),
        None,
    );
    let pending = persist_attempt_fixture(&reserved, attempt);
    let claim = pending
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("pending record should retain its exact claim");
    let forged_absence = WorkloadProvisionAbsenceEvidence::for_confirmation(
        claim,
        pending.revision(),
        pending.last_transition().transition_id().clone(),
        evidence("pending-without-inspection"),
    );

    assert_eq!(
        pending
            .inspection_to_retry_dispatch(forged_absence)
            .unwrap_err(),
        WorkloadSagaError::InvalidTransition("dispatch retry requires an exact inspected claim")
    );
    assert_eq!(pending.revision(), claim.claimed_revision());
}

#[test]
fn repeated_inspection_retry_history_has_no_fixed_revision_limit() {
    let initial = WorkloadSagaRecord::new(
        key("tenant-a", "workload-repeated-retry"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .expect("running record should validate");
    let reserved = advance_provision(&initial, WorkloadSagaPhase::NetworkReserved, None);
    let attempt = provision_attempt_fixture(
        &reserved,
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadProvisionSubjects::Execution(WorkloadExecutionReference::for_intent(
            reserved.active_intent(),
        )),
        None,
    );
    let mut pending = persist_attempt_fixture(&reserved, attempt);
    let stable_attempt_id = pending
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("initial claim should exist")
        .attempt()
        .attempt_id()
        .clone();

    for expected_epoch in 1..=8 {
        let claim = pending
            .provision_disposition()
            .and_then(WorkloadProvisionDisposition::claim)
            .expect("pending retry claim should exist")
            .clone();
        let inspection = pending
            .dispatch_to_inspection()
            .expect("every uncertain dispatch must durably enter inspection");
        let absence = WorkloadProvisionAbsenceEvidence::for_inspection(
            &inspection,
            &claim,
            evidence(&format!("provider-absent-{expected_epoch}")),
        )
        .expect("absence should bind each exact retry transition");
        pending = inspection
            .inspection_to_retry_dispatch(absence)
            .expect("exact absence should authorize the next epoch");
        let next = pending
            .provision_disposition()
            .and_then(WorkloadProvisionDisposition::claim)
            .expect("next retry claim should exist");
        assert_eq!(next.attempt().attempt_id(), &stable_attempt_id);
        assert_eq!(next.dispatch_epoch().as_u64(), expected_epoch);
        assert_eq!(next.claimed_revision(), pending.revision());
        pending
            .validate()
            .expect("repeated retry state should validate");
    }
}

#[test]
fn retry_reusing_skipping_or_crossing_absence_transition_is_rejected() {
    let initial = WorkloadSagaRecord::new(
        key("tenant-a", "workload-forged-retry"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .expect("running record should validate");
    let reserved = advance_provision(&initial, WorkloadSagaPhase::NetworkReserved, None);
    let attempt = provision_attempt_fixture(
        &reserved,
        WorkloadProvisionStep::PrepareWorkload,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadProvisionSubjects::Execution(WorkloadExecutionReference::for_intent(
            reserved.active_intent(),
        )),
        None,
    );
    let pending = persist_attempt_fixture(&reserved, attempt);
    let claim = pending
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("claim should exist");
    assert_eq!(
        WorkloadProvisionAbsenceEvidence::for_inspection(
            &pending,
            claim,
            evidence("forged-pending-absence"),
        )
        .unwrap_err(),
        WorkloadSagaError::InvalidEvidence(
            "absence observation requires the exact durable inspection state"
        )
    );
    let inspection = pending
        .dispatch_to_inspection()
        .expect("uncertain dispatch must enter inspection before absence");
    let absence = WorkloadProvisionAbsenceEvidence::for_inspection(
        &inspection,
        claim,
        evidence("provider-absent"),
    )
    .expect("absence should bind the exact durable inspection state");
    let retry = inspection
        .inspection_to_retry_dispatch(absence)
        .expect("exact next epoch should validate");
    let retry_claim = retry
        .provision_disposition()
        .and_then(WorkloadProvisionDisposition::claim)
        .expect("retry claim should exist");
    let exact = serde_json::to_value(retry_claim).expect("retry claim should encode");

    for forged_epoch in ["0", "2"] {
        let mut forged = exact.clone();
        forged["dispatchEpoch"] = json!(forged_epoch);
        assert!(serde_json::from_value::<WorkloadProvisionDispatchClaim>(forged).is_err());
    }

    let mut crossed_transition = serde_json::to_value(&retry).expect("retry record should encode");
    crossed_transition["provisionDisposition"]["value"]["authorization"]["evidence"]["transitionId"] =
        json!(format!("wst_{}", "9".repeat(64)));
    assert!(
        serde_json::from_value::<WorkloadSagaRecord>(crossed_transition).is_err(),
        "portable state must reject absence evidence crossed with its predecessor transition"
    );
}
