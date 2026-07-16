//! Hermitage named-anomaly conformance matrix for Nimbus execution units.
//!
//! The scenarios follow Turso's Hermitage suite shape, with the material
//! difference that Nimbus targets serializability: G2-item and G2 must abort a
//! participant. `PRE_ASSIGN` deterministically holds one prepared transaction
//! while its competitor commits; no test relies on timing or sleeps.

use super::*;
use crate::engine::execution_units::{CommitFaultHandle, MutationExecutionUnit};
use nimbus_core::{CommitEntry, Filter, FilterOp, Query, Result, TableName};

type CommitTask = tokio::task::JoinHandle<Result<Option<CommitEntry>>>;

struct HermitageFixture {
    _fixture: EngineFixture<Engine>,
    engine: Arc<Engine>,
    tenant_id: TenantId,
    table: TableName,
    first_id: DocumentId,
    second_id: DocumentId,
}

impl HermitageFixture {
    fn new(name: &str) -> Self {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant(name, Engine::create_tenant);
        let table = messages_table(&format!("{name}_registers"));
        let first_id = engine
            .insert_document(&tenant_id, table.clone(), value_fields(10))
            .expect("first register should seed");
        let second_id = engine
            .insert_document(&tenant_id, table.clone(), value_fields(20))
            .expect("second register should seed");
        Self {
            _fixture: fixture,
            engine,
            tenant_id,
            table,
            first_id,
            second_id,
        }
    }

    fn begin(&self) -> Arc<MutationExecutionUnit> {
        self.engine
            .begin_mutation_execution_unit(self.tenant_id.clone(), PrincipalContext::anonymous())
            .expect("execution unit should begin")
    }

    fn committed_value(&self, id: &DocumentId) -> i64 {
        self.engine
            .get_document(&self.tenant_id, &self.table, id.clone())
            .expect("committed register should exist")
            .get_field("value")
            .and_then(serde_json::Value::as_i64)
            .expect("register value should be an integer")
    }
}

fn value_fields(value: i64) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([("value".to_string(), json!(value))])
}

fn read_value(unit: &MutationExecutionUnit, table: &TableName, id: &DocumentId) -> i64 {
    unit.get_document(table, id.clone())
        .expect("execution-unit read should succeed")
        .expect("register should exist in the snapshot")
        .get_field("value")
        .and_then(serde_json::Value::as_i64)
        .expect("register value should be an integer")
}

fn update_value(unit: &MutationExecutionUnit, table: &TableName, id: &DocumentId, value: i64) {
    unit.update_document(table.clone(), id.clone(), value_fields(value))
        .expect("register update should stage");
}

fn query_value(unit: &MutationExecutionUnit, table: &TableName, op: FilterOp, value: i64) -> usize {
    unit.query_documents_cancellable(
        &Query {
            table: table.clone(),
            filters: vec![Filter {
                field: "value".to_string(),
                op,
                value: json!(value),
            }],
            order: None,
            limit: None,
        },
        &mut || Ok(()),
    )
    .expect("predicate read should succeed")
    .len()
}

async fn pause_before_assign(
    engine: &Arc<Engine>,
    unit: Arc<MutationExecutionUnit>,
) -> (CommitFaultHandle, CommitTask) {
    let faults = engine.commit_fault_handle_for_testing();
    faults.arm(labels::PRE_ASSIGN);
    let commit = tokio::task::spawn_blocking(move || unit.commit());
    let reached = tokio::task::spawn_blocking({
        let faults = faults.clone();
        move || faults.wait_until_entered(labels::PRE_ASSIGN, Duration::from_secs(5))
    })
    .await
    .expect("pause wait should join");
    assert!(reached, "commit should reach PRE_ASSIGN deterministically");
    assert!(!commit.is_finished(), "commit should remain paused");
    (faults, commit)
}

async fn release_commit(
    faults: CommitFaultHandle,
    commit: CommitTask,
) -> Result<Option<CommitEntry>> {
    faults.release(labels::PRE_ASSIGN);
    timeout(Duration::from_secs(5), commit)
        .await
        .expect("released commit should finish")
        .expect("commit task should join")
}

async fn release_and_expect_conflict(faults: CommitFaultHandle, commit: CommitTask) {
    let error = release_commit(faults, commit)
        .await
        .expect_err("serializability must reject the losing transaction");
    assert!(
        matches!(error, Error::Conflict { .. }),
        "loser should receive Error::Conflict, got {error:?}"
    );
}

fn commit_success(unit: &MutationExecutionUnit) {
    unit.commit()
        .expect("winning transaction should commit")
        .expect("winning transaction should contain writes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermitage_g0_write_cycles_prevented() {
    let h = HermitageFixture::new("hermitage-g0");
    let first = h.begin();
    let second = h.begin();
    update_value(&first, &h.table, &h.first_id, 11);
    update_value(&first, &h.table, &h.second_id, 21);
    update_value(&second, &h.table, &h.first_id, 12);
    update_value(&second, &h.table, &h.second_id, 22);

    let (faults, first_commit) = pause_before_assign(&h.engine, first).await;
    commit_success(&second);
    release_and_expect_conflict(faults, first_commit).await;
    assert_eq!(
        (
            h.committed_value(&h.first_id),
            h.committed_value(&h.second_id)
        ),
        (12, 22)
    );
}

#[test]
fn hermitage_g1a_aborted_read_prevented() {
    let h = HermitageFixture::new("hermitage-g1a");
    let aborted = h.begin();
    update_value(&aborted, &h.table, &h.first_id, 101);

    // Execution-unit writes live only in the unit's private staged map. There
    // is no partial-publish or rollback path: dropping an uncommitted unit
    // discards that map, so an aborted version is structurally unreadable.
    let observer = h.begin();
    assert_eq!(read_value(&observer, &h.table, &h.first_id), 10);
    drop(aborted);
    assert_eq!(h.committed_value(&h.first_id), 10);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermitage_g1b_intermediate_read_prevented() {
    let h = HermitageFixture::new("hermitage-g1b");
    let writer = h.begin();
    update_value(&writer, &h.table, &h.first_id, 101);
    assert_eq!(read_value(&h.begin(), &h.table, &h.first_id), 10);
    update_value(&writer, &h.table, &h.first_id, 11);

    // Repeated writes collapse into one StagedWriteEntry. PRE_ASSIGN proves
    // that even a fully prepared commit exposes neither the intermediate 101
    // nor final 11 before the single atomic persistence call.
    let (faults, commit) = pause_before_assign(&h.engine, writer).await;
    assert_eq!(read_value(&h.begin(), &h.table, &h.first_id), 10);
    release_commit(faults, commit)
        .await
        .expect("writer should commit")
        .expect("writer should contain its final write");
    assert_eq!(h.committed_value(&h.first_id), 11);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermitage_g1c_circular_information_flow_prevented() {
    let h = HermitageFixture::new("hermitage-g1c");
    let first = h.begin();
    let second = h.begin();
    update_value(&first, &h.table, &h.first_id, 11);
    update_value(&second, &h.table, &h.second_id, 22);
    assert_eq!(read_value(&first, &h.table, &h.second_id), 20);
    assert_eq!(read_value(&second, &h.table, &h.first_id), 10);

    let (faults, first_commit) = pause_before_assign(&h.engine, first).await;
    commit_success(&second);
    release_and_expect_conflict(faults, first_commit).await;
    assert_eq!(
        (
            h.committed_value(&h.first_id),
            h.committed_value(&h.second_id)
        ),
        (10, 22)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermitage_otv_observed_transaction_vanishes_prevented() {
    let h = HermitageFixture::new("hermitage-otv");
    let vanishing = h.begin();
    update_value(&vanishing, &h.table, &h.first_id, 11);
    update_value(&vanishing, &h.table, &h.second_id, 19);
    let observer = h.begin();
    let (faults, vanishing_commit) = pause_before_assign(&h.engine, vanishing).await;

    let winner = h.begin();
    update_value(&winner, &h.table, &h.first_id, 12);
    update_value(&winner, &h.table, &h.second_id, 18);
    commit_success(&winner);
    assert_eq!(read_value(&observer, &h.table, &h.first_id), 10);
    assert_eq!(read_value(&observer, &h.table, &h.second_id), 20);
    release_and_expect_conflict(faults, vanishing_commit).await;

    assert_eq!(
        (
            h.committed_value(&h.first_id),
            h.committed_value(&h.second_id)
        ),
        (12, 18)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermitage_pmp_read_prevented() {
    let h = HermitageFixture::new("hermitage-pmp-read");
    let reader = h.begin();
    assert_eq!(query_value(&reader, &h.table, FilterOp::Eq, 30), 0);
    reader
        .insert_document(h.table.clone(), value_fields(99))
        .expect("reader marker write should stage");
    let (faults, reader_commit) = pause_before_assign(&h.engine, reader).await;

    let phantom = h.begin();
    phantom
        .insert_document(h.table.clone(), value_fields(30))
        .expect("matching phantom should stage");
    commit_success(&phantom);
    release_and_expect_conflict(faults, reader_commit).await;
    assert_eq!(query_value(&h.begin(), &h.table, FilterOp::Eq, 30), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermitage_pmp_write_prevented() {
    let h = HermitageFixture::new("hermitage-pmp-write");
    let updater = h.begin();
    assert_eq!(query_value(&updater, &h.table, FilterOp::Gte, 10), 2);
    update_value(&updater, &h.table, &h.first_id, 20);
    update_value(&updater, &h.table, &h.second_id, 30);

    let deleter = h.begin();
    assert_eq!(query_value(&deleter, &h.table, FilterOp::Eq, 20), 1);
    deleter
        .delete_document(h.table.clone(), h.second_id.clone())
        .expect("predicate-selected delete should stage");
    let (faults, updater_commit) = pause_before_assign(&h.engine, updater).await;
    commit_success(&deleter);
    release_and_expect_conflict(faults, updater_commit).await;
    assert!(matches!(
        h.engine
            .get_document(&h.tenant_id, &h.table, h.second_id.clone()),
        Err(Error::DocumentNotFound(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermitage_p4_lost_update_prevented() {
    let h = HermitageFixture::new("hermitage-p4");
    let first = h.begin();
    let second = h.begin();
    assert_eq!(read_value(&first, &h.table, &h.first_id), 10);
    assert_eq!(read_value(&second, &h.table, &h.first_id), 10);
    update_value(&first, &h.table, &h.first_id, 11);
    update_value(&second, &h.table, &h.first_id, 12);

    let (faults, first_commit) = pause_before_assign(&h.engine, first).await;
    commit_success(&second);
    release_and_expect_conflict(faults, first_commit).await;
    assert_eq!(h.committed_value(&h.first_id), 12);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermitage_g_single_read_skew_prevented() {
    let h = HermitageFixture::new("hermitage-g-single");
    let reader = h.begin();
    assert_eq!(read_value(&reader, &h.table, &h.first_id), 10);

    let writer = h.begin();
    update_value(&writer, &h.table, &h.first_id, 12);
    update_value(&writer, &h.table, &h.second_id, 18);
    let (faults, writer_commit) = pause_before_assign(&h.engine, writer).await;
    assert_eq!(read_value(&reader, &h.table, &h.second_id), 20);
    release_commit(faults, writer_commit)
        .await
        .expect("writer should commit")
        .expect("writer should contain writes");

    assert_eq!(read_value(&reader, &h.table, &h.first_id), 10);
    assert_eq!(read_value(&reader, &h.table, &h.second_id), 20);
    reader
        .insert_document(h.table.clone(), value_fields(99))
        .expect("reader marker write should stage");
    let error = reader
        .commit()
        .expect_err("stale read transaction must not serialize after its writer");
    assert!(matches!(error, Error::Conflict { .. }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermitage_g2_item_write_skew_prevented() {
    let h = HermitageFixture::new("hermitage-g2-item");
    let first = h.begin();
    let second = h.begin();
    for unit in [&first, &second] {
        assert_eq!(read_value(unit, &h.table, &h.first_id), 10);
        assert_eq!(read_value(unit, &h.table, &h.second_id), 20);
    }
    update_value(&first, &h.table, &h.first_id, 11);
    update_value(&second, &h.table, &h.second_id, 21);

    let (faults, first_commit) = pause_before_assign(&h.engine, first).await;
    commit_success(&second);
    release_and_expect_conflict(faults, first_commit).await;
    assert_eq!(
        (
            h.committed_value(&h.first_id),
            h.committed_value(&h.second_id)
        ),
        (10, 21)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hermitage_g2_anti_dependency_cycles_prevented() {
    let h = HermitageFixture::new("hermitage-g2");
    let first = h.begin();
    let second = h.begin();
    assert_eq!(query_value(&first, &h.table, FilterOp::Gt, 100), 0);
    assert_eq!(query_value(&second, &h.table, FilterOp::Gt, 100), 0);
    first
        .insert_document(h.table.clone(), value_fields(101))
        .expect("first cycle insert should stage");
    second
        .insert_document(h.table.clone(), value_fields(102))
        .expect("second cycle insert should stage");

    let (faults, first_commit) = pause_before_assign(&h.engine, first).await;
    commit_success(&second);
    release_and_expect_conflict(faults, first_commit).await;
    assert_eq!(query_value(&h.begin(), &h.table, FilterOp::Gt, 100), 1);
}
