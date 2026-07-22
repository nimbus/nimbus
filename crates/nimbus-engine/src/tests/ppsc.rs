use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_core::{DocumentId, ManualMonotonicClock, SeededIdSource, TenantEventKind, hex_encode};
use nimbus_testing::ppsc::{
    PpscBackend, PpscEffect, PpscExpectedOutcome, PpscFrontiers, PpscHistory, PpscJournalEntry,
    PpscObservedStep, PpscOperation, PpscPublication, PpscRoute, PpscScenario, PpscSequenceClaim,
    PpscSequenceOwnership, PpscStep, PpscStorageFaultInjector, PpscTenantState, PpscTerminalState,
    audit_ppsc_history,
};

use super::*;

const PPSC_OBSERVER: &str = "ppsc-scenario-recorder";

#[derive(Default)]
struct PpscPublicationRecorder {
    current_step: AtomicUsize,
    publications: Mutex<Vec<(TenantId, SequenceNumber, usize)>>,
}

impl PpscPublicationRecorder {
    fn enter_step(&self, step: usize) {
        self.current_step.store(step, Ordering::Release);
    }

    fn for_tenant(&self, tenant_id: &TenantId) -> Vec<(SequenceNumber, usize)> {
        self.publications
            .lock()
            .expect("PPSC publication recorder lock should not be poisoned")
            .iter()
            .filter_map(|(candidate, sequence, step)| {
                (candidate == tenant_id).then_some((*sequence, *step))
            })
            .collect()
    }
}

impl crate::CommittedMutationObserver for PpscPublicationRecorder {
    fn committed_mutation_applied(&self, event: crate::CommittedMutationEvent) {
        self.publications
            .lock()
            .expect("PPSC publication recorder lock should not be poisoned")
            .push((
                event.tenant_id,
                event.commit.sequence,
                self.current_step.load(Ordering::Acquire),
            ));
    }
}

struct PpscEmbeddedRunner {
    backend: PpscBackend,
    _data_dir: TempDir,
    engine: Arc<Engine>,
    wall_clock: Arc<ManualWallClock>,
    monotonic_clock: Arc<ManualMonotonicClock>,
    _storage_faults: Arc<PpscStorageFaultInjector>,
    publications: Arc<PpscPublicationRecorder>,
    tenants: BTreeMap<String, TenantId>,
    scenario_seed: u64,
}

impl PpscEmbeddedRunner {
    async fn new(backend: PpscBackend, scenario: &PpscScenario) -> Self {
        assert!(
            matches!(
                backend,
                PpscBackend::Memory | PpscBackend::Redb | PpscBackend::Sqlite
            ),
            "embedded runner cannot construct backend {}",
            backend.as_str()
        );
        scenario
            .validate_for_backend(backend)
            .expect("scenario should be supported by its selected backend");
        let data_dir = tempdir().expect("PPSC engine data dir should build");
        let wall_clock = Arc::new(ManualWallClock::new(Timestamp(100_000)));
        let monotonic_clock = Arc::new(ManualMonotonicClock::new());
        let storage_faults = PpscStorageFaultInjector::new();
        let id_source = Arc::new(SeededIdSource::new(scenario.seed));
        let engine = Arc::new(
            match backend {
                PpscBackend::Memory => Engine::new_with_simulation_clocks_and_memory_persistence(
                    data_dir.path(),
                    wall_clock.clone(),
                    monotonic_clock.clone(),
                    storage_faults.clone(),
                    id_source,
                ),
                PpscBackend::Redb | PpscBackend::Sqlite => {
                    Engine::new_with_simulation_clocks_id_source_and_embedded_provider(
                        data_dir.path(),
                        wall_clock.clone(),
                        monotonic_clock.clone(),
                        storage_faults.clone(),
                        id_source,
                        match backend {
                            PpscBackend::Redb => EmbeddedProviderKind::Redb,
                            PpscBackend::Sqlite => EmbeddedProviderKind::Sqlite,
                            _ => unreachable!("embedded backend match is exhaustive"),
                        },
                    )
                }
                _ => unreachable!("provider backends were rejected above"),
            }
            .expect("PPSC embedded engine should construct"),
        );
        let publications = Arc::new(PpscPublicationRecorder::default());
        engine.install_committed_mutation_observer(PPSC_OBSERVER, publications.clone());

        let tenant_names = scenario
            .steps
            .iter()
            .filter_map(|step| step.operation.tenant())
            .collect::<BTreeSet<_>>();
        let mut tenants = BTreeMap::new();
        for tenant_name in tenant_names {
            let tenant_id = TenantId::new(tenant_name).expect("scenario tenant id should parse");
            engine
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("scenario tenant should create through async lifecycle");
            engine
                .shutdown_trigger_candidates_for_testing(&tenant_id)
                .expect("ambient trigger-cursor work should stop before the scenario");
            engine
                .flush_tenant_committer_for_testing(&tenant_id)
                .await
                .expect("tenant setup should drain before the scenario");
            tenants.insert(tenant_name.to_string(), tenant_id);
        }

        Self {
            backend,
            _data_dir: data_dir,
            engine,
            wall_clock,
            monotonic_clock,
            _storage_faults: storage_faults,
            publications,
            tenants,
            scenario_seed: scenario.seed,
        }
    }

    async fn run(mut self, scenario: PpscScenario) -> PpscHistory {
        let mut observed_steps = Vec::with_capacity(scenario.steps.len());
        let mut sequence_claims = Vec::new();
        for (index, step) in scenario.steps.iter().enumerate() {
            self.publications.enter_step(index);
            let before = self.journal_heads();
            let outcome = self.execute_step(index, &step.operation).await;
            if let Some(tenant) = step.operation.tenant() {
                let tenant_id = self.tenant(tenant).clone();
                self.engine
                    .flush_tenant_committer_for_testing(&tenant_id)
                    .await
                    .expect("scenario committer work should drain");
                self.engine
                    .flush_committed_mutation_observers_for_testing(&tenant_id)
                    .await
                    .expect("scenario publication work should drain");
            }
            let effects = self.new_effects(&before);
            for effect in &effects {
                let Some(sequence) = effect.sequence else {
                    continue;
                };
                let tenant_id = self.tenant(&effect.tenant);
                let record = self
                    .engine
                    .read_durable_journal(tenant_id, SequenceNumber(sequence.saturating_sub(1)))
                    .expect("claimed journal record should read")
                    .into_iter()
                    .find(|record| record.sequence.0 == sequence)
                    .expect("claimed sequence should exist durably");
                sequence_claims.push(PpscSequenceClaim {
                    tenant: effect.tenant.clone(),
                    sequence,
                    identity: hex_encode(record.integrity_sha256),
                    ownership: PpscSequenceOwnership::Durable,
                    step: index,
                });
            }
            observed_steps.push(PpscObservedStep {
                index,
                outcome,
                effects,
            });
        }

        let terminal = self.terminal_state().await;
        self.engine.quiesce().await;
        PpscHistory {
            backend: self.backend,
            scenario,
            observed_steps,
            sequence_claims,
            terminal,
        }
    }

    async fn execute_step(
        &mut self,
        index: usize,
        operation: &PpscOperation,
    ) -> PpscExpectedOutcome {
        match operation {
            PpscOperation::Mutation {
                tenant,
                route,
                key,
                value,
            } => {
                let tenant_id = self.tenant(tenant).clone();
                let document_id = DocumentId::from_key(format!(
                    "ppsc-{:016x}-{index:03}-{key}",
                    self.scenario_seed
                ))
                .expect("scenario document id should parse");
                let fields = serde_json::Map::from_iter([
                    ("key".to_string(), json!(key)),
                    ("value".to_string(), json!(value)),
                ]);
                match route {
                    PpscRoute::QueuedJournal => {
                        self.engine
                            .insert_document_async_with_id(
                                tenant_id,
                                tasks_table(),
                                document_id,
                                fields,
                            )
                            .await
                            .expect("queued PPSC mutation should commit");
                    }
                    PpscRoute::Direct => {
                        self.engine
                            .insert_document_with_id(&tenant_id, tasks_table(), document_id, fields)
                            .expect("direct PPSC mutation should commit");
                    }
                    PpscRoute::ExecutionUnit => {
                        let unit = self
                            .engine
                            .begin_mutation_execution_unit(tenant_id, PrincipalContext::anonymous())
                            .expect("PPSC execution unit should begin");
                        unit.insert_document_with_id(tasks_table(), Some(document_id), fields)
                            .expect("PPSC execution-unit mutation should stage");
                        unit.commit()
                            .expect("PPSC execution unit should commit")
                            .expect("PPSC execution unit should emit a durable record");
                    }
                }
                PpscExpectedOutcome::Committed
            }
            PpscOperation::AdvanceWallClock { millis } => {
                self.wall_clock.advance_ms(*millis);
                PpscExpectedOutcome::Observed
            }
            PpscOperation::AdvanceMonotonicClock { millis } => {
                self.monotonic_clock.advance(Duration::from_millis(*millis));
                PpscExpectedOutcome::Observed
            }
            PpscOperation::Quiesce => PpscExpectedOutcome::Shutdown,
            other => panic!("PPSC Engine runner does not yet implement operation {other:?}"),
        }
    }

    fn tenant(&self, name: &str) -> &TenantId {
        self.tenants
            .get(name)
            .unwrap_or_else(|| panic!("scenario tenant '{name}' should be loaded"))
    }

    fn journal_heads(&self) -> BTreeMap<String, u64> {
        self.tenants
            .iter()
            .map(|(name, tenant_id)| {
                let head = self
                    .engine
                    .read_durable_journal(tenant_id, SequenceNumber(0))
                    .expect("scenario journal should read")
                    .last()
                    .map_or(0, |record| record.sequence.0);
                (name.clone(), head)
            })
            .collect()
    }

    fn new_effects(&self, before: &BTreeMap<String, u64>) -> Vec<PpscEffect> {
        self.tenants
            .iter()
            .flat_map(|(name, tenant_id)| {
                self.engine
                    .read_durable_journal(
                        tenant_id,
                        SequenceNumber(before.get(name).copied().unwrap_or(0)),
                    )
                    .expect("scenario effects should read")
                    .into_iter()
                    .map(|record| PpscEffect {
                        tenant: name.clone(),
                        sequence: Some(record.sequence.0),
                        kind: record_kind(&record.events),
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    async fn terminal_state(&self) -> PpscTerminalState {
        let mut tenants = BTreeMap::new();
        for (name, tenant_id) in &self.tenants {
            self.engine
                .flush_tenant_committer_for_testing(tenant_id)
                .await
                .expect("terminal committer should drain");
            self.engine
                .flush_committed_mutation_observers_for_testing(tenant_id)
                .await
                .expect("terminal publication should drain");
            let stats = self
                .engine
                .mutation_journal_stats_for_testing(tenant_id)
                .expect("terminal frontiers should load");
            let journal = self
                .engine
                .read_durable_journal(tenant_id, SequenceNumber(0))
                .expect("terminal journal should read");
            let identities = journal
                .iter()
                .map(|record| (record.sequence, hex_encode(record.integrity_sha256)))
                .collect::<BTreeMap<_, _>>();
            let publications = self
                .publications
                .for_tenant(tenant_id)
                .into_iter()
                .map(|(sequence, step)| PpscPublication {
                    tenant: name.clone(),
                    sequence: sequence.0,
                    identity: identities
                        .get(&sequence)
                        .expect("published record must remain in the durable journal")
                        .clone(),
                    step,
                })
                .collect();
            let documents = self
                .engine
                .query_documents(tenant_id, &query_for("tasks"))
                .expect("terminal documents should read")
                .into_iter()
                .map(|document| {
                    (
                        document.id.to_string(),
                        serde_json::to_vec(&document)
                            .expect("terminal document should serialize canonically"),
                    )
                })
                .collect();
            tenants.insert(
                name.clone(),
                PpscTenantState {
                    frontiers: PpscFrontiers {
                        assigned_high_water: stats.assigned_high_water.0,
                        active_assigned_head: stats.active_assigned_head.0,
                        durable_head: stats.durable_head.0,
                        storage_applied_head: stats.storage_applied_head.0,
                        published_head: stats.published_head.0,
                        applied_head: stats.applied_head.0,
                    },
                    journal: journal
                        .iter()
                        .map(|record| PpscJournalEntry {
                            sequence: record.sequence.0,
                            canonical_bytes:
                                nimbus_storage::commit_log::serialize_tenant_event_record(record)
                                    .expect("production journal record should serialize"),
                        })
                        .collect(),
                    publications,
                    documents,
                    ..PpscTenantState::default()
                },
            );
        }
        PpscTerminalState { tenants }
    }
}

fn record_kind(events: &[TenantEventKind]) -> String {
    events
        .iter()
        .map(|event| match event {
            TenantEventKind::DocumentWrite { .. } => "document-write",
            TenantEventKind::SchemaChange { .. } => "schema-change",
            TenantEventKind::TableLifecycle { .. } => "table-lifecycle",
            TenantEventKind::IndexLifecycle { .. } => "index-lifecycle",
            TenantEventKind::ScheduledExecution { .. } => "scheduled-execution",
            TenantEventKind::TriggerDelivery { .. } => "trigger-delivery",
            TenantEventKind::Barrier { .. } => "barrier",
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn three_route_scenario() -> PpscScenario {
    PpscScenario::new(
        "three-production-routes",
        401,
        vec![
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: "ppsc-hot".to_string(),
                    route: PpscRoute::QueuedJournal,
                    key: "queued".to_string(),
                    value: 1,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: "ppsc-hot".to_string(),
                    route: PpscRoute::Direct,
                    key: "direct".to_string(),
                    value: 2,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: "ppsc-hot".to_string(),
                    route: PpscRoute::ExecutionUnit,
                    key: "execution-unit".to_string(),
                    value: 3,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::Mutation {
                    tenant: "ppsc-peer".to_string(),
                    route: PpscRoute::QueuedJournal,
                    key: "peer".to_string(),
                    value: 4,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(PpscOperation::Quiesce, PpscExpectedOutcome::Shutdown),
        ],
    )
    .expect("three-route scenario should build")
}

#[tokio::test(flavor = "multi_thread")]
async fn ppsc_engine_runner_exercises_three_production_commit_paths() {
    let scenario = three_route_scenario();
    let mut terminal_states = Vec::new();
    for backend in [PpscBackend::Memory, PpscBackend::Redb, PpscBackend::Sqlite] {
        let history = PpscEmbeddedRunner::new(backend, &scenario)
            .await
            .run(scenario.clone())
            .await;
        audit_ppsc_history(&history).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(history.observed_steps.len(), 5);
        assert_eq!(history.sequence_claims.len(), 4);
        terminal_states.push(history.terminal);
    }
    assert_eq!(terminal_states[0], terminal_states[1]);
    assert_eq!(terminal_states[1], terminal_states[2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn ppsc_engine_runner_replay_is_byte_deterministic() {
    let scenario = three_route_scenario();
    let first = PpscEmbeddedRunner::new(PpscBackend::Redb, &scenario)
        .await
        .run(scenario.clone())
        .await;
    let replay = PpscEmbeddedRunner::new(PpscBackend::Redb, &scenario)
        .await
        .run(scenario)
        .await;
    assert_eq!(first.canonical_bytes(), replay.canonical_bytes());
}
