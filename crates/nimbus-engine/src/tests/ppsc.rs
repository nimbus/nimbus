use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_core::{
    DocumentId, FieldSchema, FieldType, ManualMonotonicClock, Mutation, ScheduleRequest,
    SeededIdSource, TenantEventKind, TriggerDeliveryCursor, hex_encode,
};
use nimbus_storage::PointInTimeRestoreArchive;
use nimbus_testing::ppsc::{
    PpscBackend, PpscCommitOrder, PpscEffect, PpscExpectedOutcome, PpscFrontiers, PpscHistory,
    PpscJournalEntry, PpscObservedStep, PpscOperation, PpscPublication, PpscRoute, PpscScenario,
    PpscSequenceClaim, PpscSequenceOwnership, PpscStep, PpscStorageFaultInjector, PpscTenantState,
    PpscTerminalState, audit_ppsc_history,
};

use super::*;

const PPSC_OBSERVER: &str = "ppsc-scenario-recorder";

#[derive(Default)]
struct PpscPublicationRecorder {
    current_step: AtomicUsize,
    observer_publications: Mutex<Vec<(TenantId, SequenceNumber, usize)>>,
    published_prefix: Mutex<BTreeMap<TenantId, BTreeMap<SequenceNumber, usize>>>,
}

impl PpscPublicationRecorder {
    fn enter_step(&self, step: usize) {
        self.current_step.store(step, Ordering::Release);
    }

    fn observe_published_prefix(
        &self,
        tenant_id: &TenantId,
        published_head: SequenceNumber,
        step: usize,
    ) {
        let mut prefixes = self
            .published_prefix
            .lock()
            .expect("PPSC published-prefix recorder lock should not be poisoned");
        let tenant_prefix = prefixes.entry(tenant_id.clone()).or_default();
        for sequence in 1..=published_head.0 {
            tenant_prefix
                .entry(SequenceNumber(sequence))
                .or_insert(step);
        }
    }

    fn published_for_tenant(&self, tenant_id: &TenantId) -> Vec<(SequenceNumber, usize)> {
        self.published_prefix
            .lock()
            .expect("PPSC published-prefix recorder lock should not be poisoned")
            .get(tenant_id)
            .map(|prefix| {
                prefix
                    .iter()
                    .map(|(sequence, step)| (*sequence, *step))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn observer_for_tenant(&self, tenant_id: &TenantId) -> Vec<(SequenceNumber, usize)> {
        self.observer_publications
            .lock()
            .expect("PPSC observer recorder lock should not be poisoned")
            .iter()
            .filter_map(|(candidate, sequence, step)| {
                (candidate == tenant_id).then_some((*sequence, *step))
            })
            .collect()
    }
}

impl crate::CommittedMutationObserver for PpscPublicationRecorder {
    fn committed_mutation_applied(&self, event: crate::CommittedMutationEvent) {
        self.observer_publications
            .lock()
            .expect("PPSC publication recorder lock should not be poisoned")
            .push((
                event.tenant_id,
                event.commit.sequence,
                self.current_step.load(Ordering::Acquire),
            ));
    }
}

struct PpscEngineRunner {
    backend: PpscBackend,
    _data_dir: Option<TempDir>,
    engine: Arc<Engine>,
    wall_clock: Arc<ManualWallClock>,
    monotonic_clock: Arc<ManualMonotonicClock>,
    _storage_faults: Arc<PpscStorageFaultInjector>,
    publications: Arc<PpscPublicationRecorder>,
    tenants: BTreeMap<String, TenantId>,
    restore_archives: BTreeMap<u64, PointInTimeRestoreArchive>,
    scenario_seed: u64,
}

impl PpscEngineRunner {
    async fn new_embedded(backend: PpscBackend, scenario: &PpscScenario) -> Self {
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
        Self::finish_new(
            backend,
            scenario,
            Some(data_dir),
            engine,
            wall_clock,
            monotonic_clock,
            storage_faults,
        )
        .await
    }

    async fn new_configured_provider(
        backend: PpscBackend,
        scenario: &PpscScenario,
        config: EnginePersistenceConfig,
    ) -> Self {
        assert!(
            backend.capabilities().provider_authority,
            "configured provider runner requires a provider backend, got {}",
            backend.as_str()
        );
        scenario
            .validate_for_backend(backend)
            .expect("scenario should be supported by its selected backend");
        let wall_clock = Arc::new(ManualWallClock::new(Timestamp(100_000)));
        let monotonic_clock = Arc::new(ManualMonotonicClock::new());
        let storage_faults = PpscStorageFaultInjector::new();
        let id_source = Arc::new(SeededIdSource::new(scenario.seed));
        let engine = Arc::new(
            Engine::new_with_simulation_clocks_id_source_and_persistence_config(
                config,
                wall_clock.clone(),
                monotonic_clock.clone(),
                storage_faults.clone(),
                id_source,
            )
            .await
            .expect("PPSC provider engine should construct"),
        );
        Self::finish_new(
            backend,
            scenario,
            None,
            engine,
            wall_clock,
            monotonic_clock,
            storage_faults,
        )
        .await
    }

    async fn finish_new(
        backend: PpscBackend,
        scenario: &PpscScenario,
        data_dir: Option<TempDir>,
        engine: Arc<Engine>,
        wall_clock: Arc<ManualWallClock>,
        monotonic_clock: Arc<ManualMonotonicClock>,
        storage_faults: Arc<PpscStorageFaultInjector>,
    ) -> Self {
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

        let archive_ids = scenario
            .steps
            .iter()
            .filter_map(|step| match &step.operation {
                PpscOperation::RestoreImport { archive, .. } => Some(*archive),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let mut restore_archives = BTreeMap::new();
        for archive_id in archive_ids {
            let source_id = TenantId::new(format!("r-{:016x}-{archive_id:016x}", scenario.seed))
                .expect("PPSC restore source tenant id should parse");
            engine
                .create_tenant_async(source_id.clone())
                .await
                .expect("PPSC restore source tenant should create");
            engine
                .shutdown_trigger_candidates_for_testing(&source_id)
                .expect("PPSC restore source trigger worker should stop");
            let document_id = DocumentId::from_key(format!(
                "ppsc-{:016x}-archive-{archive_id:016x}",
                scenario.seed
            ))
            .expect("PPSC restore source document id should parse");
            engine
                .insert_document_async_with_id(
                    source_id.clone(),
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([
                        ("key".to_string(), json!(format!("archive-{archive_id}"))),
                        ("archive".to_string(), json!(archive_id)),
                    ]),
                )
                .await
                .expect("PPSC restore source document should commit");
            engine
                .flush_tenant_committer_for_testing(&source_id)
                .await
                .expect("PPSC restore source committer should drain");
            engine
                .flush_committed_mutation_observers_for_testing(&source_id)
                .await
                .expect("PPSC restore source publication should drain");
            let archive = engine
                .export_latest_point_in_time_restore_archive(&source_id)
                .expect("PPSC point-in-time archive should export");
            assert_eq!(
                archive.target_sequence,
                SequenceNumber(1),
                "PPSC restore fixture should contain exactly one source mutation"
            );
            restore_archives.insert(archive_id, archive);
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
            restore_archives,
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
            self.observe_published_prefixes(index);
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
                self.commit_route_insert(index, tenant, *route, key, *value)
                    .await;
                PpscExpectedOutcome::Committed
            }
            PpscOperation::CommitPermutation {
                tenant,
                order,
                value_base,
            } => {
                for route in routes_for_order(*order) {
                    let (route_name, value_offset) = match route {
                        PpscRoute::QueuedJournal => ("queued", 1_i64),
                        PpscRoute::Direct => ("direct", 2),
                        PpscRoute::ExecutionUnit => ("execution-unit", 3),
                    };
                    self.commit_route_insert(
                        index,
                        tenant,
                        route,
                        &format!("permutation-{route_name}"),
                        value_base.saturating_add(value_offset),
                    )
                    .await;
                }
                PpscExpectedOutcome::Committed
            }
            PpscOperation::ZeroWriteExecutionUnit { tenant } => {
                let tenant_id = self.tenant(tenant).clone();
                let before = self
                    .engine
                    .latest_sequence(&tenant_id)
                    .expect("PPSC zero-write head should read");
                let unit = self
                    .engine
                    .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
                    .expect("PPSC zero-write execution unit should begin");
                assert_eq!(
                    unit.commit()
                        .expect("PPSC zero-write execution unit should finalize"),
                    None,
                    "PPSC zero-write execution unit must not emit a commit"
                );
                assert_eq!(
                    self.engine
                        .latest_sequence(&tenant_id)
                        .expect("PPSC zero-write final head should read"),
                    before,
                    "PPSC zero-write execution unit must not consume a sequence"
                );
                PpscExpectedOutcome::Observed
            }
            PpscOperation::ConflictRetry {
                tenant,
                key,
                first,
                second,
            } => {
                let tenant_id = self.tenant(tenant).clone();
                let document_id = DocumentId::from_key(format!(
                    "ppsc-{:016x}-{index:03}-conflict-{key}",
                    self.scenario_seed
                ))
                .expect("PPSC conflict document id should parse");
                let first_unit = self
                    .engine
                    .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
                    .expect("PPSC first conflict execution unit should begin");
                let second_unit = self
                    .engine
                    .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
                    .expect("PPSC second conflict execution unit should begin");
                first_unit
                    .insert_document_with_id(
                        tasks_table(),
                        Some(document_id.clone()),
                        ppsc_fields(key, *first),
                    )
                    .expect("PPSC first conflict write should stage");
                second_unit
                    .insert_document_with_id(
                        tasks_table(),
                        Some(document_id.clone()),
                        ppsc_fields(key, *second),
                    )
                    .expect("PPSC second conflict write should stage");
                let first_commit = first_unit
                    .commit()
                    .expect("PPSC first conflict execution unit should commit")
                    .expect("PPSC first conflict execution unit should emit a record");
                let conflict = second_unit
                    .commit()
                    .expect_err("PPSC stale execution unit should conflict");
                assert!(
                    matches!(&conflict, nimbus_core::Error::Conflict { .. }),
                    "PPSC stale execution unit must report a typed conflict: {conflict}"
                );
                let conflicting_sequence = conflict
                    .conflicting_sequence()
                    .unwrap_or(first_commit.sequence);
                assert_eq!(conflicting_sequence, first_commit.sequence);
                let retry_count_before = self
                    .engine
                    .tenant_engine_diagnostics(&tenant_id)
                    .expect("PPSC conflict diagnostics should read")
                    .commit_phases
                    .reprepare_total;
                self.engine
                    .record_mutation_conflict_retry(&tenant_id)
                    .expect("PPSC conflict retry should be recorded");
                self.engine
                    .wait_for_applied_sequence_blocking(&tenant_id, conflicting_sequence)
                    .expect("PPSC conflict retry should wait for the conflicting sequence");
                let retry = self
                    .engine
                    .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
                    .expect("PPSC conflict retry execution unit should begin");
                retry
                    .update_document(tasks_table(), document_id, ppsc_fields(key, *second))
                    .expect("PPSC conflict retry update should stage");
                retry
                    .commit()
                    .expect("PPSC conflict retry should commit")
                    .expect("PPSC conflict retry should emit a record");
                assert_eq!(
                    self.engine
                        .tenant_engine_diagnostics(&tenant_id)
                        .expect("PPSC final conflict diagnostics should read")
                        .commit_phases
                        .reprepare_total,
                    retry_count_before + 1,
                    "PPSC conflict retry must increment the retry diagnostic exactly once"
                );
                PpscExpectedOutcome::Committed
            }
            PpscOperation::SchemaSet { tenant, revision } => {
                self.engine
                    .set_table_schema_async(
                        self.tenant(tenant).clone(),
                        ppsc_schema(tasks_table(), *revision),
                    )
                    .await
                    .expect("PPSC schema set should commit");
                PpscExpectedOutcome::Committed
            }
            PpscOperation::SchemaDelete { tenant } => {
                self.engine
                    .delete_table_schema_async(self.tenant(tenant).clone(), tasks_table())
                    .await
                    .expect("PPSC schema delete should commit");
                PpscExpectedOutcome::Committed
            }
            PpscOperation::TriggerCursorAdvance { tenant, through } => {
                self.engine
                    .set_trigger_delivery_cursor_for_testing(
                        self.tenant(tenant),
                        TriggerDeliveryCursor::new(SequenceNumber(*through)),
                    )
                    .expect("PPSC trigger cursor should advance through the internal committer");
                PpscExpectedOutcome::Committed
            }
            PpscOperation::Schedule { tenant, job } => {
                let document_id = DocumentId::from_key(format!(
                    "ppsc-{:016x}-scheduled-{job:016x}",
                    self.scenario_seed
                ))
                .expect("PPSC scheduled document id should parse");
                self.engine
                    .schedule_mutation_async(
                        self.tenant(tenant).clone(),
                        ScheduleRequest {
                            run_after_ms: 10_000_u64.saturating_add(*job % 10_000),
                            mutation: Mutation::Insert {
                                table: tasks_table(),
                                id: Some(document_id),
                                fields: serde_json::Map::from_iter([
                                    ("key".to_string(), json!(format!("scheduled-{job}"))),
                                    ("value".to_string(), json!(job)),
                                ]),
                            },
                        },
                    )
                    .await
                    .expect("PPSC scheduled work should commit");
                PpscExpectedOutcome::Committed
            }
            PpscOperation::ProjectionUpdate { tenant, revision } => {
                self.engine
                    .set_table_schema_async(
                        self.tenant(tenant).clone(),
                        ppsc_schema(
                            TableName::new("ppsc_projection")
                                .expect("PPSC projection table should parse"),
                            *revision,
                        ),
                    )
                    .await
                    .expect("PPSC projection source update should commit");
                PpscExpectedOutcome::Committed
            }
            PpscOperation::RestoreImport { tenant, archive } => {
                let restore_archive = self
                    .restore_archives
                    .get(archive)
                    .unwrap_or_else(|| panic!("PPSC restore archive {archive} should exist"))
                    .clone();
                self.engine
                    .import_point_in_time_restore_archive(self.tenant(tenant), &restore_archive)
                    .expect("PPSC point-in-time archive should import");
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

    async fn commit_route_insert(
        &self,
        step: usize,
        tenant: &str,
        route: PpscRoute,
        key: &str,
        value: i64,
    ) {
        let tenant_id = self.tenant(tenant).clone();
        let document_id =
            DocumentId::from_key(format!("ppsc-{:016x}-{step:03}-{key}", self.scenario_seed))
                .expect("scenario document id should parse");
        let fields = ppsc_fields(key, value);
        match route {
            PpscRoute::QueuedJournal => {
                self.engine
                    .insert_document_async_with_id(tenant_id, tasks_table(), document_id, fields)
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

    fn observe_published_prefixes(&self, step: usize) {
        for tenant_id in self.tenants.values() {
            let stats = self
                .engine
                .mutation_journal_stats_for_testing(tenant_id)
                .expect("scenario publication frontier should load");
            self.publications
                .observe_published_prefix(tenant_id, stats.published_head, step);
        }
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
            let mut observer_sequences = BTreeSet::new();
            for (sequence, _) in self.publications.observer_for_tenant(tenant_id) {
                assert!(
                    identities.contains_key(&sequence),
                    "observer publication {sequence} must identify a durable record"
                );
                assert!(
                    observer_sequences.insert(sequence),
                    "observer publication {sequence} must be delivered at most once"
                );
            }
            let publications = self
                .publications
                .published_for_tenant(tenant_id)
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
            let mut schema = self
                .engine
                .get_schema_async(tenant_id.clone())
                .await
                .expect("terminal schema should read")
                .tables
                .into_values()
                .collect::<Vec<_>>();
            schema.sort_by(|left, right| left.table.as_str().cmp(right.table.as_str()));
            let schema =
                serde_json::to_vec(&schema).expect("terminal schema should serialize canonically");
            let mut scheduled_jobs = self
                .engine
                .list_scheduled_jobs_async(tenant_id.clone())
                .await
                .expect("terminal scheduled jobs should read")
                .into_iter()
                .map(|job| {
                    serde_json::to_vec(&job)
                        .expect("terminal scheduled job should serialize canonically")
                })
                .collect::<Vec<_>>();
            scheduled_jobs.sort();
            let trigger_cursor = self
                .engine
                .trigger_delivery_cursor_for_testing(tenant_id)
                .expect("terminal trigger cursor should read")
                .materialized_through
                .0;
            let projection_durable_sequence = self
                .engine
                .projection_token_for_tenant_async(tenant_id)
                .await
                .expect("terminal projection token should read")
                .durable_sequence
                .0;
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
                    schema,
                    scheduled_jobs,
                    trigger_cursor,
                    projection_durable_sequence,
                },
            );
        }
        PpscTerminalState { tenants }
    }
}

fn ppsc_schema(table: TableName, revision: u64) -> TableSchema {
    TableSchema {
        table,
        fields: vec![FieldSchema {
            name: format!("revision_{revision:016x}"),
            field_type: FieldType::Any,
            required: false,
        }],
        indexes: Vec::new(),
        access_policy: None,
    }
}

fn ppsc_fields(key: &str, value: i64) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([
        ("key".to_string(), json!(key)),
        ("value".to_string(), json!(value)),
    ])
}

const fn routes_for_order(order: PpscCommitOrder) -> [PpscRoute; 3] {
    match order {
        PpscCommitOrder::QueuedDirectExecutionUnit => [
            PpscRoute::QueuedJournal,
            PpscRoute::Direct,
            PpscRoute::ExecutionUnit,
        ],
        PpscCommitOrder::QueuedExecutionUnitDirect => [
            PpscRoute::QueuedJournal,
            PpscRoute::ExecutionUnit,
            PpscRoute::Direct,
        ],
        PpscCommitOrder::DirectQueuedExecutionUnit => [
            PpscRoute::Direct,
            PpscRoute::QueuedJournal,
            PpscRoute::ExecutionUnit,
        ],
        PpscCommitOrder::DirectExecutionUnitQueued => [
            PpscRoute::Direct,
            PpscRoute::ExecutionUnit,
            PpscRoute::QueuedJournal,
        ],
        PpscCommitOrder::ExecutionUnitQueuedDirect => [
            PpscRoute::ExecutionUnit,
            PpscRoute::QueuedJournal,
            PpscRoute::Direct,
        ],
        PpscCommitOrder::ExecutionUnitDirectQueued => [
            PpscRoute::ExecutionUnit,
            PpscRoute::Direct,
            PpscRoute::QueuedJournal,
        ],
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

fn internal_durable_jobs_scenario() -> PpscScenario {
    let tenant = "ppsc-internal".to_string();
    let restore_tenant = "ppsc-restore".to_string();
    PpscScenario::new(
        "internal-durable-jobs",
        409,
        vec![
            PpscStep::new(
                PpscOperation::SchemaSet {
                    tenant: tenant.clone(),
                    revision: 41,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::Schedule {
                    tenant: tenant.clone(),
                    job: 42,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::TriggerCursorAdvance {
                    tenant: tenant.clone(),
                    through: 1,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::ProjectionUpdate {
                    tenant: tenant.clone(),
                    revision: 43,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::SchemaDelete { tenant },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::RestoreImport {
                    tenant: restore_tenant,
                    archive: 44,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(PpscOperation::Quiesce, PpscExpectedOutcome::Shutdown),
        ],
    )
    .expect("internal durable-jobs scenario should build")
}

fn mutation_edge_scenario(order: PpscCommitOrder) -> PpscScenario {
    let tenant = "ppsc-mutation-edges".to_string();
    PpscScenario::new(
        format!("mutation-edges-{order:?}"),
        419,
        vec![
            PpscStep::new(
                PpscOperation::CommitPermutation {
                    tenant: tenant.clone(),
                    order,
                    value_base: 100,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(
                PpscOperation::ZeroWriteExecutionUnit {
                    tenant: tenant.clone(),
                },
                PpscExpectedOutcome::Observed,
            ),
            PpscStep::new(
                PpscOperation::ConflictRetry {
                    tenant,
                    key: "shared".to_string(),
                    first: 201,
                    second: 202,
                },
                PpscExpectedOutcome::Committed,
            ),
            PpscStep::new(PpscOperation::Quiesce, PpscExpectedOutcome::Shutdown),
        ],
    )
    .expect("mutation-edge scenario should build")
}

fn logical_ppsc_documents(state: &PpscTenantState) -> BTreeMap<String, serde_json::Value> {
    state
        .documents
        .iter()
        .map(|(document_id, bytes)| {
            let document = serde_json::from_slice::<serde_json::Value>(bytes)
                .expect("PPSC terminal document should deserialize");
            (document_id.clone(), document["fields"].clone())
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn ppsc_engine_runner_exercises_three_production_commit_paths() {
    let scenario = three_route_scenario();
    let mut terminal_states = Vec::new();
    for backend in [PpscBackend::Memory, PpscBackend::Redb, PpscBackend::Sqlite] {
        let history = PpscEngineRunner::new_embedded(backend, &scenario)
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
    let first = PpscEngineRunner::new_embedded(PpscBackend::Redb, &scenario)
        .await
        .run(scenario.clone())
        .await;
    let replay = PpscEngineRunner::new_embedded(PpscBackend::Redb, &scenario)
        .await
        .run(scenario)
        .await;
    assert_eq!(first.canonical_bytes(), replay.canonical_bytes());
}

#[tokio::test(flavor = "multi_thread")]
async fn ppsc_engine_runner_internal_durable_jobs_match_embedded_backends() {
    let scenario = internal_durable_jobs_scenario();
    let mut terminal_states = Vec::new();
    for backend in [PpscBackend::Memory, PpscBackend::Redb, PpscBackend::Sqlite] {
        let history = PpscEngineRunner::new_embedded(backend, &scenario)
            .await
            .run(scenario.clone())
            .await;
        audit_ppsc_history(&history).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(history.observed_steps.len(), scenario.steps.len());
        assert!(
            history.observed_steps[1].effects.is_empty(),
            "scheduler persistence is serial and durable but does not consume a journal sequence"
        );
        let state = &history.terminal.tenants["ppsc-internal"];
        assert_eq!(state.frontiers.assigned_high_water, 4);
        assert_eq!(state.frontiers.active_assigned_head, 4);
        assert_eq!(state.frontiers.durable_head, 4);
        assert_eq!(state.frontiers.storage_applied_head, 4);
        assert_eq!(state.frontiers.published_head, 4);
        assert_eq!(state.frontiers.applied_head, 4);
        assert_eq!(state.journal.len(), 4);
        assert_eq!(state.publications.len(), 4);
        assert_eq!(state.scheduled_jobs.len(), 1);
        assert_eq!(state.trigger_cursor, 1);
        assert_eq!(state.projection_durable_sequence, 4);
        assert_eq!(
            state.schema,
            serde_json::to_vec(&vec![ppsc_schema(
                TableName::new("ppsc_projection").expect("PPSC projection table should parse"),
                43,
            )])
            .expect("expected PPSC terminal schema should serialize")
        );
        let restored = &history.terminal.tenants["ppsc-restore"];
        assert_eq!(restored.frontiers.assigned_high_water, 1);
        assert_eq!(restored.frontiers.active_assigned_head, 1);
        assert_eq!(restored.frontiers.durable_head, 1);
        assert_eq!(restored.frontiers.storage_applied_head, 1);
        assert_eq!(restored.frontiers.published_head, 1);
        assert_eq!(restored.frontiers.applied_head, 1);
        assert_eq!(restored.journal.len(), 1);
        assert_eq!(restored.publications.len(), 1);
        assert_eq!(restored.documents.len(), 1);
        assert_eq!(restored.schema, b"[]");
        assert!(restored.scheduled_jobs.is_empty());
        assert_eq!(restored.trigger_cursor, 0);
        assert_eq!(restored.projection_durable_sequence, 1);
        let restored_document = serde_json::from_slice::<serde_json::Value>(
            restored
                .documents
                .values()
                .next()
                .expect("PPSC restored document should exist"),
        )
        .expect("PPSC restored document should deserialize");
        assert_eq!(restored_document["fields"]["archive"], json!(44));
        terminal_states.push(history.terminal);
    }
    assert_eq!(terminal_states[0], terminal_states[1]);
    assert_eq!(terminal_states[1], terminal_states[2]);
}

#[tokio::test(flavor = "multi_thread")]
async fn ppsc_commit_order_permutations_preserve_terminal_state() {
    const ORDERS: [PpscCommitOrder; 6] = [
        PpscCommitOrder::QueuedDirectExecutionUnit,
        PpscCommitOrder::QueuedExecutionUnitDirect,
        PpscCommitOrder::DirectQueuedExecutionUnit,
        PpscCommitOrder::DirectExecutionUnitQueued,
        PpscCommitOrder::ExecutionUnitQueuedDirect,
        PpscCommitOrder::ExecutionUnitDirectQueued,
    ];

    let mut expected_documents = None;
    for order in ORDERS {
        let scenario = mutation_edge_scenario(order);
        let mut backend_terminals = Vec::new();
        for backend in [PpscBackend::Memory, PpscBackend::Redb, PpscBackend::Sqlite] {
            let history = PpscEngineRunner::new_embedded(backend, &scenario)
                .await
                .run(scenario.clone())
                .await;
            audit_ppsc_history(&history).unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(history.observed_steps[0].effects.len(), 3);
            assert!(history.observed_steps[1].effects.is_empty());
            assert_eq!(history.observed_steps[2].effects.len(), 2);
            let state = &history.terminal.tenants["ppsc-mutation-edges"];
            assert_eq!(state.frontiers.assigned_high_water, 5);
            assert_eq!(state.frontiers.active_assigned_head, 5);
            assert_eq!(state.frontiers.durable_head, 5);
            assert_eq!(state.frontiers.storage_applied_head, 5);
            assert_eq!(state.frontiers.published_head, 5);
            assert_eq!(state.frontiers.applied_head, 5);
            assert_eq!(state.journal.len(), 5);
            assert_eq!(state.publications.len(), 5);
            assert_eq!(state.documents.len(), 4);
            let logical_documents = logical_ppsc_documents(state);
            match &expected_documents {
                Some(expected) => assert_eq!(&logical_documents, expected),
                None => expected_documents = Some(logical_documents),
            }
            backend_terminals.push(history.terminal);
        }
        assert_eq!(backend_terminals[0], backend_terminals[1]);
        assert_eq!(backend_terminals[1], backend_terminals[2]);
    }
}

pub(crate) async fn exercise_ppsc_provider_three_route_differential(
    backend: PpscBackend,
    config: EnginePersistenceConfig,
) {
    let scenario = three_route_scenario();
    let oracle = PpscEngineRunner::new_embedded(PpscBackend::Redb, &scenario)
        .await
        .run(scenario.clone())
        .await;
    let provider = PpscEngineRunner::new_configured_provider(backend, &scenario, config)
        .await
        .run(scenario)
        .await;

    audit_ppsc_history(&oracle).unwrap_or_else(|error| panic!("redb oracle: {error}"));
    audit_ppsc_history(&provider)
        .unwrap_or_else(|error| panic!("{} provider: {error}", backend.as_str()));
    assert_eq!(
        provider.terminal,
        oracle.terminal,
        "{} should match the redb terminal state and canonical journal bytes",
        backend.as_str()
    );
}
