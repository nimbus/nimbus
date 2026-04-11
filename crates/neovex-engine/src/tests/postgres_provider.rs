use std::collections::BTreeSet;
use std::env;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use neovex_core::{
    Document, DocumentId, Mutation, ScheduleRequest, ScheduledJobOutcome, Schema, Timestamp,
};
use neovex_storage::{PostgresProvider, PostgresProviderConfig};
use testcontainers_modules::{
    postgres,
    testcontainers::{ContainerAsync, runners::AsyncRunner},
};
use tokio_postgres::NoTls;

use super::*;
use crate::{
    ControlPlaneConfig, PersistenceDialect, PersistenceTopology, PoolConfig, ProviderCredentials,
    TenantProviderConfig, TenantRoutingConfig,
};

const TEST_POSTGRES_URL_ENV: &str = "NEOVEX_TEST_POSTGRES_URL";
static TEST_SUFFIX_COUNTER: AtomicU64 = AtomicU64::new(0);

#[tokio::test(flavor = "multi_thread")]
async fn typed_postgres_config_supports_async_tenant_lifecycle_and_empty_read_paths() {
    with_postgres_service_config(|service_config, _provider_config| async move {
        let tenant_id = TenantId::new("pg-tenant").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config.clone())
                .await
                .expect("postgres-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        assert_eq!(
            service
                .list_tenants_async()
                .await
                .expect("tenant list should load"),
            vec![tenant_id.clone()]
        );
        service
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("tenant existence should verify");
        assert_eq!(
            service
                .latest_sequence_async(tenant_id.clone())
                .await
                .expect("latest sequence should read"),
            SequenceNumber(0)
        );
        assert!(
            service
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("empty query should succeed")
                .is_empty()
        );
        let bootstrap = service
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await
            .expect("bootstrap should export");
        assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(0));
        assert_eq!(bootstrap.resume_after, SequenceNumber(0));
        assert_eq!(bootstrap.cursor_floor, SequenceNumber(0));
        assert!(bootstrap.snapshot.documents.is_empty());

        service.quiesce().await;
        drop(service);

        let reopened = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should reopen"),
        );
        assert_eq!(
            reopened
                .list_tenants_async()
                .await
                .expect("reopened tenant list should load"),
            vec![tenant_id.clone()]
        );
        reopened
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("tenant should lazy load after reopen");
        assert!(
            reopened
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("reopened empty query should succeed")
                .is_empty()
        );
        reopened.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn typed_postgres_config_supports_async_schema_mutation_journal_and_scheduler_paths() {
    with_postgres_service_config(|service_config, _provider_config| async move {
        let tenant_id = TenantId::new("pg-mutations").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        service
            .set_table_schema_async(tenant_id.clone(), tasks_schema())
            .await
            .expect("schema write should succeed");

        let inserted_id = service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("title".to_string(), json!("First"))]),
            )
            .await
            .expect("insert should succeed");
        service
            .update_document_async(
                tenant_id.clone(),
                tasks_table(),
                inserted_id,
                serde_json::Map::from_iter([("title".to_string(), json!("Renamed"))]),
            )
            .await
            .expect("update should succeed");

        let scheduled_job_id = service
            .schedule_mutation_async(
                tenant_id.clone(),
                ScheduleRequest {
                    run_after_ms: 5_000,
                    mutation: Mutation::Insert {
                        table: tasks_table(),
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
            service
                .list_scheduled_jobs_async(tenant_id.clone())
                .await
                .expect("pending jobs should load")
                .len(),
            1
        );

        let claimed = service
            .claim_due_jobs_async(tenant_id.clone(), Timestamp(u64::MAX))
            .await
            .expect("claim should succeed");
        assert_eq!(claimed.len(), 1);
        service
            .record_scheduled_job_result_async(
                tenant_id.clone(),
                neovex_core::ScheduledJobResult {
                    id: scheduled_job_id,
                    run_at: claimed[0].run_at,
                    finished_at: Timestamp(claimed[0].run_at.0.saturating_add(1)),
                    mutation: claimed[0].mutation.clone(),
                    outcome: ScheduledJobOutcome::Completed,
                    error: None,
                },
            )
            .await
            .expect("scheduled result should persist");
        service
            .complete_scheduled_job_async(tenant_id.clone(), scheduled_job_id)
            .await
            .expect("scheduled completion should persist");
        assert_eq!(
            service
                .get_scheduled_job_result_async(tenant_id.clone(), scheduled_job_id)
                .await
                .expect("scheduled result should load")
                .outcome,
            ScheduledJobOutcome::Completed
        );

        let documents = service
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

        let bootstrap = service
            .export_durable_journal_bootstrap_async(tenant_id.clone())
            .await
            .expect("bootstrap should export");
        assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(2));
        assert_eq!(bootstrap.resume_after, SequenceNumber(2));

        service.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn typed_postgres_config_keeps_sequence_heads_in_sync_across_repeated_direct_crud() {
    with_postgres_service_config(|service_config, _provider_config| async move {
        let tenant_id = TenantId::new("pg-repeated-crud").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");

        const CRUD_ROUNDS: usize = 128;
        for round in 0..CRUD_ROUNDS {
            let document_id = service
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([
                        ("title".to_string(), json!(format!("round-{round}"))),
                        ("rank".to_string(), json!(round)),
                    ]),
                )
                .await
                .expect("insert should succeed");
            service
                .update_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([("rank".to_string(), json!(round + CRUD_ROUNDS))]),
                )
                .await
                .expect("update should succeed");
            service
                .delete_document_async(tenant_id.clone(), tasks_table(), document_id)
                .await
                .expect("delete should succeed");
        }

        assert_eq!(
            service
                .latest_sequence_async(tenant_id.clone())
                .await
                .expect("latest sequence should track every direct mutation"),
            SequenceNumber((CRUD_ROUNDS * 3) as u64)
        );
        assert!(
            service
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("query should succeed after repeated CRUD")
                .is_empty(),
            "repeated direct CRUD should leave no remaining documents"
        );

        drop(service);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn typed_postgres_config_reopens_multiple_tenants_for_concurrent_mixed_ops() {
    with_postgres_service_config(|service_config, _provider_config| async move {
        let service = Arc::new(
            Service::new_with_persistence_config(service_config.clone())
                .await
                .expect("postgres-backed service should create"),
        );

        let mut seeded = Vec::new();
        for index in 0..4 {
            let tenant_id =
                TenantId::new(format!("pg-reopen-mixed-{index}")).expect("tenant id should build");
            service
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");
            service
                .set_table_schema_async(tenant_id.clone(), tasks_schema())
                .await
                .expect("schema should persist");
            let document_id = service
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!(format!("seed-{index}")),
                    )]),
                )
                .await
                .expect("seed insert should succeed");
            seeded.push((tenant_id, document_id));
        }

        service.quiesce().await;
        drop(service);

        let reopened = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should reopen"),
        );

        tokio::time::timeout(Duration::from_secs(10), async {
            let mut handles = Vec::new();
            for (index, (tenant_id, document_id)) in seeded.into_iter().enumerate() {
                let service = reopened.clone();
                handles.push(tokio::spawn(async move {
                    let document = service
                        .get_document_async(tenant_id.clone(), tasks_table(), document_id)
                        .await
                        .expect("reopened point read should succeed");
                    let expected_title = format!("seed-{index}");
                    assert_eq!(
                        document
                            .fields
                            .get("title")
                            .and_then(|value| value.as_str()),
                        Some(expected_title.as_str())
                    );

                    let documents = service
                        .query_documents_async(tenant_id.clone(), query_for("tasks"))
                        .await
                        .expect("reopened query should succeed");
                    assert_eq!(documents.len(), 1);

                    let inserted_id = service
                        .insert_document_async(
                            tenant_id.clone(),
                            tasks_table(),
                            serde_json::Map::from_iter([(
                                "title".to_string(),
                                json!(format!("reopened-{index}")),
                            )]),
                        )
                        .await
                        .expect("reopened insert should succeed");
                    service
                        .update_document_async(
                            tenant_id.clone(),
                            tasks_table(),
                            inserted_id,
                            serde_json::Map::from_iter([(
                                "title".to_string(),
                                json!(format!("updated-{index}")),
                            )]),
                        )
                        .await
                        .expect("reopened update should succeed");
                }));
            }

            for handle in handles {
                handle.await.expect("task should join");
            }
        })
        .await
        .expect("reopened concurrent mixed ops should finish promptly");

        reopened.quiesce().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_notifications_refresh_loaded_runtime_schema_and_journal_state() {
    with_shared_postgres_service_configs(
        |service_config_a, service_config_b, _provider_config| async move {
            let tenant_id = TenantId::new("pg-notify-journal").expect("tenant id should build");
            let service_a = Arc::new(
                Service::new_with_persistence_config(service_config_a)
                    .await
                    .expect("first postgres-backed service should create"),
            );
            let service_b = Arc::new(
                Service::new_with_persistence_config(service_config_b)
                    .await
                    .expect("second postgres-backed service should create"),
            );

            service_a
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");
            service_b
                .ensure_tenant_exists_async(tenant_id.clone())
                .await
                .expect("second service should load tenant");
            assert_eq!(
                service_b
                    .get_schema_async(tenant_id.clone())
                    .await
                    .expect("empty schema should load"),
                Schema::default()
            );

            service_a
                .set_table_schema_async(tenant_id.clone(), tasks_schema())
                .await
                .expect("schema write should succeed");
            service_a
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("External"))]),
                )
                .await
                .expect("insert should succeed");

            wait_for_value(
                "postgres notification should refresh loaded schema",
                Duration::from_secs(2),
                Duration::from_millis(25),
                || {
                    let service = service_b.clone();
                    let tenant_id = tenant_id.clone();
                    async move {
                        service
                            .get_schema_async(tenant_id)
                            .await
                            .expect("schema should load")
                    }
                },
                |schema| schema.get_table(&tasks_table()).is_some(),
            )
            .await;
            wait_for_mutation_journal_stats(
                &service_b,
                &tenant_id,
                "postgres notification should catch up journal heads",
                |stats| {
                    stats.durable_head == SequenceNumber(1)
                        && stats.applied_head == SequenceNumber(1)
                },
            )
            .await;

            let documents = service_b
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

            tokio::time::timeout(Duration::from_secs(2), service_a.quiesce())
                .await
                .expect("first service should quiesce after reconnect test");
            tokio::time::timeout(Duration::from_secs(2), service_b.quiesce())
                .await
                .expect("second service should quiesce after reconnect test");
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_notifications_load_unloaded_tenants_with_scheduled_work() {
    with_shared_postgres_service_configs(
        |service_config_a, service_config_b, _provider_config| async move {
            let tenant_id = TenantId::new("pg-notify-scheduler").expect("tenant id should build");
            let service_a = Arc::new(
                Service::new_with_persistence_config(service_config_a)
                    .await
                    .expect("first postgres-backed service should create"),
            );
            let service_b = Arc::new(
                Service::new_with_persistence_config(service_config_b)
                    .await
                    .expect("second postgres-backed service should create"),
            );

            service_b
                .load_tenants_with_scheduled_work_async()
                .await
                .expect("initial scheduled-work preload should succeed");
            service_a
                .create_tenant_async(tenant_id.clone())
                .await
                .expect("tenant should create");

            let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
            let scheduler_handle =
                tokio::spawn(crate::run_scheduler(service_b.clone(), shutdown_rx));
            service_a
                .schedule_mutation_async(
                    tenant_id.clone(),
                    ScheduleRequest {
                        run_after_ms: 0,
                        mutation: Mutation::Insert {
                            table: tasks_table(),
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
                "postgres notification should load tenant and execute scheduled work",
                Duration::from_secs(2),
                Duration::from_millis(25),
                || {
                    let service = service_a.clone();
                    let tenant_id = tenant_id.clone();
                    async move {
                        service
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
            wait_for_value(
                "postgres notification should load the scheduled tenant into the second service",
                Duration::from_secs(2),
                Duration::from_millis(25),
                || {
                    let service = service_b.clone();
                    async move { service.loaded_tenant_ids() }
                },
                |tenant_ids| tenant_ids.contains(&tenant_id),
            )
            .await;

            let _ = shutdown_tx.send(true);
            scheduler_handle.await.expect("scheduler should shut down");
            service_a.quiesce().await;
            service_b.quiesce().await;
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_listener_reconnect_recovers_missed_journal_hints() {
    with_postgres_service_config(|service_config, provider_config| async move {
        let tenant_id = TenantId::new("pg-notify-reconnect").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        service
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("service should load tenant");
        assert_eq!(
            service
                .get_schema_async(tenant_id.clone())
                .await
                .expect("empty schema should load"),
            Schema::default()
        );
        let original_listener_pids = list_postgres_hint_listener_pids(&provider_config)
            .await
            .expect("listener pid list should load");
        assert!(
            !original_listener_pids.is_empty(),
            "expected at least one hint listener backend before reconnect drill"
        );
        let original_listener_pids = original_listener_pids.into_iter().collect::<BTreeSet<_>>();

        terminate_postgres_hint_listeners(&provider_config)
            .await
            .expect("listener termination should succeed");

        let provider = PostgresProvider::connect(provider_config.clone())
            .await
            .expect("external provider should connect");
        let opened = provider
            .open_existing_opened_tenant(&tenant_id)
            .await
            .expect("tenant lookup should succeed")
            .expect("tenant should exist");
        opened
            .store
            .insert(&Document {
                table: tasks_table(),
                id: DocumentId::new(),
                fields: serde_json::Map::from_iter([("title".to_string(), json!("Recovered"))]),
                creation_time: Timestamp(100),
            })
            .expect("external document write should succeed");

        wait_for_value(
            "postgres reconnect should restore a new hint listener backend",
            Duration::from_secs(4),
            Duration::from_millis(25),
            || {
                let provider_config = provider_config.clone();
                let original_listener_pids = original_listener_pids.clone();
                async move {
                    let current = list_postgres_hint_listener_pids(&provider_config)
                        .await
                        .expect("listener pid list should load");
                    current
                        .into_iter()
                        .any(|pid| !original_listener_pids.contains(&pid))
                }
            },
            |restored| *restored,
        )
        .await;
        wait_for_value(
            "postgres reconnect should recover missed journal commits",
            Duration::from_secs(4),
            Duration::from_millis(25),
            || {
                let service = service.clone();
                let tenant_id = tenant_id.clone();
                async move {
                    service
                        .mutation_journal_stats_for_testing(&tenant_id)
                        .expect("mutation journal stats should load")
                }
            },
            |stats| {
                stats.durable_head == SequenceNumber(1) && stats.applied_head == SequenceNumber(1)
            },
        )
        .await;
        wait_for_value(
            "postgres reconnect should recover missed writes via journal catch-up",
            Duration::from_secs(4),
            Duration::from_millis(25),
            || {
                let service = service.clone();
                let tenant_id = tenant_id.clone();
                async move {
                    service
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
                        == Some("Recovered")
                })
            },
        )
        .await;

        tokio::time::timeout(Duration::from_secs(2), service.quiesce())
            .await
            .expect("service should quiesce after reconnect test");
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_restart_recovers_due_scheduler_work_after_reopen() {
    with_postgres_service_config(|service_config, _provider_config| async move {
        let tenant_id = TenantId::new("pg-restart-scheduler").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config.clone())
                .await
                .expect("postgres-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        service
            .set_table_schema_async(tenant_id.clone(), tasks_schema())
            .await
            .expect("schema write should succeed");
        let scheduled_job_id = service
            .schedule_mutation_async(
                tenant_id.clone(),
                ScheduleRequest {
                    run_after_ms: 0,
                    mutation: Mutation::Insert {
                        table: tasks_table(),
                        fields: serde_json::Map::from_iter([(
                            "title".to_string(),
                            json!("Recovered after restart"),
                        )]),
                    },
                },
            )
            .await
            .expect("scheduled mutation should persist");
        assert_eq!(
            service
                .list_scheduled_jobs_async(tenant_id.clone())
                .await
                .expect("pending jobs should load")
                .len(),
            1
        );

        tokio::time::timeout(Duration::from_secs(2), service.quiesce())
            .await
            .expect("service should quiesce before restart");
        drop(service);

        let reopened = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should reopen"),
        );
        // External-provider startup recovery preloads scheduled-work tenants
        // and running-job recovery across real Postgres connections. Allow a
        // slightly wider bound here so the default container-backed path stays
        // deterministic under colder connection and statement-cache startup.
        tokio::time::timeout(
            Duration::from_secs(5),
            reopened.recover_scheduled_work_on_startup_async(),
        )
        .await
        .expect("startup scheduled-work recovery should finish promptly after reopen")
        .expect("startup scheduled-work recovery should succeed after reopen");
        tokio::time::timeout(
            Duration::from_secs(15),
            crate::scheduler::tick_async(&reopened),
        )
        .await
        .expect("scheduler tick should finish after restart recovery")
        .expect("scheduler tick should process recovered work after reopen");

        let documents = tokio::time::timeout(
            Duration::from_secs(5),
            reopened.query_documents_async(tenant_id.clone(), query_for("tasks")),
        )
        .await
        .expect("query should finish after restart recovery")
        .expect("query should succeed after restart recovery");
        assert!(documents.iter().any(|document| {
            document
                .fields
                .get("title")
                .and_then(|value| value.as_str())
                == Some("Recovered after restart")
        }));
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(5),
                reopened.get_scheduled_job_result_async(tenant_id.clone(), scheduled_job_id),
            )
            .await
            .expect("scheduled job result should finish after restart recovery")
            .expect("scheduled job result should load after restart recovery")
            .outcome,
            ScheduledJobOutcome::Completed
        );
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(5),
                reopened.list_scheduled_jobs_async(tenant_id),
            )
            .await
            .expect("scheduled jobs should finish after restart recovery")
            .expect("scheduled jobs should load after restart recovery")
            .len(),
            0
        );
        drop(reopened);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_transient_pool_backend_termination_recovers_subsequent_mixed_ops() {
    with_postgres_service_config(|service_config, provider_config| async move {
        let tenant_id = TenantId::new("pg-pool-recover").expect("tenant id should build");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        service
            .set_table_schema_async(tenant_id.clone(), tasks_schema())
            .await
            .expect("schema write should succeed");
        for index in 0..8 {
            service
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!(format!("Seed {index}")),
                    )]),
                )
                .await
                .expect("seed insert should succeed");
        }

        let original_pool_pids = wait_for_value(
            "postgres pool should expose live backends before termination",
            Duration::from_secs(2),
            Duration::from_millis(25),
            || {
                let provider_config = provider_config.clone();
                async move {
                    list_postgres_pool_backend_pids(&provider_config)
                        .await
                        .expect("pool pid list should load")
                }
            },
            |pids| !pids.is_empty(),
        )
        .await
        .into_iter()
        .collect::<BTreeSet<_>>();
        terminate_postgres_pool_backends(&provider_config)
            .await
            .expect("pool backend termination should succeed");

        wait_for_value(
            "postgres pool should recreate terminated backends",
            Duration::from_secs(4),
            Duration::from_millis(25),
            || {
                let provider_config = provider_config.clone();
                let original_pool_pids = original_pool_pids.clone();
                async move {
                    let current = list_postgres_pool_backend_pids(&provider_config)
                        .await
                        .expect("pool pid list should load");
                    current
                        .into_iter()
                        .any(|pid| !original_pool_pids.contains(&pid))
                }
            },
            |restored| *restored,
        )
        .await;

        let recovered_title = format!("Recovered {}", unique_suffix());
        wait_for_value(
            "postgres pooled backend termination should recover mixed ops",
            Duration::from_secs(4),
            Duration::from_millis(50),
            || {
                let service = service.clone();
                let tenant_id = tenant_id.clone();
                let recovered_title = recovered_title.clone();
                async move {
                    let existing = service
                        .query_documents_async(tenant_id.clone(), query_for("tasks"))
                        .await;
                    if let Ok(documents) = existing
                        && documents.iter().any(|document| {
                            document
                                .fields
                                .get("title")
                                .and_then(|value| value.as_str())
                                == Some(recovered_title.as_str())
                        })
                    {
                        return true;
                    }

                    let insert = service
                        .insert_document_async(
                            tenant_id.clone(),
                            tasks_table(),
                            serde_json::Map::from_iter([(
                                "title".to_string(),
                                json!(recovered_title.clone()),
                            )]),
                        )
                        .await;
                    let bootstrap = service
                        .export_durable_journal_bootstrap_async(tenant_id.clone())
                        .await;
                    let query = service
                        .query_documents_async(tenant_id.clone(), query_for("tasks"))
                        .await;
                    match (insert, bootstrap, query) {
                        (Ok(_), Ok(bootstrap), Ok(documents)) => {
                            bootstrap.resume_after.0 >= 9
                                && documents.iter().any(|document| {
                                    document
                                        .fields
                                        .get("title")
                                        .and_then(|value| value.as_str())
                                        == Some(recovered_title.as_str())
                                })
                        }
                        _ => false,
                    }
                }
            },
            |recovered| *recovered,
        )
        .await;

        tokio::time::timeout(Duration::from_secs(2), service.quiesce())
            .await
            .expect("service should quiesce after pooled-backend recovery test");
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_tenant_delete_recreate_cleans_schema_and_runtime_state() {
    with_postgres_service_config(|service_config, provider_config| async move {
        let tenant_id = TenantId::new("pg-tenant-delete").expect("tenant id should build");
        let provider = PostgresProvider::connect(provider_config.clone())
            .await
            .expect("provider should connect");
        let schema_name = provider
            .tenant_schema_name(&tenant_id)
            .expect("schema name should derive");
        let service = Arc::new(
            Service::new_with_persistence_config(service_config)
                .await
                .expect("postgres-backed service should create"),
        );

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        assert!(matches!(
            service.create_tenant_async(tenant_id.clone()).await,
            Err(Error::AlreadyExists(_))
        ));
        assert!(
            postgres_schema_exists(&provider_config, &schema_name)
                .await
                .expect("schema existence should load"),
            "tenant schema should exist after creation"
        );

        service
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("tenant should load");
        assert!(
            service.loaded_tenant_ids().contains(&tenant_id),
            "tenant should be loaded before deletion"
        );

        service
            .delete_tenant_async(tenant_id.clone())
            .await
            .expect("tenant delete should succeed");
        assert!(
            !service.loaded_tenant_ids().contains(&tenant_id),
            "tenant should evict from the loaded registry after deletion"
        );
        assert!(
            !service
                .list_tenants_async()
                .await
                .expect("tenant list should load")
                .contains(&tenant_id),
            "tenant should disappear from provider-owned registry after deletion"
        );
        assert!(
            !postgres_schema_exists(&provider_config, &schema_name)
                .await
                .expect("schema existence should reload"),
            "tenant schema should be removed after deletion"
        );
        assert!(matches!(
            service.delete_tenant_async(tenant_id.clone()).await,
            Err(Error::TenantNotFound(_))
        ));

        service
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should recreate cleanly");
        service
            .ensure_tenant_exists_async(tenant_id.clone())
            .await
            .expect("recreated tenant should load");
        assert!(
            service
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("recreated tenant query should succeed")
                .is_empty(),
            "recreated tenant should start empty after schema cleanup"
        );

        tokio::time::timeout(Duration::from_secs(2), service.quiesce())
            .await
            .expect("service should quiesce after tenant lifecycle test");
    })
    .await;
}

async fn with_postgres_service_config<F, Fut>(test: F)
where
    F: FnOnce(ServicePersistenceConfig, PostgresProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    with_shared_postgres_service_configs(|service_config, _unused, provider_config| async move {
        test(service_config, provider_config).await;
    })
    .await;
}

async fn with_shared_postgres_service_configs<F, Fut>(test: F)
where
    F: FnOnce(ServicePersistenceConfig, ServicePersistenceConfig, PostgresProviderConfig) -> Fut,
    Fut: Future<Output = ()>,
{
    let connection = match test_connection().await {
        Some(connection) => connection,
        None => return,
    };
    let suffix = unique_suffix();
    let metadata_schema = format!("neovex_test_{}", &suffix[..24.min(suffix.len())]);
    let tenant_schema_prefix = format!("tenant_{}_", &suffix[..12.min(suffix.len())]);
    let provider_config = PostgresProviderConfig {
        connection_string: connection.connection_string().to_string(),
        metadata_schema: metadata_schema.clone(),
        tenant_schema_prefix: tenant_schema_prefix.clone(),
        min_connections: Some(1),
        max_connections: Some(4),
    };
    let control_dir_a = tempdir().expect("first temporary control dir should create");
    let control_dir_b = tempdir().expect("second temporary control dir should create");
    let service_config_a = ServicePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            dialect: PersistenceDialect::Postgres,
            topology: PersistenceTopology::ExternalPrimary,
            routing: TenantRoutingConfig::SchemaPerTenant {
                metadata_schema: metadata_schema.clone(),
                tenant_schema_prefix: tenant_schema_prefix.clone(),
            },
            pool: PoolConfig {
                min_connections: Some(1),
                max_connections: Some(4),
            },
            credentials: ProviderCredentials::ConnectionString(
                provider_config.connection_string.clone(),
            ),
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_dir_a.path()),
    };
    let service_config_b = ServicePersistenceConfig {
        tenant_provider: service_config_a.tenant_provider.clone(),
        control_plane: ControlPlaneConfig::embedded_redb(control_dir_b.path()),
    };

    test(service_config_a, service_config_b, provider_config.clone()).await;

    PostgresProvider::connect(provider_config.clone())
        .await
        .expect("postgres provider should connect for cleanup")
        .drop_metadata_schema_for_test()
        .await
        .expect("test metadata schema should drop");
    drop(connection);
    drop(control_dir_a);
    drop(control_dir_b);
}

enum TestConnection {
    External(String),
    Container {
        connection_string: String,
        _container: Box<ContainerAsync<postgres::Postgres>>,
    },
}

impl TestConnection {
    fn connection_string(&self) -> &str {
        match self {
            Self::External(connection_string) => connection_string,
            Self::Container {
                connection_string, ..
            } => connection_string,
        }
    }
}

async fn test_connection() -> Option<TestConnection> {
    if let Ok(connection_string) = env::var(TEST_POSTGRES_URL_ENV) {
        return Some(TestConnection::External(connection_string));
    }

    let container = match postgres::Postgres::default().start().await {
        Ok(container) => container,
        Err(error) => {
            eprintln!(
                "skipping postgres engine test because no explicit Postgres URL was provided and container startup failed: {error}"
            );
            return None;
        }
    };
    let host = container
        .get_host()
        .await
        .expect("container host should resolve");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("container port should resolve");
    Some(TestConnection::Container {
        connection_string: format!(
            "host={host} port={port} user=postgres password=postgres dbname=postgres"
        ),
        _container: Box::new(container),
    })
}

async fn terminate_postgres_hint_listeners(
    config: &PostgresProviderConfig,
) -> neovex_core::Result<()> {
    let terminated = with_postgres_activity_client(
        config,
        PostgresProvider::notification_listener_application_name,
        |client, application_name| async move {
            client
                .execute(
                    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = $1",
                    &[&application_name],
                )
                .await
                .map_err(|error| neovex_core::Error::Internal(error.to_string()))
        },
    )
    .await?;
    assert!(
        terminated > 0,
        "expected at least one listener backend to terminate"
    );
    Ok(())
}

async fn list_postgres_hint_listener_pids(
    config: &PostgresProviderConfig,
) -> neovex_core::Result<Vec<i32>> {
    with_postgres_activity_client(
        config,
        PostgresProvider::notification_listener_application_name,
        |client, application_name| async move {
            let rows = client
                .query(
                    "SELECT pid FROM pg_stat_activity WHERE application_name = $1 ORDER BY pid",
                    &[&application_name],
                )
                .await
                .map_err(|error| neovex_core::Error::Internal(error.to_string()))?;
            Ok(rows.into_iter().map(|row| row.get::<_, i32>(0)).collect())
        },
    )
    .await
}

async fn terminate_postgres_pool_backends(
    config: &PostgresProviderConfig,
) -> neovex_core::Result<()> {
    let terminated = with_postgres_activity_client(
        config,
        PostgresProvider::pool_application_name,
        |client, application_name| async move {
            client
                .execute(
                    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = $1",
                    &[&application_name],
                )
                .await
                .map_err(|error| neovex_core::Error::Internal(error.to_string()))
        },
    )
    .await?;
    assert!(
        terminated > 0,
        "expected at least one pooled backend to terminate"
    );
    Ok(())
}

async fn list_postgres_pool_backend_pids(
    config: &PostgresProviderConfig,
) -> neovex_core::Result<Vec<i32>> {
    with_postgres_activity_client(
        config,
        PostgresProvider::pool_application_name,
        |client, application_name| async move {
            let rows = client
                .query(
                    "SELECT pid FROM pg_stat_activity WHERE application_name = $1 ORDER BY pid",
                    &[&application_name],
                )
                .await
                .map_err(|error| neovex_core::Error::Internal(error.to_string()))?;
            Ok(rows.into_iter().map(|row| row.get::<_, i32>(0)).collect())
        },
    )
    .await
}

async fn postgres_schema_exists(
    config: &PostgresProviderConfig,
    schema_name: &str,
) -> neovex_core::Result<bool> {
    let schema_name = schema_name.to_string();
    with_postgres_activity_client(
        config,
        PostgresProvider::pool_application_name,
        move |client, _application_name| async move {
            client
                .query_opt(
                    "SELECT 1 FROM information_schema.schemata WHERE schema_name = $1",
                    &[&schema_name],
                )
                .await
                .map(|row| row.is_some())
                .map_err(|error| neovex_core::Error::Internal(error.to_string()))
        },
    )
    .await
}

async fn with_postgres_activity_client<F, Fut, T>(
    config: &PostgresProviderConfig,
    application_name_selector: fn(&PostgresProvider) -> &str,
    action: F,
) -> neovex_core::Result<T>
where
    F: FnOnce(tokio_postgres::Client, String) -> Fut,
    Fut: Future<Output = neovex_core::Result<T>>,
{
    let provider = PostgresProvider::connect(config.clone()).await?;
    let application_name = application_name_selector(&provider).to_string();
    let (client, connection) = tokio_postgres::connect(&config.connection_string, NoTls)
        .await
        .map_err(|error| neovex_core::Error::Internal(error.to_string()))?;
    let connection_task = tokio::spawn(async move {
        let _ = connection.await;
    });
    let result = action(client, application_name).await;
    connection_task.abort();
    result
}

fn unique_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    let counter = TEST_SUFFIX_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{counter:08x}{:x}{timestamp:x}", std::process::id())
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
