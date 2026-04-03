use super::*;

fn queued_request(enqueued_at: Instant) -> QueuedMutationRequest {
    let (response, _response_rx) = oneshot::channel();
    QueuedMutationRequest {
        mutation: Mutation::Insert {
            table: TableName::new("tasks").expect("table name should build"),
            fields: serde_json::Map::new(),
        },
        principal: PrincipalContext::anonymous(),
        scheduled_execution_id: None,
        cancelled: Arc::new(AtomicBool::new(false)),
        _operation: TenantOperationGuard {
            lifecycle: Arc::new(TenantLifecycle::new()),
        },
        response,
        enqueued_at,
    }
}

#[test]
fn mutation_admission_gate_codel_sheds_stale_request_after_interval() {
    let gate = MutationAdmissionGate::new();
    gate.set_codel_for_testing(Duration::from_millis(5), Duration::from_millis(10));

    let now = Instant::now();
    gate.enqueue(queued_request(now - Duration::from_millis(40)))
        .expect("first request should enqueue");
    gate.enqueue(queued_request(now - Duration::from_millis(40)))
        .expect("second request should enqueue");

    assert!(matches!(
        gate.pop_next_at(now),
        MutationAdmissionDecision::Admit(_)
    ));

    match gate.pop_next_at(now + Duration::from_millis(11)) {
        MutationAdmissionDecision::Reject { error, .. } => {
            assert!(matches!(
                error,
                Error::ResourceExhausted(message)
                    if message.contains("mutation shed by admission gate")
            ));
        }
        MutationAdmissionDecision::Admit(_) => {
            panic!("expected second request to be shed, got an admitted request")
        }
        MutationAdmissionDecision::Empty => {
            panic!("expected second request to be shed, got an empty gate")
        }
    }

    assert!(matches!(
        gate.pop_next_at(now + Duration::from_millis(12)),
        MutationAdmissionDecision::Empty
    ));

    let stats = gate.stats();
    assert_eq!(stats.queue_depth, 0);
    assert_eq!(stats.admitted_count, 2);
    assert_eq!(stats.shed_count, 1);
    assert_eq!(stats.queue_rejection_count, 0);
    assert_eq!(stats.codel_phase, MutationAdmissionPhase::Idle);
}
