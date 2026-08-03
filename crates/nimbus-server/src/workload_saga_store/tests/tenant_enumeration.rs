use std::collections::BTreeSet;

use nimbus_core::{TenantId, WorkloadId};
use nimbus_workloads::{
    DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity, WorkloadActivationIntent,
    WorkloadAdmissionEvidence, WorkloadEffectReferences, WorkloadFailureEvidence,
    WorkloadGeneration, WorkloadInspectionRequirement, WorkloadNetworkIntent,
    WorkloadOwnerEvidenceDigest, WorkloadPhaseDetail, WorkloadPublicationIntent,
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaIntent, WorkloadSagaIntentUpdate,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
    WorkloadSagaTenantCursor, WorkloadSagaTenantPageRequest, WorkloadTerminalEvidenceDigest,
    WorkloadTerminalObservation,
};

use super::super::EngineWorkloadSagaStore;
use super::super::tenant_enumeration::decode_tenant_page;
use super::{compiled_network_plan, document_for, engine, provision_fixture, provision_source};

#[tokio::test]
async fn tenant_inventory_isolated_stably_ordered_bounded_indexed_and_durable_after_reopen() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let writer_engine = engine(&root);
    let writer_lifetime = std::sync::Arc::downgrade(&writer_engine);
    let store = EngineWorkloadSagaStore::new(std::sync::Arc::clone(&writer_engine));
    let tenant_id = tenant("inventory-owner");
    let other_tenant = tenant("inventory-other");
    let records = ["echo", "alpha", "delta", "bravo", "charlie"]
        .map(|workload| initial_record(&tenant_id, workload));
    for record in records
        .iter()
        .chain([initial_record(&other_tenant, "alpha")].iter())
    {
        persist(&store, WorkloadSagaExpected::Missing, record).await;
    }

    let before = writer_engine
        .query_planning_stats_for_testing(
            &super::super::schema::workload_saga_tenant().expect("system tenant is valid"),
        )
        .expect("query stats should load");
    let (first, first_page_sizes) = collect_tenant_pages(&store, &tenant_id, 2).await;
    let after = writer_engine
        .query_planning_stats_for_testing(
            &super::super::schema::workload_saga_tenant().expect("system tenant is valid"),
        )
        .expect("query stats should load");

    let mut expected = records.to_vec();
    expected.sort_by(|left, right| left.key().workload_id().cmp(right.key().workload_id()));
    assert_eq!(first_page_sizes, vec![2, 2, 1]);
    assert_eq!(first, expected);
    assert!(
        first
            .iter()
            .all(|record| record.key().tenant_id() == &tenant_id)
    );
    assert_eq!(
        after.query_composite_index_count - before.query_composite_index_count,
        3,
        "every bounded page must use by_tenantId_and_workloadId"
    );
    assert_eq!(after.query_full_scan_count, before.query_full_scan_count);
    assert_eq!(
        after.query_single_field_index_count,
        before.query_single_field_index_count
    );

    let (repeated, repeated_page_sizes) = collect_tenant_pages(&store, &tenant_id, 2).await;
    assert_eq!(repeated_page_sizes, first_page_sizes);
    assert_eq!(
        repeated, expected,
        "unchanged tenant inventory must be stable"
    );

    drop(store);
    drop(writer_engine);
    assert!(
        writer_lifetime.upgrade().is_none(),
        "all writer Engine handles must be gone before reopening durable truth"
    );
    let reopened = EngineWorkloadSagaStore::new(engine(&root));
    let (after_reopen, reopened_page_sizes) = collect_tenant_pages(&reopened, &tenant_id, 2).await;
    assert_eq!(reopened_page_sizes, vec![2, 2, 1]);
    assert_eq!(after_reopen, expected);
}

#[tokio::test]
async fn tenant_inventory_includes_every_durable_phase_including_quiescent_and_terminal() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let tenant_id = tenant("all-phases");
    let full = full_lifecycle_history(&tenant_id, "full");

    for (index, record) in full.iter().enumerate() {
        let expected = index
            .checked_sub(1)
            .map_or(WorkloadSagaExpected::Missing, |previous| {
                WorkloadSagaExpected::Revision(full[previous].revision())
            });
        persist(&store, expected, record).await;
        assert_current_phase_is_enumerated(&store, record).await;
    }

    let cleanup = cleanup_pending_history(&tenant_id, "cleanup");
    for (index, record) in cleanup.iter().enumerate() {
        let expected = index
            .checked_sub(1)
            .map_or(WorkloadSagaExpected::Missing, |previous| {
                WorkloadSagaExpected::Revision(cleanup[previous].revision())
            });
        persist(&store, expected, record).await;
    }
    assert_current_phase_is_enumerated(&store, cleanup.last().expect("history is nonempty")).await;

    let prepare_only = provision_history(
        &tenant_id,
        "prepare-only",
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    );
    for (index, record) in prepare_only.iter().enumerate() {
        let expected = index
            .checked_sub(1)
            .map_or(WorkloadSagaExpected::Missing, |previous| {
                WorkloadSagaExpected::Revision(prepare_only[previous].revision())
            });
        persist(&store, expected, record).await;
    }
    let prepare_only = prepare_only.last().expect("history is nonempty");
    assert_eq!(prepare_only.phase(), WorkloadSagaPhase::NetworkAttached);
    assert!(!prepare_only.requires_recovery());
    assert_current_phase_is_enumerated(&store, prepare_only).await;

    let terminal = stopped_record(&tenant_id, "terminal-recorded");
    persist(&store, WorkloadSagaExpected::Missing, &terminal).await;
    assert_eq!(terminal.phase(), WorkloadSagaPhase::Recorded);
    assert!(!terminal.requires_recovery());
    assert_current_phase_is_enumerated(&store, &terminal).await;

    let phases = full
        .iter()
        .chain(cleanup.last())
        .map(WorkloadSagaRecord::phase)
        .collect::<BTreeSet<_>>();
    let expected = [
        WorkloadSagaPhase::IntentCommitted,
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaPhase::Published,
        WorkloadSagaPhase::Observed,
        WorkloadSagaPhase::WithdrawalCommitted,
        WorkloadSagaPhase::Withdrawn,
        WorkloadSagaPhase::Drained,
        WorkloadSagaPhase::WorkloadStopped,
        WorkloadSagaPhase::NetworkDetached,
        WorkloadSagaPhase::NetworkReleased,
        WorkloadSagaPhase::Recorded,
        WorkloadSagaPhase::CleanupPending,
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    assert_eq!(
        phases, expected,
        "the proof fixture must exercise every phase"
    );
    assert!(
        !prepare_only.requires_recovery(),
        "prepare-only NetworkAttached must be represented as quiescent"
    );
    assert!(
        full.iter()
            .any(|record| record.phase() == WorkloadSagaPhase::Observed)
            && !terminal.requires_recovery(),
        "Observed and terminal Recorded must remain enumerable"
    );
}

#[tokio::test]
async fn tenant_inventory_cursor_mismatch_fails_closed() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let requested = tenant("cursor-requested");
    let crossed = initial_record(&tenant("cursor-crossed"), "cursor-workload");
    let request =
        WorkloadSagaTenantPageRequest::new(Some(WorkloadSagaTenantCursor::for_record(&crossed)), 2)
            .expect("request shape should be valid before tenant binding");

    assert!(matches!(
        store.list_for_tenant(&requested, request).await,
        Err(WorkloadSagaStoreError::InvalidTransition(_))
    ));
}

#[test]
fn tenant_page_decoder_rejects_crossed_malformed_and_unsorted_documents() {
    let tenant_id = tenant("decoder-owner");
    let request = WorkloadSagaTenantPageRequest::new(None, 2).expect("request should be valid");
    let crossed = initial_record(&tenant("decoder-crossed"), "alpha");
    assert_eq!(
        decode_tenant_page(&tenant_id, &request, vec![document_for(&crossed)]),
        Err(WorkloadSagaStoreError::Corrupt)
    );

    let valid = initial_record(&tenant_id, "alpha");
    let mut malformed = document_for(&valid);
    malformed
        .fields
        .insert("tenantId".to_owned(), serde_json::Value::Bool(true));
    assert_eq!(
        decode_tenant_page(&tenant_id, &request, vec![malformed]),
        Err(WorkloadSagaStoreError::Corrupt)
    );

    let first = initial_record(&tenant_id, "bravo");
    let second = initial_record(&tenant_id, "alpha");
    assert_eq!(
        decode_tenant_page(
            &tenant_id,
            &request,
            vec![document_for(&first), document_for(&second)],
        ),
        Err(WorkloadSagaStoreError::Corrupt)
    );

    let over_read = ["alpha", "bravo", "charlie", "delta"]
        .map(|workload| document_for(&initial_record(&tenant_id, workload)))
        .to_vec();
    assert_eq!(
        decode_tenant_page(&tenant_id, &request, over_read),
        Err(WorkloadSagaStoreError::Corrupt),
        "the adapter must reject more than one physical lookahead row"
    );
}

async fn collect_tenant_pages(
    store: &EngineWorkloadSagaStore,
    tenant_id: &TenantId,
    limit: u16,
) -> (Vec<WorkloadSagaRecord>, Vec<usize>) {
    let mut after = None;
    let mut records = Vec::new();
    let mut page_sizes = Vec::new();
    let mut previous = None;
    let mut seen = BTreeSet::new();

    for _ in 0..32 {
        let page = store
            .list_for_tenant(
                tenant_id,
                WorkloadSagaTenantPageRequest::new(after.clone(), limit)
                    .expect("request should be valid"),
            )
            .await
            .expect("tenant page should load");
        assert_eq!(page.tenant_id(), tenant_id);
        assert!(page.records().len() <= usize::from(limit));
        for record in page.records() {
            assert_eq!(record.key().tenant_id(), tenant_id);
            if let Some(previous) = previous.as_ref() {
                assert!(record.key().workload_id() > previous);
            }
            assert!(seen.insert(record.key().clone()));
            previous = Some(record.key().workload_id().clone());
        }

        let next = page.next_cursor().cloned();
        if let Some(next) = next.as_ref() {
            assert_eq!(page.records().len(), usize::from(limit));
            assert_eq!(next.key().workload_id(), previous.as_ref().unwrap());
        }
        page_sizes.push(page.records().len());
        records.extend(page.into_records());
        after = next;
        if after.is_none() {
            return (records, page_sizes);
        }
    }

    panic!("tenant pagination did not terminate within the fixture bound");
}

async fn assert_current_phase_is_enumerated(
    store: &EngineWorkloadSagaStore,
    expected: &WorkloadSagaRecord,
) {
    let page = store
        .list_for_tenant(
            expected.key().tenant_id(),
            WorkloadSagaTenantPageRequest::new(None, 16).expect("request should be valid"),
        )
        .await
        .expect("tenant inventory should include every phase");
    assert!(
        page.records().contains(expected),
        "missing {:?}",
        expected.phase()
    );
}

async fn persist(
    store: &EngineWorkloadSagaStore,
    expected: WorkloadSagaExpected,
    record: &WorkloadSagaRecord,
) {
    assert_eq!(
        store.compare_and_swap(expected, record.clone()).await,
        Ok(WorkloadSagaCommit::Applied),
        "fixture {:?} transition must persist",
        record.phase()
    );
}

fn full_lifecycle_history(tenant_id: &TenantId, workload: &str) -> Vec<WorkloadSagaRecord> {
    let mut history = provision_history(
        tenant_id,
        workload,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let current = history.last().expect("provision history is nonempty");
    let successor = workload_intent(
        current.key(),
        2,
        DesiredWorkloadState::Stopped,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    );
    let WorkloadSagaIntentUpdate::Transition(withdrawal) = current
        .apply_intent(successor)
        .expect("higher stopped intent should begin withdrawal")
    else {
        panic!("higher stopped intent must transition");
    };
    history.push(*withdrawal);

    for phase in [
        WorkloadSagaPhase::Withdrawn,
        WorkloadSagaPhase::Drained,
        WorkloadSagaPhase::WorkloadStopped,
        WorkloadSagaPhase::NetworkDetached,
        WorkloadSagaPhase::NetworkReleased,
    ] {
        let next = advance_teardown(history.last().unwrap(), phase);
        history.push(next);
    }
    let WorkloadPhaseDetail::Teardown(detail) = history.last().unwrap().phase_detail() else {
        panic!("released fixture has teardown detail");
    };
    let terminal_digest =
        WorkloadTerminalEvidenceDigest::for_observations(detail.terminal_observations())
            .expect("terminal observations should digest");
    let recorded = history
        .last()
        .unwrap()
        .advance(
            WorkloadSagaPhase::Recorded,
            WorkloadPhaseDetail::recorded(history.last().unwrap().active_intent(), terminal_digest),
            None,
        )
        .expect("recorded transition should validate");
    history.push(recorded);
    history
}

fn cleanup_pending_history(tenant_id: &TenantId, workload: &str) -> Vec<WorkloadSagaRecord> {
    let mut history = provision_history(
        tenant_id,
        workload,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    history.truncate(
        history
            .iter()
            .position(|record| record.phase() == WorkloadSagaPhase::Ready)
            .expect("history contains Ready")
            + 1,
    );
    let current = history.last().unwrap();
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
    .expect("cleanup detail should validate");
    let cleanup = current
        .advance(
            WorkloadSagaPhase::CleanupPending,
            detail,
            Some(
                WorkloadFailureEvidence::new("provider_timeout", evidence("provider-timeout"))
                    .expect("failure evidence should validate"),
            ),
        )
        .expect("cleanup transition should validate");
    history.push(cleanup);
    history
}

fn provision_history(
    tenant_id: &TenantId,
    workload: &str,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
) -> Vec<WorkloadSagaRecord> {
    let key = workload_key(tenant_id, workload);
    let intent = workload_intent(
        &key,
        1,
        DesiredWorkloadState::Running,
        activation,
        publication,
    );
    let mut history = vec![WorkloadSagaRecord::new(key, intent).expect("record should initialize")];
    let target = if activation == WorkloadActivationIntent::PrepareOnly {
        WorkloadSagaPhase::NetworkAttached
    } else {
        WorkloadSagaPhase::Observed
    };
    while history.last().expect("history is nonempty").phase() != target {
        provision_fixture::extend_confirmed_step(&mut history);
    }
    history
}

fn advance_teardown(record: &WorkloadSagaRecord, phase: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let WorkloadPhaseDetail::Teardown(current) = record.phase_detail() else {
        panic!("teardown record must retain teardown detail");
    };
    let references = current.retained_references().clone();
    let detail = WorkloadPhaseDetail::teardown(
        phase,
        record.active_intent(),
        current.origin(),
        references.clone(),
        terminal_observations(phase, &references),
    )
    .expect("teardown detail should validate");
    record
        .advance(phase, detail, None)
        .expect("teardown transition should validate")
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
        _ => panic!("not a teardown phase"),
    };
    let mut observations = Vec::new();
    if rank >= 1 {
        observations.push(WorkloadTerminalObservation::PublicationAbsent {
            reference: references.publication().unwrap().clone(),
            evidence: evidence("publication-absent"),
        });
    }
    if rank >= 2 {
        observations.push(WorkloadTerminalObservation::ExecutionDrained {
            reference: references.execution().unwrap().clone(),
            evidence: evidence("execution-drained"),
        });
    }
    if rank >= 3 {
        observations.push(WorkloadTerminalObservation::ExecutionStopped {
            reference: references.execution().unwrap().clone(),
            evidence: evidence("execution-stopped"),
        });
    }
    if rank >= 4 {
        observations.push(WorkloadTerminalObservation::NetworkDetached {
            reference: references.network().unwrap().clone(),
            evidence: evidence("network-detached"),
        });
    }
    if rank >= 5 {
        observations.push(WorkloadTerminalObservation::NetworkReleased {
            reference: references.network().unwrap().clone(),
            evidence: evidence("network-released"),
        });
    }
    observations
}

fn initial_record(tenant_id: &TenantId, workload: &str) -> WorkloadSagaRecord {
    let key = workload_key(tenant_id, workload);
    let intent = workload_intent(
        &key,
        1,
        DesiredWorkloadState::Running,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    WorkloadSagaRecord::new(key, intent).expect("initial record should validate")
}

fn stopped_record(tenant_id: &TenantId, workload: &str) -> WorkloadSagaRecord {
    let key = workload_key(tenant_id, workload);
    let intent = workload_intent(
        &key,
        1,
        DesiredWorkloadState::Stopped,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    );
    WorkloadSagaRecord::new(key, intent).expect("terminal record should validate")
}

fn workload_key(tenant_id: &TenantId, workload: &str) -> nimbus_workloads::WorkloadSagaKey {
    nimbus_workloads::WorkloadSagaKey::new(
        tenant_id.clone(),
        WorkloadId::new(format!("workload-{workload}")).expect("workload id should be valid"),
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
    WorkloadSagaIntent::new(
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
            format!("tid_{}", "1".repeat(64)).try_into().unwrap(),
            format!("twu_{}", "2".repeat(64)).try_into().unwrap(),
            NodeIdentity::new("node-tenant-enumeration").unwrap(),
        ),
    )
    .expect("intent should validate")
}

fn tenant(label: &str) -> TenantId {
    TenantId::new(format!("tenant-{label}")).expect("tenant id should be valid")
}

fn evidence(label: &str) -> WorkloadOwnerEvidenceDigest {
    WorkloadOwnerEvidenceDigest::sha256(label)
}
