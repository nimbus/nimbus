use std::collections::BTreeSet;
use std::sync::Arc;

use neovex_core::{Query, SequenceNumber, TableName};
use neovex_testing::ServiceFixture;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};

use crate::{Service, SubscriptionUpdate};

fn tasks_table() -> TableName {
    TableName::new("tasks").expect("table name should be valid")
}

fn query_for(table: &str) -> Query {
    Query {
        table: TableName::new(table).expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    }
}

fn subscription_channel() -> (
    mpsc::Sender<SubscriptionUpdate>,
    mpsc::Receiver<SubscriptionUpdate>,
) {
    mpsc::channel(16)
}

async fn wait_for_subscription_delivery_stats(
    service: &Arc<Service>,
    tenant_id: &neovex_core::TenantId,
    description: &str,
    predicate: impl Fn(&crate::tenant::SubscriptionDeliveryStats) -> bool,
) -> crate::tenant::SubscriptionDeliveryStats {
    let started_at = tokio::time::Instant::now();
    loop {
        let stats = service
            .subscription_delivery_stats_for_testing(tenant_id)
            .expect("subscription delivery stats should load");
        if predicate(&stats) {
            return stats;
        }
        assert!(
            started_at.elapsed() < Duration::from_secs(1),
            "timed out waiting for {description}; last subscription delivery stats: {stats:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn service_mutation_returns_while_subscription_delivery_worker_is_blocked() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
        )
        .expect("seed insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let _subscription = service
        .subscribe(&tenant_id, query_for("tasks"), "async-sub".to_string(), tx)
        .expect("subscribe should succeed");
    let _ = rx
        .recv()
        .await
        .expect("initial subscription update should arrive");

    let pause = service
        .subscription_delivery_pause_handle_for_testing(&tenant_id)
        .expect("pause handle should load");
    pause.arm();

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should return while the worker is blocked");

    assert!(
        pause.wait_until_entered(Duration::from_secs(1)),
        "worker should begin processing the queued delivery"
    );
    assert!(
        timeout(Duration::from_millis(150), rx.recv())
            .await
            .is_err(),
        "blocked worker should prevent the reactive result from arriving yet"
    );

    pause.release();

    let update = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("reactive update should arrive after the worker is released")
        .expect("reactive update channel should stay open");
    match update {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            let commit = snapshot
                .commit
                .expect("single-commit delivery should retain commit metadata");
            assert_eq!(commit.sequence, SequenceNumber(2));
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("After"));
        }
        other => panic!("unexpected subscription update: {other:?}"),
    }

    let stats = wait_for_subscription_delivery_stats(
        &service,
        &tenant_id,
        "blocked worker reevaluation stats",
        |stats| stats.reevaluation_count >= 1 && stats.total_reevaluation_nanos > 0,
    )
    .await;
    assert_eq!(stats.queue_depth, 0);
    assert_eq!(
        stats.queue_capacity,
        crate::tenant::DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY
    );
    assert_eq!(stats.oldest_queue_age_nanos, 0);
    assert!(stats.worker_running);
    assert_eq!(stats.worker_start_count, 1);
    assert_eq!(stats.worker_restart_count, 0);
    assert!(stats.reevaluation_count >= 1);
    assert!(stats.total_reevaluation_nanos > 0);
}

#[tokio::test]
async fn subscription_delivery_queue_overflow_falls_back_without_regressing_monotonicity() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("seed"))]),
        )
        .expect("seed insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let _subscription = service
        .subscribe(
            &tenant_id,
            query_for("tasks"),
            "overflow-sub".to_string(),
            tx,
        )
        .expect("subscribe should succeed");
    let _ = rx
        .recv()
        .await
        .expect("initial subscription update should arrive");

    service
        .set_subscription_delivery_queue_capacity_for_testing(&tenant_id, 1)
        .expect("queue capacity should be configurable for the test");
    let pause = service
        .subscription_delivery_pause_handle_for_testing(&tenant_id)
        .expect("pause handle should load");
    pause.arm();

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
        )
        .expect("first update should succeed");
    assert!(
        pause.wait_until_entered(Duration::from_secs(1)),
        "worker should block on the first queued delivery"
    );

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("second"))]),
        )
        .expect("second update should queue behind the blocked delivery");

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("third"))]),
        )
        .expect("overflow update should fall back without failing");

    let latest = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("overflow fallback should still deliver the latest visible state")
        .expect("subscription channel should stay open");
    match latest {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            let commit = snapshot
                .commit
                .expect("single-commit fallback should retain commit metadata");
            assert_eq!(commit.sequence, SequenceNumber(4));
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("third"));
        }
        other => panic!("unexpected overflow subscription update: {other:?}"),
    }

    pause.release();

    assert!(
        timeout(Duration::from_millis(200), rx.recv())
            .await
            .is_err(),
        "older queued deliveries should be skipped once a newer sequence has already been delivered"
    );

    let stats = wait_for_subscription_delivery_stats(
        &service,
        &tenant_id,
        "overflow delivery stats",
        |stats| {
            stats.reevaluation_count >= 1
                && stats.queue_level_merge_count + stats.coalesced_work_count >= 2
        },
    )
    .await;
    assert_eq!(stats.queue_depth, 0);
    assert_eq!(stats.queue_capacity, 1);
    assert_eq!(stats.oldest_queue_age_nanos, 0);
    assert!(stats.worker_running);
    assert_eq!(stats.worker_start_count, 1);
    assert_eq!(stats.worker_restart_count, 0);
    assert_eq!(stats.overflow_sync_fallback_count, 1);
    assert_eq!(stats.queue_level_merge_count, 1);
    assert!(
        stats.queue_level_merge_count + stats.coalesced_work_count >= 2,
        "superseded queued deliveries should be accounted for by queue merges or stale-delivery skips"
    );
    assert!(stats.reevaluation_count >= 1);
}

#[tokio::test]
async fn subscription_delivery_queue_merge_coalesces_overlapping_work_items() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("seed"))]),
        )
        .expect("seed insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let _subscription = service
        .subscribe(
            &tenant_id,
            query_for("tasks"),
            "queue-merge-sub".to_string(),
            tx,
        )
        .expect("subscribe should succeed");
    let _ = rx
        .recv()
        .await
        .expect("initial subscription update should arrive");

    let pause = service
        .subscription_delivery_pause_handle_for_testing(&tenant_id)
        .expect("pause handle should load");
    pause.arm();

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
        )
        .expect("first update should succeed");
    assert!(
        pause.wait_until_entered(Duration::from_secs(1)),
        "worker should pause after popping the first delivery item"
    );

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("second"))]),
        )
        .expect("second update should enqueue a later delivery item");

    pause.release();

    let update = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("merged subscription update should arrive")
        .expect("subscription channel should stay open");
    match update {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert!(
                snapshot.commit.is_none(),
                "queue-level merged deliveries should omit per-commit metadata"
            );
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("second"));
        }
        other => panic!("unexpected merged subscription update: {other:?}"),
    }

    assert!(
        timeout(Duration::from_millis(200), rx.recv())
            .await
            .is_err(),
        "merged delivery work should collapse the redundant second queue item"
    );

    let stats =
        wait_for_subscription_delivery_stats(&service, &tenant_id, "queue merge stats", |stats| {
            stats.queue_depth == 0
                && stats.queue_level_merge_count == 1
                && stats.coalesced_work_count == 0
                && stats.reevaluation_count == 1
                && stats.total_reevaluation_nanos > 0
        })
        .await;
    assert_eq!(stats.queue_depth, 0);
    assert_eq!(stats.queue_level_merge_count, 1);
    assert_eq!(stats.coalesced_work_count, 0);
    assert_eq!(stats.reevaluation_count, 1);
    assert!(stats.total_reevaluation_nanos > 0);
}

#[tokio::test]
async fn journal_batch_coalesces_subscription_delivery_into_one_update() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (tx, mut rx) = subscription_channel();
    let _subscription = service
        .subscribe(&tenant_id, query_for("tasks"), "batch-sub".to_string(), tx)
        .expect("subscribe should succeed");
    let _ = rx
        .recv()
        .await
        .expect("initial subscription update should arrive");

    let pause = service
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let first_insert = {
        let service = Arc::clone(&service);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("Task 1"))]),
                )
                .await
        })
    };

    let pause_wait = pause.clone();
    assert!(
        tokio::task::spawn_blocking(move || pause_wait.wait_until_entered(Duration::from_secs(1)))
            .await
            .expect("pause wait should join"),
        "journal worker should pause before draining the queued batch"
    );

    let second_insert = {
        let service = Arc::clone(&service);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("Task 2"))]),
                )
                .await
        })
    };
    let third_insert = {
        let service = Arc::clone(&service);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("Task 3"))]),
                )
                .await
        })
    };

    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    pause.release();

    let inserted_ids = timeout(Duration::from_secs(1), async {
        [
            first_insert
                .await
                .expect("first insert task should join")
                .expect("first insert should succeed"),
            second_insert
                .await
                .expect("second insert task should join")
                .expect("second insert should succeed"),
            third_insert
                .await
                .expect("third insert task should join")
                .expect("third insert should succeed"),
        ]
    })
    .await
    .expect("queued inserts should complete once the journal worker is released");
    assert_eq!(
        inserted_ids.iter().cloned().collect::<BTreeSet<_>>().len(),
        3,
        "all queued inserts should complete successfully"
    );

    let update = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("coalesced subscription update should arrive")
        .expect("subscription channel should remain open");
    match update {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert!(
                snapshot.commit.is_none(),
                "multi-commit coalesced deliveries should omit per-commit metadata"
            );
            assert!(
                snapshot.deleted_documents.is_empty(),
                "insert-only batches should not surface deleted documents"
            );
            assert_eq!(data.len(), 3);
            let titles = data
                .into_iter()
                .map(|document| {
                    document["title"]
                        .as_str()
                        .expect("subscription payload should include a title")
                        .to_string()
                })
                .collect::<BTreeSet<_>>();
            assert_eq!(
                titles,
                BTreeSet::from([
                    "Task 1".to_string(),
                    "Task 2".to_string(),
                    "Task 3".to_string(),
                ])
            );
        }
        other => panic!("unexpected coalesced subscription update: {other:?}"),
    }

    assert!(
        timeout(Duration::from_millis(200), rx.recv())
            .await
            .is_err(),
        "the journal batch should emit a single coalesced subscription wakeup"
    );

    let stats = wait_for_subscription_delivery_stats(
        &service,
        &tenant_id,
        "journal batch coalescing stats",
        |stats| {
            stats.queue_depth == 0
                && stats.coalesced_batch_count == 1
                && stats.coalesced_commit_count == 3
                && stats.merged_subscription_wakeup_count == 2
                && stats.coalesced_work_count == 0
                && stats.reevaluation_count == 1
                && stats.total_reevaluation_nanos > 0
        },
    )
    .await;
    assert_eq!(stats.queue_depth, 0);
    assert_eq!(
        stats.queue_capacity,
        crate::tenant::DEFAULT_SUBSCRIPTION_WORK_QUEUE_CAPACITY
    );
    assert_eq!(stats.coalesced_batch_count, 1);
    assert_eq!(stats.coalesced_commit_count, 3);
    assert_eq!(stats.merged_subscription_wakeup_count, 2);
    assert_eq!(stats.coalesced_work_count, 0);
    assert_eq!(stats.reevaluation_count, 1);
    assert!(stats.total_reevaluation_nanos > 0);
}
