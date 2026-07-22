use std::env;
use std::future::Future;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use libsql::{Builder, Database};
use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, CollectionName, DocumentId, DocumentLocator, DocumentPath,
    FieldReference, FieldSchema, FieldType, IndexDefinition, Mutation, PrincipalContext,
    QueryDirection, ResourcePathBinding, ScheduleRequest, ScheduledJobOutcome, StructuredCursor,
    StructuredOrder, StructuredQuery, TableId, TableName, TableSchema, TenantId, Timestamp,
    WriteKey, WritePrecondition, WriteSetMode,
};
use nimbus_crypto::{KeyManifest, LocalKeySubject, ManifestCipher};
use nimbus_storage::libsql::libsql_transport_connector;
use nimbus_storage::{
    FaultInjector, FaultPoint, LibsqlReplicaProvider, LibsqlReplicaProviderConfig,
    NoopFaultInjector,
};

use super::*;
use crate::tenant::ManualLeaseRenewalClock;
use crate::{
    ControlPlaneConfig, LibsqlReplicaBarrierPath, LibsqlReplicaRefreshPath, LocalEncryptionConfig,
    LocalKeyProviderConfig, MasterKeyFileConfig, PersistenceDialect, PersistenceTopology,
    PoolConfig, ProviderCredentials, TenantProviderConfig, TenantRoutingConfig,
};

const LIBSQL_URL_ENV: &str = "NIMBUS_LIBSQL_URL";
const LIBSQL_AUTH_TOKEN_ENV: &str = "NIMBUS_LIBSQL_AUTH_TOKEN";
const LIBSQL_ADMIN_URL_ENV: &str = "NIMBUS_LIBSQL_ADMIN_URL";
const LIBSQL_ADMIN_AUTH_HEADER_ENV: &str = "NIMBUS_LIBSQL_ADMIN_AUTH_HEADER";
static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct ArmedLibsqlCommitAcknowledgementLoss {
    armed: AtomicBool,
    fired: AtomicBool,
}

impl ArmedLibsqlCommitAcknowledgementLoss {
    fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }
}

impl FaultInjector for ArmedLibsqlCommitAcknowledgementLoss {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point == FaultPoint::StorageCommitAfterVisibilityBeforeReturn
            && self.armed.load(Ordering::Acquire)
            && !self.fired.swap(true, Ordering::AcqRel)
        {
            return Err(nimbus_core::Error::storage(
                nimbus_core::StorageErrorKind::Transient,
                "injected libSQL commit acknowledgement loss",
            ));
        }
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn libsql_replica_post_visibility_ack_loss_forces_crash_and_replay() {
    with_libsql_replica_engine_config(|engine_config, provider_config| async move {
        let clock = Arc::new(ManualClock::new(Timestamp(8_500)));
        let faults = Arc::new(ArmedLibsqlCommitAcknowledgementLoss::default());
        let engine = Arc::new(
            Engine::new_with_simulation_and_persistence_config(
                engine_config,
                clock,
                faults.clone(),
            )
            .await
            .expect("replica-backed engine should create"),
        );
        let tenant_id = TenantId::new("libsql-replica-ack-loss").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        engine
            .shutdown_trigger_candidates_for_testing(&tenant_id)
            .expect("trigger cursor should not add unrelated records");
        let failed_runtime = engine
            .tenant_runtime_for_testing(&tenant_id)
            .expect("loaded runtime should be inspectable");
        let failed_runtime_identity = engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("loaded runtime identity should read");

        faults.arm();
        let error = engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([(
                    "title".to_string(),
                    serde_json::json!("landed-without-ack"),
                )]),
            )
            .await
            .expect_err("lost libSQL acknowledgement must be terminally ambiguous");
        assert_eq!(error.retryability(), nimbus_core::Retryability::Terminal);
        assert!(
            error.to_string().contains("crash-and-replay"),
            "a post-visibility libSQL error must force durable recovery: {error}"
        );
        let documents = engine
            .query_documents_async(tenant_id.clone(), query_for("tasks"))
            .await
            .expect("query should wait for eviction and reload durable state");
        assert_eq!(documents.len(), 1);
        assert_eq!(
            documents[0]
                .fields
                .get("title")
                .and_then(serde_json::Value::as_str),
            Some("landed-without-ack")
        );
        assert_ne!(
            engine
                .tenant_runtime_identity_for_testing(&tenant_id)
                .expect("replacement runtime identity should read"),
            failed_runtime_identity,
            "ambiguous libSQL outcome must replace the failed runtime"
        );
        assert!(
            !engine.runtime_is_registered_for_testing(&tenant_id, &failed_runtime),
            "the failed runtime must be deregistered after durable recovery"
        );
        assert_eq!(
            failed_runtime
                .mutation_journal_stats()
                .committer_lease_epoch,
            1,
            "the failed runtime should have acquired the tenant's first lease epoch"
        );

        engine
            .shutdown_trigger_candidates_for_testing(&tenant_id)
            .expect("replacement trigger cursor should stay quiet");
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([(
                    "title".to_string(),
                    serde_json::json!("next-independent-write"),
                )]),
            )
            .await
            .expect("the replacement runtime should continue at the next sequence");
        assert_eq!(
            engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("replacement lease stats should read")
                .committer_lease_epoch,
            1,
            "same-engine recovery must resume its lease identity without waiting for expiry or advancing the epoch"
        );

        let provider = LibsqlReplicaProvider::connect(provider_config)
            .await
            .expect("inspection provider should connect");
        let opened = provider
            .open_existing_opened_tenant(&tenant_id)
            .await
            .expect("tenant lookup should succeed")
            .expect("tenant should exist");
        let records = opened
            .store
            .read_durable_journal_from(nimbus_core::SequenceNumber(1))
            .expect("provider journal should read");
        assert_eq!(
            records
                .iter()
                .map(|record| record.sequence)
                .collect::<Vec<_>>(),
            vec![
                nimbus_core::SequenceNumber(1),
                nimbus_core::SequenceNumber(2),
            ],
            "recovery must neither duplicate nor reuse the ambiguous sequence"
        );
        assert_eq!(
            records
                .iter()
                .flat_map(|record| record.writes.iter())
                .filter_map(|write| write.current.as_ref())
                .filter_map(|document| document.fields.get("title"))
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>(),
            vec!["landed-without-ack", "next-independent-write"]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn libsql_replica_lease_is_lazy_and_renews_from_manual_clock_wakeup() {
    with_libsql_replica_engine_config(|engine_config, provider_config| async move {
        let clock = Arc::new(ManualClock::new(Timestamp(9_000)));
        let lease_clock = Arc::new(ManualLeaseRenewalClock::new());
        let engine = Arc::new(
            Engine::new_with_simulation_and_persistence_config_and_lease_clock(
                engine_config,
                clock.clone(),
                Arc::new(NoopFaultInjector),
                lease_clock.clone(),
            )
            .await
            .expect("replica-backed engine should create"),
        );
        let tenant_id = TenantId::new("libsql-replica-lazy-lease").expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");

        let provider = LibsqlReplicaProvider::connect(provider_config)
            .await
            .expect("inspection provider should connect");
        let opened = provider
            .open_existing_opened_tenant(&tenant_id)
            .await
            .expect("tenant lookup should succeed")
            .expect("tenant should exist");
        let loaded = engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("loaded stats should read");
        assert!(!loaded.committer_lease_acquired);
        assert_eq!(loaded.committer_lease_acquire_count, 0);
        assert!(
            opened
                .store
                .read_committer_lease()
                .expect("lease read should succeed")
                .is_none()
        );

        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
            )
            .await
            .expect("first assignment should acquire and commit");
        let acquired = engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("acquired stats should read");
        assert!(acquired.committer_lease_acquired);
        assert_eq!(acquired.committer_lease_epoch, 1);
        assert_eq!(acquired.committer_lease_acquire_count, 1);
        let initial_expiry = acquired.committer_lease_expires_at;

        lease_clock.advance(Duration::from_secs(10));
        engine
            .wake_committer_lease_renewal_for_testing(&tenant_id)
            .expect("renewal worker should wake");
        let renewed = wait_for_mutation_journal_stats(
            &engine,
            &tenant_id,
            "libsql manual-clock renewal should complete",
            |stats| stats.committer_lease_renewal_count == 1,
        )
        .await;
        assert!(renewed.committer_lease_expires_at > initial_expiry);

        engine.quiesce().await;
        assert!(
            !engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("quiesced stats should read")
                .committer_lease_renewal_worker_running
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn typed_libsql_replica_config_reads_seeded_remote_state_and_reopens() {
    with_shared_libsql_replica_engine_configs(
        |engine_config_a, engine_config_b, provider_config| async move {
            let provider = LibsqlReplicaProvider::connect(provider_config.clone())
                .await
                .expect("replica provider should connect");
            let tenant_id = TenantId::new("libsql-replica-tenant").expect("tenant id should build");
            let registration = provider
                .create_tenant(&tenant_id)
                .await
                .expect("tenant should create through provider");
            let document_id = DocumentId::new();
            seed_remote_namespace(
                &provider_config,
                &registration.namespace,
                &tasks_schema(),
                document_id.clone(),
                serde_json::json!({
                    "title": "from-primary"
                }),
            )
            .await;
            drop(provider);

            let engine = Arc::new(
                Engine::new_with_persistence_config(engine_config_a.clone())
                    .await
                    .expect("replica-backed engine should create"),
            );
            assert_eq!(
                engine
                    .list_tenants_async()
                    .await
                    .expect("tenant list should load from provider metadata"),
                vec![tenant_id.clone()]
            );
            engine
                .ensure_tenant_exists_async(tenant_id.clone())
                .await
                .expect("tenant should lazy load through the replica provider");
            let documents = engine
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("replica-backed query should succeed");
            assert_eq!(documents.len(), 1);
            assert_eq!(documents[0].id, document_id);
            assert_eq!(
                documents[0]
                    .fields
                    .get("title")
                    .and_then(|value| value.as_str()),
                Some("from-primary")
            );

            engine.quiesce().await;
            drop(engine);

            let reopened = Arc::new(
                Engine::new_with_persistence_config(engine_config_b)
                    .await
                    .expect("replica-backed engine should reopen"),
            );
            let reopened_documents = reopened
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("reopened replica-backed query should succeed");
            assert_eq!(reopened_documents.len(), 1);
            assert_eq!(reopened_documents[0].id, document_id);
            reopened.quiesce().await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn typed_libsql_replica_config_supports_async_schema_mutation_journal_and_scheduler_paths() {
    with_libsql_replica_engine_config(|engine_config, _provider_config| async move {
        let tenant_id = TenantId::new("libsql-replica-mutations").expect("tenant id should build");
        let engine = Arc::new(
            Engine::new_with_persistence_config(engine_config)
                .await
                .expect("replica-backed engine should create"),
        );

        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        assert_eq!(
            engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("libSQL committer-arm diagnostics should load")
                .committer_arm,
            crate::tenant::CommitterArm::Serial,
            "U5 must leave the libSQL production arm serial"
        );
        engine
            .set_table_schema_async(tenant_id.clone(), tasks_schema())
            .await
            .expect("schema write should succeed");

        let inserted_id = engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("title".to_string(), json!("First"))]),
            )
            .await
            .expect("insert should succeed");
        engine
            .update_document_async(
                tenant_id.clone(),
                tasks_table(),
                inserted_id,
                serde_json::Map::from_iter([("title".to_string(), json!("Renamed"))]),
            )
            .await
            .expect("update should succeed");

        let scheduled_job_id = engine
            .schedule_mutation_async(
                tenant_id.clone(),
                ScheduleRequest {
                    run_after_ms: 5_000,
                    mutation: Mutation::Insert {
                        table: tasks_table(),
                        id: None,
                        fields: serde_json::Map::from_iter([(
                            "title".to_string(),
                            json!("Scheduled"),
                        )]),
                    },
                },
            )
            .await
            .expect("scheduled mutation should persist");
        assert_eq!(
            engine
                .list_scheduled_jobs_async(tenant_id.clone())
                .await
                .expect("pending jobs should load")
                .len(),
            1
        );

        let claimed = engine
            .claim_due_jobs_async(tenant_id.clone(), Timestamp(u64::MAX))
            .await
            .expect("claim should succeed");
        assert_eq!(claimed.len(), 1);
        engine
            .record_scheduled_job_result_async(
                tenant_id.clone(),
                nimbus_core::ScheduledJobResult {
                    id: scheduled_job_id.clone(),
                    run_at: claimed[0].run_at,
                    finished_at: Timestamp(claimed[0].run_at.0.saturating_add(1)),
                    mutation: claimed[0].mutation.clone(),
                    outcome: ScheduledJobOutcome::Completed,
                    error: None,
                },
            )
            .await
            .expect("scheduled result should persist");
        engine
            .complete_scheduled_job_async(tenant_id.clone(), scheduled_job_id.clone())
            .await
            .expect("scheduled completion should persist");
        assert_eq!(
            engine
                .get_scheduled_job_result_async(tenant_id.clone(), scheduled_job_id.clone())
                .await
                .expect("scheduled result should load")
                .outcome,
            ScheduledJobOutcome::Completed
        );

        let documents = engine
            .query_documents_async(tenant_id.clone(), query_for("tasks"))
            .await
            .expect("query should succeed");
        assert_eq!(documents.len(), 1);
        assert_eq!(
            documents[0]
                .fields
                .get("title")
                .and_then(|value| value.as_str()),
            Some("Renamed")
        );
        assert_eq!(
            engine
                .latest_sequence_async(tenant_id.clone())
                .await
                .expect("latest sequence should track journaled mutations"),
            engine
                .mutation_journal_stats_for_testing(&tenant_id)
                .expect("journal stats should load")
                .durable_head
        );

        let bootstrap = engine
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await
            .expect("bootstrap should export");
        let latest_sequence = engine
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("latest sequence should load");
        assert_eq!(bootstrap.bootstrap_cut, latest_sequence);
        assert_eq!(bootstrap.resume_after, latest_sequence);

        engine.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn libsql_replica_background_poll_refreshes_loaded_runtime_schema_and_journal_state() {
    with_shared_libsql_replica_engine_configs(
        |engine_config_a, engine_config_b, _provider_config| async move {
            let tenant_id =
                TenantId::new("libsql-replica-poll-journal").expect("tenant id should build");
            let engine_a = Arc::new(
                Engine::new_with_persistence_config(engine_config_a)
                    .await
                    .expect("first replica-backed engine should create"),
            );
            let engine_b = Arc::new(
                Engine::new_with_persistence_config(engine_config_b)
                    .await
                    .expect("second replica-backed engine should create"),
            );

            engine_a
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");
            engine_b
                .ensure_tenant_exists_async(tenant_id.clone())
                .await
                .expect("second engine should load tenant");
            assert_eq!(
                engine_b
                    .get_schema_async(tenant_id.clone())
                    .await
                    .expect("empty schema should load"),
                nimbus_core::Schema::default()
            );

            engine_a
                .set_table_schema_async(tenant_id.clone(), tasks_schema())
                .await
                .expect("schema write should succeed");
            engine_a
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("External"))]),
                )
                .await
                .expect("insert should succeed");

            wait_for_value(
                "replica poll should refresh loaded schema",
                Duration::from_secs(3),
                Duration::from_millis(25),
                || {
                    let engine = engine_b.clone();
                    let tenant_id = tenant_id.clone();
                    async move {
                        engine
                            .get_schema_async(tenant_id)
                            .await
                            .expect("schema should load")
                    }
                },
                |schema| schema.get_table(&tasks_table()).is_some(),
            )
            .await;
            wait_for_mutation_journal_stats(
                &engine_b,
                &tenant_id,
                "replica poll should catch up journal heads",
                |stats| {
                    stats.durable_head.0 >= 2
                        && stats.applied_head.0 >= 2
                },
            )
            .await;

            let documents = engine_b
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("caught-up query should succeed");
            assert_eq!(documents.len(), 1);
            assert_eq!(
                documents[0]
                    .fields
                    .get("title")
                    .and_then(|value| value.as_str()),
                Some("External")
            );
            let diagnostics = engine_b
                .tenant_engine_diagnostics_async(tenant_id.clone())
                .await
                .expect("tenant diagnostics should surface replica freshness");
            let freshness = diagnostics
                .libsql_replica_freshness
                .expect("libsql-replica diagnostics should include freshness stats");
            assert!(
                freshness.required_sequence.0 >= 2,
                "replica diagnostics should require at least the remote schema and document writes"
            );
            assert!(
                freshness.local_applied_sequence.0 >= 2,
                "replica diagnostics should report local visibility through the remote document write"
            );
            assert_eq!(freshness.refresh_error_count, 0);
            assert!(
                freshness.incremental_refresh_count
                    + freshness.full_snapshot_refresh_count
                    + freshness.incremental_fallback_to_snapshot_count
                    >= 1,
                "replica diagnostics should show at least one refresh path"
            );
            assert_ne!(
                freshness.last_barrier_path,
                LibsqlReplicaBarrierPath::Unknown
            );
            assert_ne!(
                freshness.last_refresh_path,
                LibsqlReplicaRefreshPath::Unknown
            );

            engine_a.quiesce().await;
            engine_b.quiesce().await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn libsql_replica_background_poll_loads_unloaded_tenants_with_scheduled_work() {
    with_shared_libsql_replica_engine_configs(
        |engine_config_a, engine_config_b, _provider_config| async move {
            let tenant_id =
                TenantId::new("libsql-replica-poll-scheduler").expect("tenant id should build");
            let engine_a = Arc::new(
                Engine::new_with_persistence_config(engine_config_a)
                    .await
                    .expect("first replica-backed engine should create"),
            );
            let engine_b = Arc::new(
                Engine::new_with_persistence_config(engine_config_b)
                    .await
                    .expect("second replica-backed engine should create"),
            );

            engine_b
                .load_tenants_with_scheduled_work_async()
                .await
                .expect("initial scheduled-work preload should succeed");
            engine_a
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");

            let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
            let scheduler_handle =
                tokio::spawn(crate::run_scheduler(engine_b.clone(), shutdown_rx));
            engine_a
                .schedule_mutation_async(
                    tenant_id.clone(),
                    ScheduleRequest {
                        run_after_ms: 0,
                        mutation: Mutation::Insert {
                            table: tasks_table(),
                            id: None,
                            fields: serde_json::Map::from_iter([(
                                "title".to_string(),
                                json!("Scheduled externally"),
                            )]),
                        },
                    },
                )
                .await
                .expect("scheduled mutation should persist");

            wait_for_value(
                "replica poll should load the scheduled tenant into the second engine",
                Duration::from_secs(3),
                Duration::from_millis(25),
                || {
                    let engine = engine_b.clone();
                    async move { engine.loaded_tenant_ids() }
                },
                |tenant_ids| tenant_ids.contains(&tenant_id),
            )
            .await;
            wait_for_value(
                "replica poll should execute scheduled work on the second engine",
                Duration::from_secs(3),
                Duration::from_millis(25),
                || {
                    let engine = engine_b.clone();
                    let tenant_id = tenant_id.clone();
                    async move {
                        engine
                            .query_documents_async(tenant_id, query_for("tasks"))
                            .await
                            .expect("query should succeed")
                    }
                },
                |documents| {
                    documents.iter().any(|document| {
                        document
                            .fields
                            .get("title")
                            .and_then(|value| value.as_str())
                            == Some("Scheduled externally")
                    })
                },
            )
            .await;

            let _ = shutdown_tx.send(true);
            scheduler_handle.await.expect("scheduler should shut down");
            engine_a.quiesce().await;
            engine_b.quiesce().await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn encrypted_libsql_replica_config_reads_seeded_remote_state_and_reopens_existing_cache() {
    with_encrypted_libsql_replica_engine_config(|engine_config, provider_config| async move {
        let provider = LibsqlReplicaProvider::connect(provider_config.clone())
            .await
            .expect("replica provider should connect");
        let tenant_id =
            TenantId::new("libsql-replica-encrypted-tenant").expect("tenant id should build");
        let registration = provider
            .create_tenant(&tenant_id)
            .await
            .expect("tenant should create through provider");
        let document_id = DocumentId::new();
        seed_remote_namespace(
            &provider_config,
            &registration.namespace,
            &tasks_schema(),
            document_id.clone(),
            serde_json::json!({
                "title": "from-encrypted-primary"
            }),
        )
        .await;
        let replica_path = provider.replica_path_for_tenant(&tenant_id);
        let manifest_path = KeyManifest::manifest_path(&replica_path);
        drop(provider);

        assert!(
            !replica_path.exists(),
            "local replica cache should not exist before the first encrypted open"
        );
        assert!(
            !manifest_path.exists(),
            "manifest should not exist before the first encrypted open"
        );

        let engine = Arc::new(
            Engine::new_with_persistence_config(engine_config.clone())
                .await
                .expect("encrypted replica-backed engine should create"),
        );
        engine
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("tenant should lazy load through the encrypted replica provider");
        let documents = engine
            .query_documents_async(tenant_id.clone(), query_for("tasks"))
            .await
            .expect("encrypted replica-backed query should succeed");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].id, document_id);
        assert_eq!(
            documents[0]
                .fields
                .get("title")
                .and_then(|value| value.as_str()),
            Some("from-encrypted-primary")
        );
        assert!(replica_path.exists(), "encrypted cache should materialize");
        assert!(
            manifest_path.exists(),
            "encrypted cache manifest should materialize"
        );

        let manifest = KeyManifest::read_for(&replica_path)
            .expect("encrypted cache manifest should read back after first open");
        assert_eq!(manifest.header.cipher, ManifestCipher::SqlCipher);
        let logical_name = replica_path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("replica cache path should end with a filename");
        assert_eq!(
            manifest.header.subject_descriptor,
            LocalKeySubject::libsql_cache(tenant_id.clone(), logical_name).descriptor()
        );
        assert_sqlite_file_is_not_plaintext_header(&replica_path);

        engine.quiesce().await;
        drop(engine);

        let reopened = Arc::new(
            Engine::new_with_persistence_config(engine_config)
                .await
                .expect("encrypted replica-backed engine should reopen"),
        );
        let reopened_documents = reopened
            .query_documents_async(tenant_id.clone(), query_for("tasks"))
            .await
            .expect("reopened encrypted replica-backed query should succeed");
        assert_eq!(reopened_documents.len(), 1);
        assert_eq!(reopened_documents[0].id, document_id);
        assert_eq!(
            reopened_documents[0]
                .fields
                .get("title")
                .and_then(|value| value.as_str()),
            Some("from-encrypted-primary")
        );
        assert_sqlite_file_is_not_plaintext_header(&replica_path);
        reopened.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial(libsql_replica_provider)]
async fn libsql_replica_collection_group_queries_use_path_binding_metadata() {
    with_libsql_replica_engine_config(|engine_config, _provider_config| async move {
        let tenant_id =
            TenantId::new("libsql-replica-collection-group").expect("tenant id should build");
        let engine = Arc::new(
            Engine::new_with_persistence_config(engine_config)
                .await
                .expect("replica-backed engine should create"),
        );
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");

        let direct_table = TableName::new("landmarks_direct").expect("table name should build");
        let nested_table = TableName::new("landmarks_nested").expect("table name should build");
        let other_table = TableName::new("landmarks_other").expect("table name should build");
        for table in [&direct_table, &nested_table, &other_table] {
            engine
                .set_table_schema_async(
                    tenant_id.clone(),
                    TableSchema {
                        table: table.clone(),
                        fields: vec![FieldSchema {
                            name: "rank".to_string(),
                            field_type: FieldType::Number,
                            required: false,
                        }],
                        indexes: vec![IndexDefinition { id: nimbus_core::IndexId::new(), state: nimbus_core::IndexState::Enabled,
                            name: "by_rank".to_string(),
                            fields: vec!["rank".to_string()],
                        }],
                        access_policy: None,
                    },
                )
                .await
                .expect("landmarks schema should persist");
        }

        seed_bound_collection_group_document(
            &engine,
            &tenant_id,
            direct_table.clone(),
            "aa-top",
            &["cities", "SF", "landmarks", "aa-top"],
            [("rank", json!(1))],
        );
        seed_bound_collection_group_document(
            &engine,
            &tenant_id,
            direct_table.clone(),
            "bb-top",
            &["cities", "SF", "landmarks", "bb-top"],
            [("rank", json!(2))],
        );
        seed_bound_collection_group_document(
            &engine,
            &tenant_id,
            nested_table.clone(),
            "zz-top",
            &["cities", "SF", "districts", "1", "landmarks", "zz-top"],
            [("rank", json!(3))],
        );
        seed_bound_collection_group_document(
            &engine,
            &tenant_id,
            other_table,
            "cc-top",
            &["cities", "LA", "landmarks", "cc-top"],
            [("rank", json!(4))],
        );

        let rows = engine
            .query_collection_group_documents_structured_with_principal_cancellable(
                &tenant_id,
                &CollectionName::new("landmarks").expect("collection group should parse"),
                Some(
                    &DocumentPath::from_segments(["cities", "SF"])
                        .expect("ancestor path should parse"),
                ),
                &StructuredQuery {
                    order_by: vec![StructuredOrder {
                        field: FieldReference::new("__name__"),
                        direction: QueryDirection::Ascending,
                    }],
                    start_at: Some(StructuredCursor {
                        values: vec![json!("cities/SF/landmarks/aa-top")],
                        before: true,
                    }),
                    ..StructuredQuery::default()
                },
                &PrincipalContext::anonymous(),
                &mut || Ok(()),
            )
            .expect("collection-group query should succeed on libsql replicas");

        assert_eq!(
            rows.into_iter()
                .map(|(path, document)| (path.to_string(), document.get_field("rank").cloned()))
                .collect::<Vec<_>>(),
            vec![
                ("cities/SF/landmarks/aa-top".to_string(), Some(json!(1))),
                ("cities/SF/landmarks/bb-top".to_string(), Some(json!(2))),
            ],
            "libsql collection-group queries should use the persisted path bindings and full document-path cursors"
        );

        engine.quiesce().await;
    })
    .await;
}

async fn with_libsql_replica_engine_config<F, Fut>(test: F)
where
    F: FnOnce(EnginePersistenceConfig, LibsqlReplicaProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    with_shared_libsql_replica_engine_configs(
        |engine_config, _unused, provider_config| async move {
            test(engine_config, provider_config).await;
        },
    )
    .await;
}

async fn with_encrypted_libsql_replica_engine_config<F, Fut>(test: F)
where
    F: FnOnce(EnginePersistenceConfig, LibsqlReplicaProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let suffix = unique_suffix();
    let metadata_namespace = format!("nimbus_meta_{}", &suffix[..16.min(suffix.len())]);
    let tenant_namespace_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let replica_cache_dir = tempdir().expect("encrypted replica cache dir should create");
    let control_dir = tempdir().expect("encrypted control tempdir should build");
    let key_dir = tempdir().expect("master key tempdir should build");
    let key_path = key_dir.path().join("master.key");
    std::fs::write(&key_path, [0x42_u8; 32]).expect("master key file should write");

    let provider_config = LibsqlReplicaProviderConfig {
        primary_url: connection.primary_url().to_string(),
        auth_token: connection.auth_token().map(ToOwned::to_owned),
        admin_api_url: connection.admin_api_url().to_string(),
        admin_auth_header: connection.admin_auth_header().map(ToOwned::to_owned),
        metadata_namespace: metadata_namespace.clone(),
        tenant_namespace_prefix: tenant_namespace_prefix.clone(),
        replica_cache_dir: replica_cache_dir.path().to_path_buf(),
        encryption_provider: None,
    };

    let engine_config = EnginePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            dialect: PersistenceDialect::Sqlite,
            topology: PersistenceTopology::ExternalPrimaryWithReplicas,
            routing: TenantRoutingConfig::NamespacePerTenant {
                metadata_namespace,
                tenant_namespace_prefix,
                replica_cache_dir: replica_cache_dir.path().to_path_buf(),
            },
            pool: PoolConfig::default(),
            credentials: ProviderCredentials::LibsqlReplica {
                primary_url: connection.primary_url().to_string(),
                auth_token: connection.auth_token().map(ToOwned::to_owned),
                admin_api_url: connection.admin_api_url().to_string(),
                admin_auth_header: connection.admin_auth_header().map(ToOwned::to_owned),
            },
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_dir.path()),
        local_encryption: LocalEncryptionConfig::Enabled(LocalKeyProviderConfig::MasterKeyFile(
            MasterKeyFileConfig { path: key_path },
        )),
    };

    test(engine_config, provider_config.clone()).await;

    LibsqlReplicaProvider::connect(provider_config)
        .await
        .expect("cleanup provider should connect")
        .drop_provider_namespaces_for_test()
        .await
        .expect("provider namespaces should clean up");
    drop(connection);
}

async fn with_shared_libsql_replica_engine_configs<F, Fut>(test: F)
where
    F: FnOnce(EnginePersistenceConfig, EnginePersistenceConfig, LibsqlReplicaProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let suffix = unique_suffix();
    let metadata_namespace = format!("nimbus_meta_{}", &suffix[..16.min(suffix.len())]);
    let tenant_namespace_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let replica_cache_dir_a = tempdir().expect("first replica cache dir should create");
    let replica_cache_dir_b = tempdir().expect("second replica cache dir should create");
    let control_dir_a = tempdir().expect("first control tempdir should build");
    let control_dir_b = tempdir().expect("second control tempdir should build");

    let provider_config = LibsqlReplicaProviderConfig {
        primary_url: connection.primary_url().to_string(),
        auth_token: connection.auth_token().map(ToOwned::to_owned),
        admin_api_url: connection.admin_api_url().to_string(),
        admin_auth_header: connection.admin_auth_header().map(ToOwned::to_owned),
        metadata_namespace: metadata_namespace.clone(),
        tenant_namespace_prefix: tenant_namespace_prefix.clone(),
        replica_cache_dir: replica_cache_dir_a.path().to_path_buf(),
        encryption_provider: None,
    };

    let engine_config_a = EnginePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            dialect: PersistenceDialect::Sqlite,
            topology: PersistenceTopology::ExternalPrimaryWithReplicas,
            routing: TenantRoutingConfig::NamespacePerTenant {
                metadata_namespace: metadata_namespace.clone(),
                tenant_namespace_prefix: tenant_namespace_prefix.clone(),
                replica_cache_dir: replica_cache_dir_a.path().to_path_buf(),
            },
            pool: PoolConfig::default(),
            credentials: ProviderCredentials::LibsqlReplica {
                primary_url: connection.primary_url().to_string(),
                auth_token: connection.auth_token().map(ToOwned::to_owned),
                admin_api_url: connection.admin_api_url().to_string(),
                admin_auth_header: connection.admin_auth_header().map(ToOwned::to_owned),
            },
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_dir_a.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };
    let engine_config_b = EnginePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            routing: TenantRoutingConfig::NamespacePerTenant {
                metadata_namespace,
                tenant_namespace_prefix,
                replica_cache_dir: replica_cache_dir_b.path().to_path_buf(),
            },
            ..engine_config_a.tenant_provider.clone()
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_dir_b.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };

    test(engine_config_a, engine_config_b, provider_config.clone()).await;

    LibsqlReplicaProvider::connect(provider_config)
        .await
        .expect("cleanup provider should connect")
        .drop_provider_namespaces_for_test()
        .await
        .expect("provider namespaces should clean up");
    drop(connection);
}

struct TestConnection {
    primary_url: String,
    auth_token: Option<String>,
    admin_api_url: String,
    admin_auth_header: Option<String>,
}

impl TestConnection {
    fn primary_url(&self) -> &str {
        &self.primary_url
    }

    fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    fn admin_api_url(&self) -> &str {
        &self.admin_api_url
    }

    fn admin_auth_header(&self) -> Option<&str> {
        self.admin_auth_header.as_deref()
    }
}

async fn test_connection() -> Option<TestConnection> {
    match external_provider_fixture_mode(
        "libsql",
        "libSQL replica engine provider",
        &[LIBSQL_URL_ENV, LIBSQL_ADMIN_URL_ENV],
    ) {
        ExternalProviderFixtureMode::UseExplicit => Some(TestConnection {
            primary_url: env::var(LIBSQL_URL_ENV)
                .expect("fixture policy should require the libSQL primary URL"),
            auth_token: env::var(LIBSQL_AUTH_TOKEN_ENV).ok(),
            admin_api_url: env::var(LIBSQL_ADMIN_URL_ENV)
                .expect("fixture policy should require the libSQL admin URL"),
            admin_auth_header: env::var(LIBSQL_ADMIN_AUTH_HEADER_ENV).ok(),
        }),
        ExternalProviderFixtureMode::Omit => None,
    }
}

fn assert_sqlite_file_is_not_plaintext_header(path: &Path) {
    let mut header = [0_u8; 16];
    let mut file = std::fs::File::open(path).expect("replica cache file should open");
    let bytes_read = file
        .read(&mut header)
        .expect("replica cache header should read");
    assert_eq!(
        bytes_read,
        header.len(),
        "replica cache file should contain a full SQLite header-sized prefix"
    );
    assert_ne!(
        &header, b"SQLite format 3\0",
        "encrypted replica cache should not expose the plaintext SQLite header on disk"
    );
}

fn unique_suffix() -> String {
    format!(
        "{:x}{:x}{:016x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after the unix epoch")
            .as_nanos(),
        TEST_SUFFIX_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn seed_bound_collection_group_document(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: TableName,
    document_id: &str,
    document_path: &[&str],
    fields: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) {
    let document_id = DocumentId::from_key(document_id).expect("document id should parse");
    let document_path = DocumentPath::from_segments(document_path.iter().copied())
        .expect("document path should parse");
    let batch = AtomicWriteBatch::new(vec![AtomicWrite::Set {
        key: WriteKey::from(ResourcePathBinding::new(
            DocumentLocator::new(table, document_id),
            document_path,
        )),
        document: serde_json::Map::from_iter(
            fields
                .into_iter()
                .map(|(field, value)| (field.to_string(), value)),
        ),
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    }])
    .expect("seed write batch should build");
    engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("seed execution unit should begin")
        .execute_atomic_write_batch(batch)
        .expect("seed write batch should commit");
}

fn tasks_schema() -> TableSchema {
    TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "title".to_string(),
            field_type: FieldType::String,
            required: true,
        }],
        indexes: Vec::new(),
        access_policy: None,
    }
}

async fn seed_remote_namespace(
    config: &LibsqlReplicaProviderConfig,
    namespace: &str,
    table_schema: &TableSchema,
    document_id: DocumentId,
    fields: serde_json::Value,
) {
    let database = open_remote_namespace_database(config, namespace)
        .await
        .expect("remote namespace database should open");
    let conn = database
        .connect()
        .expect("remote namespace connection should open");
    conn.execute(
        "INSERT INTO schemas (table_name, schema_json) VALUES (?, ?)",
        libsql::params![
            table_schema.table.as_str(),
            serde_json::to_string(table_schema).expect("schema should serialize")
        ],
    )
    .await
    .expect("remote schema insert should succeed");
    let table_id = TableId::new();
    conn.execute(
        "INSERT INTO table_catalog (namespace, table_name, table_id) VALUES (?, ?, ?)",
        libsql::params!["default", table_schema.table.as_str(), table_id.as_str()],
    )
    .await
    .expect("remote table catalog insert should succeed");
    conn.execute(
        "INSERT INTO documents (table_id, id, data_json, typed_fields_json, creation_time, update_time) VALUES (?, ?, ?, ?, ?, ?)",
        libsql::params![
            table_id.as_str(),
            document_id.to_string(),
            fields.to_string(),
            "{}",
            7_i64,
            7_i64
        ],
    )
    .await
    .expect("remote document insert should succeed");
}

async fn open_remote_namespace_database(
    config: &LibsqlReplicaProviderConfig,
    namespace: &str,
) -> nimbus_core::Result<Database> {
    let builder = Builder::new_remote(
        config.primary_url.clone(),
        config.auth_token.clone().unwrap_or_default(),
    )
    .namespace(namespace.to_string())
    .connector(libsql_transport_connector()?);
    builder.build().await.map_err(|error| {
        nimbus_core::Error::storage(nimbus_core::StorageErrorKind::Other, error.to_string())
    })
}
