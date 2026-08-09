use super::*;

fn candidate_record(label: &str) -> WorkloadSagaRecord {
    let key = workload_key(label);
    let base = intent_with_publication(
        &key,
        DesiredWorkloadState::Running,
        1,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let intent = WorkloadSagaIntent::new_with_restart_policy(
        base.kind(),
        base.desired_state(),
        base.generation(),
        base.executable().clone(),
        base.source().clone(),
        crate::WorkloadRestartPolicy::Always { max_restarts: 2 },
        base.network().clone(),
        base.activation(),
        base.publication(),
        base.admission().clone(),
    )
    .expect("candidate restart policy should validate");
    let publication = WorkloadPublicationReference::new([PublishedEndpointId::generate()], &intent)
        .expect("candidate publication should validate");
    let mut record = WorkloadSagaRecord::new(key, intent).unwrap();
    for target in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Published,
        WorkloadSagaPhase::Observed,
    ] {
        record = advance_provision(&record, target, &publication);
    }
    record
}

#[test]
fn restart_candidate_request_bounds_are_strict() {
    assert!(WorkloadRestartCandidatePageRequest::new(None, 0).is_err());
    assert!(WorkloadRestartCandidatePageRequest::new(None, MAX_WORKLOAD_SAGA_PAGE_SIZE).is_ok());
    assert!(
        WorkloadRestartCandidatePageRequest::new(
            None,
            MAX_WORKLOAD_SAGA_PAGE_SIZE.saturating_add(1),
        )
        .is_err()
    );
}

#[test]
fn restart_candidate_cursor_rejects_ineligible_records() {
    let ineligible = WorkloadSagaRecord::new(
        workload_key("restart-ineligible"),
        intent(
            &workload_key("restart-ineligible"),
            DesiredWorkloadState::Running,
            WorkloadActivationIntent::ActivateWhenAttached,
        ),
    )
    .unwrap();
    assert!(WorkloadRestartCandidateCursor::for_record(&ineligible).is_err());
    assert!(
        WorkloadRestartCandidateCursor::for_record(&candidate_record("restart-eligible")).is_ok()
    );
}

#[test]
fn restart_candidate_page_requires_candidates_order_and_full_more_page() {
    let first = candidate_record("restart-page-a");
    let second = candidate_record("restart-page-b");
    let request = WorkloadRestartCandidatePageRequest::new(None, 2).unwrap();

    assert!(WorkloadRestartCandidatePage::new(&request, vec![first.clone()], true).is_err());
    assert!(
        WorkloadRestartCandidatePage::new(&request, vec![second.clone(), first.clone()], false,)
            .is_err()
    );
    assert!(
        WorkloadRestartCandidatePage::new(&request, vec![first.clone(), first.clone()], false,)
            .is_err()
    );

    let page =
        WorkloadRestartCandidatePage::new(&request, vec![first.clone(), second.clone()], true)
            .expect("ordered full page should validate");
    assert_eq!(page.records(), &[first, second]);
    assert_eq!(
        page.next_cursor()
            .map(WorkloadRestartCandidateCursor::saga_id),
        page.records().last().map(WorkloadSagaRecord::saga_id),
    );
}

#[test]
fn restart_candidate_page_rejects_cursor_regression() {
    let mut records = vec![
        candidate_record("restart-cursor-a"),
        candidate_record("restart-cursor-b"),
    ];
    records.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
    let first = records.remove(0);
    let second = records.remove(0);
    let request = WorkloadRestartCandidatePageRequest::new(
        Some(WorkloadRestartCandidateCursor::for_record(&second).unwrap()),
        1,
    )
    .unwrap();
    assert!(WorkloadRestartCandidatePage::new(&request, vec![first], false).is_err());
}
