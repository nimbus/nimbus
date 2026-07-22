use std::collections::BTreeMap;

use nimbus_core::{
    DocumentId, FieldSchema, FieldType, IdSource, ManualMonotonicClock, Mutation, ScheduleRequest,
    SeededIdSource, TenantEventKind, TriggerDeliveryCursor, hex_encode,
};
use nimbus_storage::PointInTimeRestoreArchive;
use nimbus_storage::provider_test_fixtures::ProviderLeaseTimeControl;
use nimbus_testing::ppsc::{
    PpscBackend, PpscCommitOrder, PpscEffect, PpscExpectedOutcome, PpscFrontiers, PpscHistory,
    PpscInjectedFault, PpscJournalEntry, PpscObservedStep, PpscOperation, PpscPublication,
    PpscRoute, PpscScenario, PpscSequenceClaim, PpscSequenceOwnership, PpscStep,
    PpscStorageFaultInjector, PpscTenantState, PpscTerminalState, audit_ppsc_history,
    retained_ppsc_scenarios, retained_provider_authority_scenarios,
};

use super::*;

mod commit_faults;
mod harness;
mod pressure;
mod scenarios;

use harness::*;
use scenarios::*;

struct PpscEngineRunner {
    backend: PpscBackend,
    _data_dir: Option<TempDir>,
    engine: PpscEngineSlot,
    engine_factory: PpscEngineFactory,
    wall_clock: Arc<ManualWallClock>,
    monotonic_clock: Arc<ManualMonotonicClock>,
    storage_faults: Arc<PpscStorageFaultInjector>,
    id_source: Arc<dyn IdSource>,
    publications: Arc<PpscPublicationRecorder>,
    tenants: BTreeMap<String, TenantId>,
    publisher_limited_tenants: BTreeSet<TenantId>,
    restore_archives: BTreeMap<u64, PointInTimeRestoreArchive>,
    takeover_engine_factory: Option<PpscEngineFactory>,
    provider_lease_time_control: Option<Arc<dyn ProviderLeaseTimeControl>>,
    stale_engine: Option<Arc<Engine>>,
    crashed_heads: Option<BTreeMap<String, u64>>,
    crashed_runtime_identities: Option<BTreeMap<String, u64>>,
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
        let id_source: Arc<dyn IdSource> = Arc::new(SeededIdSource::new(scenario.seed));
        let engine_factory = match backend {
            PpscBackend::Memory => PpscEngineFactory::Memory(data_dir.path().to_path_buf()),
            PpscBackend::Redb => PpscEngineFactory::Embedded(
                data_dir.path().to_path_buf(),
                EmbeddedProviderKind::Redb,
            ),
            PpscBackend::Sqlite => PpscEngineFactory::Embedded(
                data_dir.path().to_path_buf(),
                EmbeddedProviderKind::Sqlite,
            ),
            _ => unreachable!("provider backends were rejected above"),
        };
        let engine = engine_factory
            .build(
                wall_clock.clone(),
                monotonic_clock.clone(),
                storage_faults.clone(),
                id_source.clone(),
            )
            .await;
        Self::finish_new(
            backend,
            scenario,
            PpscEngineBootstrap {
                data_dir: Some(data_dir),
                engine,
                engine_factory,
                wall_clock,
                monotonic_clock,
                storage_faults,
                id_source,
            },
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
        let id_source: Arc<dyn IdSource> = Arc::new(SeededIdSource::new(scenario.seed));
        let engine_factory = PpscEngineFactory::Configured(Box::new(config));
        let engine = engine_factory
            .build(
                wall_clock.clone(),
                monotonic_clock.clone(),
                storage_faults.clone(),
                id_source.clone(),
            )
            .await;
        Self::finish_new(
            backend,
            scenario,
            PpscEngineBootstrap {
                data_dir: None,
                engine,
                engine_factory,
                wall_clock,
                monotonic_clock,
                storage_faults,
                id_source,
            },
        )
        .await
    }

    async fn finish_new(
        backend: PpscBackend,
        scenario: &PpscScenario,
        bootstrap: PpscEngineBootstrap,
    ) -> Self {
        let PpscEngineBootstrap {
            data_dir,
            engine,
            engine_factory,
            wall_clock,
            monotonic_clock,
            storage_faults,
            id_source,
        } = bootstrap;
        let publications = Arc::new(PpscPublicationRecorder::default());
        engine.install_committed_mutation_observer(PPSC_OBSERVER, publications.clone());

        let tenant_names = scenario
            .steps
            .iter()
            .filter_map(|step| step.operation.tenant())
            .collect::<BTreeSet<_>>();
        let mut tenants = BTreeMap::new();
        let mut publisher_limited_tenants = BTreeSet::new();
        for tenant_name in tenant_names {
            let tenant_id = TenantId::new(tenant_name).expect("scenario tenant id should parse");
            if scenario.steps.iter().any(|step| {
                matches!(
                    &step.operation,
                    PpscOperation::ForceOverload { tenant } if tenant == tenant_name
                )
            }) {
                publisher_limited_tenants.insert(tenant_id.clone());
                crate::tenant::configure_publisher_limits_for_testing(
                    tenant_id.clone(),
                    1,
                    Duration::from_millis(25),
                );
            }
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
            engine: PpscEngineSlot::new(engine),
            engine_factory,
            wall_clock,
            monotonic_clock,
            storage_faults,
            id_source,
            publications,
            tenants,
            publisher_limited_tenants,
            restore_archives,
            takeover_engine_factory: None,
            provider_lease_time_control: None,
            stale_engine: None,
            crashed_heads: None,
            crashed_runtime_identities: None,
            scenario_seed: scenario.seed,
        }
    }

    async fn run(mut self, scenario: PpscScenario) -> PpscHistory {
        let mut observed_steps = Vec::with_capacity(scenario.steps.len());
        let mut sequence_claims = Vec::new();
        for (index, step) in scenario.steps.iter().enumerate() {
            self.publications.enter_step(index);
            let before = if self.engine.is_running() {
                self.journal_heads()
            } else {
                self.crashed_heads
                    .clone()
                    .expect("PPSC crashed Engine must retain its durable heads")
            };
            let outcome = self
                .execute_step(index, &step.operation, step.expected)
                .await;
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
            if self.engine.is_running() {
                self.observe_published_prefixes(index);
            }
            let effects = if self.engine.is_running() {
                self.new_effects(&before)
            } else {
                Vec::new()
            };
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
        if let Some(stale_engine) = self.stale_engine.take() {
            stale_engine.quiesce().await;
        }
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
        expected: PpscExpectedOutcome,
    ) -> PpscExpectedOutcome {
        match operation {
            PpscOperation::Mutation {
                tenant,
                route,
                key,
                value,
            } => {
                let tenant_id = self.tenant(tenant).clone();
                let runtime_identity_before = self
                    .engine
                    .tenant_runtime_identity_for_testing(&tenant_id)
                    .expect("PPSC mutation runtime identity should load");
                let result = self
                    .commit_route_insert(index, tenant, *route, key, *value)
                    .await;
                match expected {
                    PpscExpectedOutcome::Committed => {
                        result.expect("PPSC mutation should commit");
                        PpscExpectedOutcome::Committed
                    }
                    PpscExpectedOutcome::AmbiguousRecovered => {
                        result.expect(
                            "PPSC ordered publisher must reconcile acknowledgement loss as committed",
                        );
                        let document_id = ppsc_document_id(self.scenario_seed, index, key);
                        let document = self
                            .engine
                            .get_document_async(tenant_id.clone(), tasks_table(), document_id)
                            .await
                            .expect("PPSC ambiguous mutation should recover from durable state");
                        assert_eq!(document.fields.get("value"), Some(&json!(value)));
                        assert_eq!(
                            self.engine
                                .tenant_runtime_identity_for_testing(&tenant_id)
                                .expect("PPSC reconciled runtime identity should load"),
                            runtime_identity_before,
                            "a resolved acknowledgement loss must retain the live runtime"
                        );
                        let fault = self
                            .storage_faults
                            .snapshot(&tenant_id, PpscInjectedFault::AcknowledgementLoss)
                            .expect("PPSC acknowledgement-loss snapshot should read");
                        assert!(!fault.active);
                        assert_eq!(fault.fires, 1);
                        PpscExpectedOutcome::AmbiguousRecovered
                    }
                    PpscExpectedOutcome::ProviderError => {
                        let error = result.expect_err("PPSC provider transient must fail");
                        assert_eq!(
                            error.storage_kind(),
                            Some(nimbus_core::StorageErrorKind::Transient),
                            "PPSC provider error must preserve the transient storage class"
                        );
                        assert_eq!(
                            self.engine
                                .tenant_runtime_identity_for_testing(&tenant_id)
                                .expect("PPSC definitive-error runtime identity should load"),
                            runtime_identity_before,
                            "a before-visibility provider error must retain the live runtime"
                        );
                        let fault = self
                            .storage_faults
                            .snapshot(&tenant_id, PpscInjectedFault::ProviderTransient)
                            .expect("PPSC provider-transient snapshot should read");
                        assert!(fault.active);
                        assert!(fault.fires >= 1);
                        PpscExpectedOutcome::ProviderError
                    }
                    other => panic!(
                        "PPSC mutation route {route:?} does not implement expected outcome {other:?}"
                    ),
                }
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
                    .await
                    .expect("PPSC permutation mutation should commit");
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
            PpscOperation::CommitPhaseFault {
                tenant,
                route,
                fault,
            } => {
                self.exercise_commit_phase_fault(index, tenant, *route, *fault)
                    .await
            }
            PpscOperation::PublicationPredecessorRace {
                tenant,
                predecessor_route,
                successor_route,
            } => {
                self.exercise_publication_predecessor_race(
                    index,
                    tenant,
                    *predecessor_route,
                    *successor_route,
                )
                .await
            }
            PpscOperation::ArmFault { tenant, fault } => {
                let tenant_id = self.tenant(tenant).clone();
                self.storage_faults
                    .arm(tenant_id, *fault)
                    .unwrap_or_else(|error| {
                        panic!("PPSC fault '{}' should arm: {error}", fault.as_str())
                    });
                PpscExpectedOutcome::Observed
            }
            PpscOperation::ReleaseFault { tenant, fault } => {
                let tenant_id = self.tenant(tenant);
                self.storage_faults
                    .release(tenant_id, *fault)
                    .unwrap_or_else(|error| {
                        panic!("PPSC fault '{}' should release: {error}", fault.as_str())
                    });
                let snapshot = self
                    .storage_faults
                    .snapshot(tenant_id, *fault)
                    .expect("PPSC released fault snapshot should read");
                assert!(!snapshot.active);
                assert!(snapshot.fires >= 1);
                PpscExpectedOutcome::Observed
            }
            PpscOperation::CancelNext { tenant, route } => {
                match route {
                    PpscRoute::QueuedJournal => self.cancel_queued_insert(index, tenant).await,
                    PpscRoute::ExecutionUnit => self.cancel_execution_unit_admission(tenant).await,
                    PpscRoute::Direct => {
                        unreachable!("scenario validation rejects synchronous direct cancellation")
                    }
                }
                PpscExpectedOutcome::Cancelled
            }
            PpscOperation::ForceOverload { tenant } => {
                self.force_publisher_overload(tenant).await;
                PpscExpectedOutcome::Overloaded
            }
            PpscOperation::AdvanceWallClock { millis } => {
                self.wall_clock.advance_ms(*millis);
                PpscExpectedOutcome::Observed
            }
            PpscOperation::AdvanceMonotonicClock { millis } => {
                self.monotonic_clock.advance(Duration::from_millis(*millis));
                PpscExpectedOutcome::Observed
            }
            PpscOperation::ExpireProviderLease { tenant } => {
                assert_eq!(expected, PpscExpectedOutcome::Observed);
                let control = self
                    .provider_lease_time_control
                    .as_ref()
                    .expect("provider-authority scenario requires lease-time control");
                assert_eq!(
                    control.provider_name(),
                    self.backend.as_str(),
                    "provider lease-time adapter must match the scenario backend"
                );
                control
                    .expire_lease(self.tenant(tenant))
                    .await
                    .expect("provider-owned committer lease should expire");
                PpscExpectedOutcome::Observed
            }
            PpscOperation::ProviderTakeover { tenant } => {
                assert_eq!(expected, PpscExpectedOutcome::Committed);
                self.take_over_provider_tenant(index, tenant).await;
                PpscExpectedOutcome::Committed
            }
            PpscOperation::AttemptStaleProviderWrite { tenant } => {
                assert_eq!(expected, PpscExpectedOutcome::Fenced);
                let stale_engine = self
                    .stale_engine
                    .as_ref()
                    .expect("stale-writer step requires a completed provider takeover")
                    .clone();
                let error = self
                    .commit_route_insert_with_engine(
                        &stale_engine,
                        index,
                        tenant,
                        PpscRoute::QueuedJournal,
                        "provider-stale-writer",
                        provider_takeover_value(self.scenario_seed).saturating_add(1),
                    )
                    .await
                    .expect_err("stale provider writer must be fenced");
                assert!(
                    matches!(error, Error::CommitterFenced { .. }),
                    "stale provider write must retain the typed fence error: {error}"
                );
                assert_eq!(error.retryability(), nimbus_core::Retryability::Terminal);
                assert_ne!(
                    error.commit_class(),
                    Some(nimbus_core::CommitErrorClass::Conflict),
                    "provider fencing is not an OCC conflict"
                );
                PpscExpectedOutcome::Fenced
            }
            PpscOperation::Crash => {
                assert!(self.backend.capabilities().durable_reopen);
                self.crashed_heads = Some(self.journal_heads());
                self.crashed_runtime_identities = Some(
                    self.tenants
                        .iter()
                        .map(|(name, tenant_id)| {
                            (
                                name.clone(),
                                self.engine
                                    .tenant_runtime_identity_for_testing(tenant_id)
                                    .expect("PPSC pre-crash runtime identity should load"),
                            )
                        })
                        .collect(),
                );
                self.engine.crash();
                PpscExpectedOutcome::Observed
            }
            PpscOperation::Reopen => {
                self.reopen_engine().await;
                PpscExpectedOutcome::Observed
            }
            PpscOperation::Quiesce => PpscExpectedOutcome::Shutdown,
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
    ) -> nimbus_core::Result<()> {
        self.commit_route_insert_with_engine(
            &self.engine.current(),
            step,
            tenant,
            route,
            key,
            value,
        )
        .await
    }

    async fn commit_route_insert_with_engine(
        &self,
        engine: &Arc<Engine>,
        step: usize,
        tenant: &str,
        route: PpscRoute,
        key: &str,
        value: i64,
    ) -> nimbus_core::Result<()> {
        let tenant_id = self.tenant(tenant).clone();
        let document_id = ppsc_document_id(self.scenario_seed, step, key);
        let fields = ppsc_fields(key, value);
        match route {
            PpscRoute::QueuedJournal => {
                engine
                    .insert_document_async_with_id(tenant_id, tasks_table(), document_id, fields)
                    .await?;
            }
            PpscRoute::Direct => {
                engine.insert_document_with_id(&tenant_id, tasks_table(), document_id, fields)?;
            }
            PpscRoute::ExecutionUnit => {
                let unit = engine
                    .begin_mutation_execution_unit(tenant_id, PrincipalContext::anonymous())?;
                unit.insert_document_with_id(tasks_table(), Some(document_id), fields)?;
                unit.commit()?
                    .expect("PPSC execution unit should emit a durable record");
            }
        }
        Ok(())
    }

    async fn take_over_provider_tenant(&mut self, step: usize, tenant: &str) {
        assert!(
            self.backend.capabilities().provider_authority,
            "provider takeover requires provider sequence authority"
        );
        assert!(
            self.stale_engine.is_none(),
            "one bounded PPSC scenario may perform at most one provider takeover"
        );
        let factory = self
            .takeover_engine_factory
            .take()
            .expect("provider takeover requires an independent Engine factory");
        let takeover_engine = factory
            .build(
                self.wall_clock.clone(),
                self.monotonic_clock.clone(),
                self.storage_faults.clone(),
                self.id_source.clone(),
            )
            .await;
        takeover_engine
            .install_committed_mutation_observer(PPSC_OBSERVER, self.publications.clone());
        for tenant_id in self.tenants.values() {
            takeover_engine
                .ensure_tenant_ready_async(tenant_id.clone())
                .await
                .expect("takeover Engine should open every scenario tenant");
            takeover_engine
                .shutdown_trigger_candidates_for_testing(tenant_id)
                .expect("takeover Engine trigger worker should stop before continuation");
            takeover_engine
                .flush_tenant_committer_for_testing(tenant_id)
                .await
                .expect("takeover Engine should reconcile before continuation");
        }

        let stale_engine = self.engine.replace(takeover_engine);
        self.engine_factory = factory;
        self.stale_engine = Some(stale_engine);
        self.commit_route_insert(
            step,
            tenant,
            PpscRoute::QueuedJournal,
            "provider-takeover",
            provider_takeover_value(self.scenario_seed),
        )
        .await
        .expect("successor provider Engine should acquire the expired lease and commit");
    }

    async fn reopen_engine(&mut self) {
        assert!(
            !self.engine.is_running(),
            "PPSC reopen requires a prior crash"
        );
        let expected_heads = self
            .crashed_heads
            .take()
            .expect("PPSC reopen requires recorded durable crash heads");
        let crashed_runtime_identities = self
            .crashed_runtime_identities
            .take()
            .expect("PPSC reopen requires recorded runtime identities");
        let engine = self
            .engine_factory
            .build(
                self.wall_clock.clone(),
                self.monotonic_clock.clone(),
                self.storage_faults.clone(),
                self.id_source.clone(),
            )
            .await;
        engine.install_committed_mutation_observer(PPSC_OBSERVER, self.publications.clone());
        for tenant_id in self.tenants.values() {
            if self.publisher_limited_tenants.contains(tenant_id) {
                crate::tenant::configure_publisher_limits_for_testing(
                    tenant_id.clone(),
                    1,
                    Duration::from_millis(25),
                );
            }
            engine
                .ensure_tenant_ready_async(tenant_id.clone())
                .await
                .expect("PPSC existing tenant should reopen through async admission");
            engine
                .shutdown_trigger_candidates_for_testing(tenant_id)
                .expect("PPSC reopened trigger worker should stop before continuation");
            engine
                .flush_tenant_committer_for_testing(tenant_id)
                .await
                .expect("PPSC reopened tenant should reconcile before continuation");
        }
        self.engine.reopen(engine);
        assert_eq!(
            self.journal_heads(),
            expected_heads,
            "PPSC reopen must retain the exact durable prefix"
        );
        for (name, tenant_id) in &self.tenants {
            assert_ne!(
                self.engine
                    .tenant_runtime_identity_for_testing(tenant_id)
                    .expect("PPSC reopened runtime identity should load"),
                crashed_runtime_identities[name],
                "PPSC reopen must construct a new runtime for tenant {name}"
            );
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

fn ppsc_document_id(seed: u64, step: usize, key: &str) -> DocumentId {
    DocumentId::from_key(format!("ppsc-{seed:016x}-{step:03}-{key}"))
        .expect("scenario document id should parse")
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ppsc_storage_faults_recover_without_cross_tenant_effects() {
    let scenario = storage_fault_scenario();
    let mut terminal_states = Vec::new();
    for backend in [PpscBackend::Memory, PpscBackend::Redb, PpscBackend::Sqlite] {
        let history = PpscEngineRunner::new_embedded(backend, &scenario)
            .await
            .run(scenario.clone())
            .await;
        audit_ppsc_history(&history).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(history.observed_steps[1].effects.len(), 1);
        assert!(history.observed_steps[3].effects.is_empty());
        let hot = &history.terminal.tenants["ppsc-fault-hot"];
        assert_eq!(hot.frontiers.durable_head, 2);
        assert_eq!(hot.frontiers.storage_applied_head, 2);
        assert_eq!(hot.frontiers.published_head, 2);
        assert_eq!(hot.frontiers.applied_head, 2);
        assert_eq!(hot.journal.len(), 2);
        assert_eq!(hot.publications.len(), 2);
        assert_eq!(hot.documents.len(), 2);
        let peer = &history.terminal.tenants["ppsc-fault-peer"];
        assert_eq!(peer.frontiers.durable_head, 1);
        assert_eq!(peer.frontiers.storage_applied_head, 1);
        assert_eq!(peer.frontiers.published_head, 1);
        assert_eq!(peer.frontiers.applied_head, 1);
        assert_eq!(peer.journal.len(), 1);
        assert_eq!(peer.publications.len(), 1);
        assert_eq!(peer.documents.len(), 1);
        terminal_states.push(history.terminal);
    }
    assert_eq!(terminal_states[0], terminal_states[1]);
    assert_eq!(terminal_states[1], terminal_states[2]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ppsc_hot_tenant_failure_preserves_other_tenant_progress() {
    let scenario = cancellation_overload_scenario();
    let mut terminal_states = Vec::new();
    for backend in [PpscBackend::Memory, PpscBackend::Redb, PpscBackend::Sqlite] {
        let history = PpscEngineRunner::new_embedded(backend, &scenario)
            .await
            .run(scenario.clone())
            .await;
        audit_ppsc_history(&history).unwrap_or_else(|error| panic!("{error}"));
        assert!(history.observed_steps[0].effects.is_empty());
        assert!(history.observed_steps[1].effects.is_empty());
        assert!(history.observed_steps[2].effects.is_empty());
        for tenant in ["ppsc-pressure-hot", "ppsc-pressure-peer"] {
            let state = &history.terminal.tenants[tenant];
            assert_eq!(state.frontiers.assigned_high_water, 1);
            assert_eq!(state.frontiers.active_assigned_head, 1);
            assert_eq!(state.frontiers.durable_head, 1);
            assert_eq!(state.frontiers.storage_applied_head, 1);
            assert_eq!(state.frontiers.published_head, 1);
            assert_eq!(state.frontiers.applied_head, 1);
            assert_eq!(state.journal.len(), 1);
            assert_eq!(state.publications.len(), 1);
            assert_eq!(state.documents.len(), 1);
        }
        terminal_states.push(history.terminal);
    }
    assert_eq!(terminal_states[0], terminal_states[1]);
    assert_eq!(terminal_states[1], terminal_states[2]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ppsc_commit_phase_faults_preserve_prefix_and_recover() {
    let scenario = commit_phase_fault_scenario();
    let mut terminal_states = Vec::new();
    for backend in [PpscBackend::Memory, PpscBackend::Redb, PpscBackend::Sqlite] {
        let history = PpscEngineRunner::new_embedded(backend, &scenario)
            .await
            .run(scenario.clone())
            .await;
        audit_ppsc_history(&history).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(history.observed_steps[0].effects.len(), 2);
        assert_eq!(history.observed_steps[1].effects.len(), 1);
        assert_eq!(history.observed_steps[2].effects.len(), 1);
        let hot = &history.terminal.tenants["ppsc-commit-phase-hot"];
        assert_eq!(hot.frontiers.assigned_high_water, 5);
        assert_eq!(hot.frontiers.active_assigned_head, 5);
        assert_eq!(hot.frontiers.durable_head, 5);
        assert_eq!(hot.frontiers.storage_applied_head, 5);
        assert_eq!(hot.frontiers.published_head, 5);
        assert_eq!(hot.frontiers.applied_head, 5);
        assert_eq!(hot.journal.len(), 5);
        assert_eq!(hot.publications.len(), 5);
        assert_eq!(hot.documents.len(), 5);
        let peer = &history.terminal.tenants["ppsc-commit-phase-peer"];
        assert_eq!(peer.frontiers.durable_head, 1);
        assert_eq!(peer.frontiers.storage_applied_head, 1);
        assert_eq!(peer.frontiers.published_head, 1);
        assert_eq!(peer.frontiers.applied_head, 1);
        assert_eq!(peer.journal.len(), 1);
        assert_eq!(peer.publications.len(), 1);
        assert_eq!(peer.documents.len(), 1);
        terminal_states.push(history.terminal);
    }
    assert_eq!(terminal_states[0], terminal_states[1]);
    assert_eq!(terminal_states[1], terminal_states[2]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ppsc_durable_crash_reopen_preserves_prefix_and_continues() {
    let scenario = crash_reopen_scenario();
    let mut terminal_states = Vec::new();
    for backend in [PpscBackend::Redb, PpscBackend::Sqlite] {
        let history = PpscEngineRunner::new_embedded(backend, &scenario)
            .await
            .run(scenario.clone())
            .await;
        audit_ppsc_history(&history).unwrap_or_else(|error| panic!("{error}"));
        assert!(history.observed_steps[1].effects.is_empty());
        assert!(history.observed_steps[2].effects.is_empty());
        let state = &history.terminal.tenants["ppsc-reopen"];
        assert_eq!(state.frontiers.assigned_high_water, 2);
        assert_eq!(state.frontiers.active_assigned_head, 2);
        assert_eq!(state.frontiers.durable_head, 2);
        assert_eq!(state.frontiers.storage_applied_head, 2);
        assert_eq!(state.frontiers.published_head, 2);
        assert_eq!(state.frontiers.applied_head, 2);
        assert_eq!(state.journal.len(), 2);
        assert_eq!(state.publications.len(), 2);
        assert_eq!(state.documents.len(), 2);
        terminal_states.push(history.terminal);
    }
    assert_eq!(terminal_states[0], terminal_states[1]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn redb_ppsc_seeded_journal_differential() {
    for scenario in retained_ppsc_scenarios() {
        let first = PpscEngineRunner::new_embedded(PpscBackend::Redb, &scenario)
            .await
            .run(scenario.clone())
            .await;
        let replay = PpscEngineRunner::new_embedded(PpscBackend::Redb, &scenario)
            .await
            .run(scenario.clone())
            .await;
        audit_ppsc_history(&first).unwrap_or_else(|error| panic!("redb first run: {error}"));
        audit_ppsc_history(&replay).unwrap_or_else(|error| panic!("redb replay: {error}"));
        assert_ppsc_terminal_matches(
            PpscBackend::Redb,
            &scenario,
            &first.terminal,
            &replay.terminal,
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sqlite_ppsc_seeded_journal_differential() {
    for scenario in retained_ppsc_scenarios() {
        let oracle = PpscEngineRunner::new_embedded(PpscBackend::Redb, &scenario)
            .await
            .run(scenario.clone())
            .await;
        let sqlite = PpscEngineRunner::new_embedded(PpscBackend::Sqlite, &scenario)
            .await
            .run(scenario.clone())
            .await;
        audit_ppsc_history(&oracle).unwrap_or_else(|error| panic!("redb oracle: {error}"));
        audit_ppsc_history(&sqlite).unwrap_or_else(|error| panic!("sqlite: {error}"));
        assert_ppsc_terminal_matches(
            PpscBackend::Sqlite,
            &scenario,
            &oracle.terminal,
            &sqlite.terminal,
        );
    }
}

pub(crate) async fn exercise_ppsc_provider_retained_differential(
    backend: PpscBackend,
    config: EnginePersistenceConfig,
) {
    for scenario in retained_ppsc_scenarios() {
        let oracle = PpscEngineRunner::new_embedded(PpscBackend::Redb, &scenario)
            .await
            .run(scenario.clone())
            .await;
        let provider =
            PpscEngineRunner::new_configured_provider(backend, &scenario, config.clone())
                .await
                .run(scenario.clone())
                .await;

        audit_ppsc_history(&oracle).unwrap_or_else(|error| panic!("redb oracle: {error}"));
        audit_ppsc_history(&provider)
            .unwrap_or_else(|error| panic!("{} provider: {error}", backend.as_str()));
        assert_ppsc_terminal_matches(backend, &scenario, &oracle.terminal, &provider.terminal);
    }
}

pub(crate) async fn exercise_ppsc_provider_authority_extension(
    backend: PpscBackend,
    first_config: EnginePersistenceConfig,
    takeover_config: EnginePersistenceConfig,
    lease_time_control: Arc<dyn ProviderLeaseTimeControl>,
) {
    for scenario in retained_provider_authority_scenarios() {
        let oracle_scenario = provider_authority_terminal_oracle(&scenario);
        let oracle = PpscEngineRunner::new_embedded(PpscBackend::Redb, &oracle_scenario)
            .await
            .run(oracle_scenario)
            .await;
        let mut provider =
            PpscEngineRunner::new_configured_provider(backend, &scenario, first_config.clone())
                .await;
        provider.takeover_engine_factory = Some(PpscEngineFactory::Configured(Box::new(
            takeover_config.clone(),
        )));
        provider.provider_lease_time_control = Some(lease_time_control.clone());
        let provider = provider.run(scenario.clone()).await;

        audit_ppsc_history(&oracle)
            .unwrap_or_else(|error| panic!("provider terminal oracle: {error}"));
        audit_ppsc_history(&provider)
            .unwrap_or_else(|error| panic!("{} provider authority: {error}", backend.as_str()));
        assert_ppsc_terminal_matches(backend, &scenario, &oracle.terminal, &provider.terminal);
    }
}

fn provider_authority_terminal_oracle(scenario: &PpscScenario) -> PpscScenario {
    let steps = scenario
        .steps
        .iter()
        .map(|step| match &step.operation {
            PpscOperation::ExpireProviderLease { .. }
            | PpscOperation::AttemptStaleProviderWrite { .. } => PpscStep::new(
                PpscOperation::AdvanceMonotonicClock { millis: 0 },
                PpscExpectedOutcome::Observed,
            ),
            PpscOperation::ProviderTakeover { tenant } => PpscStep::new(
                PpscOperation::Mutation {
                    tenant: tenant.clone(),
                    route: PpscRoute::QueuedJournal,
                    key: "provider-takeover".to_string(),
                    value: provider_takeover_value(scenario.seed),
                },
                PpscExpectedOutcome::Committed,
            ),
            _ => step.clone(),
        })
        .collect();
    PpscScenario::new(
        format!("{}-terminal-oracle", scenario.name),
        scenario.seed,
        steps,
    )
    .expect("provider-authority terminal oracle should remain a valid bounded scenario")
}

fn provider_takeover_value(seed: u64) -> i64 {
    i64::try_from(seed).expect("retained PPSC seed should fit in i64")
}

fn assert_ppsc_terminal_matches(
    backend: PpscBackend,
    scenario: &PpscScenario,
    oracle: &PpscTerminalState,
    candidate: &PpscTerminalState,
) {
    if candidate == oracle {
        return;
    }
    let tenant_names = oracle
        .tenants
        .keys()
        .chain(candidate.tenants.keys())
        .collect::<BTreeSet<_>>();
    for tenant in tenant_names {
        let Some(expected) = oracle.tenants.get(tenant) else {
            panic!(
                "PPSC {} seed {} has unexpected tenant {tenant}; replay: {}",
                backend.as_str(),
                scenario.seed,
                scenario.replay_command(backend)
            );
        };
        let Some(actual) = candidate.tenants.get(tenant) else {
            panic!(
                "PPSC {} seed {} is missing tenant {tenant}; replay: {}",
                backend.as_str(),
                scenario.seed,
                scenario.replay_command(backend)
            );
        };
        if actual.frontiers != expected.frontiers {
            panic!(
                "PPSC {} seed {} tenant {tenant} frontier divergence: expected {:?}, actual {:?}; replay: {}",
                backend.as_str(),
                scenario.seed,
                expected.frontiers,
                actual.frontiers,
                scenario.replay_command(backend)
            );
        }
        if actual.journal.len() != expected.journal.len() {
            panic!(
                "PPSC {} seed {} tenant {tenant} journal length divergence: expected {}, actual {}; frontiers {:?}; replay: {}",
                backend.as_str(),
                scenario.seed,
                expected.journal.len(),
                actual.journal.len(),
                actual.frontiers,
                scenario.replay_command(backend)
            );
        }
        for (index, (expected_record, actual_record)) in expected
            .journal
            .iter()
            .zip(actual.journal.iter())
            .enumerate()
        {
            if expected_record == actual_record {
                continue;
            }
            let expected_kind = ppsc_record_kind(&expected_record.canonical_bytes);
            let actual_kind = ppsc_record_kind(&actual_record.canonical_bytes);
            let byte_offset = expected_record
                .canonical_bytes
                .iter()
                .zip(actual_record.canonical_bytes.iter())
                .position(|(expected, actual)| expected != actual)
                .unwrap_or_else(|| {
                    expected_record
                        .canonical_bytes
                        .len()
                        .min(actual_record.canonical_bytes.len())
                });
            panic!(
                "PPSC {} seed {} tenant {tenant} first journal divergence at record {index}, sequence expected {} actual {}, byte {byte_offset}, lengths expected {} actual {}, event kinds expected {expected_kind} actual {actual_kind}; frontiers {:?}; replay: {}",
                backend.as_str(),
                scenario.seed,
                expected_record.sequence,
                actual_record.sequence,
                expected_record.canonical_bytes.len(),
                actual_record.canonical_bytes.len(),
                actual.frontiers,
                scenario.replay_command(backend)
            );
        }
        for (field, matches) in [
            ("publications", actual.publications == expected.publications),
            ("documents", actual.documents == expected.documents),
            ("schema", actual.schema == expected.schema),
            (
                "scheduled-jobs",
                actual.scheduled_jobs == expected.scheduled_jobs,
            ),
            (
                "trigger-cursor",
                actual.trigger_cursor == expected.trigger_cursor,
            ),
            (
                "projection-durable-sequence",
                actual.projection_durable_sequence == expected.projection_durable_sequence,
            ),
        ] {
            if !matches {
                panic!(
                    "PPSC {} seed {} tenant {tenant} terminal field {field} diverged after identical journal/frontiers; replay: {}",
                    backend.as_str(),
                    scenario.seed,
                    scenario.replay_command(backend)
                );
            }
        }
    }
    unreachable!("a terminal-state difference must identify a tenant field")
}

fn ppsc_record_kind(bytes: &[u8]) -> String {
    nimbus_storage::commit_log::deserialize_tenant_event_record(bytes)
        .map(|record| record_kind(record.events()))
        .unwrap_or_else(|error| format!("invalid-record:{error}"))
}
