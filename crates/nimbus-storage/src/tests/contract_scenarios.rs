//! Scenario bodies that every provider in the qualification matrix runs.
//!
//! `provider_scenarios` and `sql_pair_scenarios` are shared only among the
//! remote providers, because their bounds name remote-only seams. The bodies
//! here bind to the traits every tenant store implements, so the embedded
//! providers and the remote providers execute the same assertions rather than
//! keeping a private copy each.

use nimbus_core::Result;

use super::{
    Document, DocumentId, SequenceNumber, TableId, TableName, TenantEventRecord, Timestamp,
    WriteOp, WriteOpType,
};
use crate::store::JournalProgress;
use crate::{DurableJournal, MaterializedPosition, TenantPointRead, TenantPointWrite};

/// Reads the materialized position a store currently publishes.
///
/// `export_materialized_journal_snapshot` is an inherent method on each store
/// rather than a trait method, so the shared bodies below need this test-only
/// bridge to stay provider-independent. It adds no production surface.
pub(crate) trait MaterializedPositionOracle {
    fn export_materialized_position(&self) -> Result<MaterializedPosition>;
}

macro_rules! impl_materialized_position_oracle {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl MaterializedPositionOracle for $ty {
                fn export_materialized_position(&self) -> Result<MaterializedPosition> {
                    self.export_materialized_journal_snapshot()?
                        .materialized_position()
                }
            }
        )+
    };
}

impl_materialized_position_oracle!(
    crate::TenantStore,
    crate::SqliteTenantStore,
    crate::MemoryTenantStore,
);

#[cfg(feature = "postgres")]
impl_materialized_position_oracle!(crate::postgres::PostgresTenantStore);
#[cfg(feature = "mysql")]
impl_materialized_position_oracle!(crate::mysql::MySqlTenantStore);
#[cfg(feature = "libsql")]
impl_materialized_position_oracle!(crate::libsql::LibsqlReplicaTenantStore);

/// An insert, an update, and a delete must each advance the journal by exactly
/// one sequence, leave the durable head and the applied head equal, and appear
/// in the commit log in that order.
pub(crate) fn exercise_journal_progress_round_trip<S>(store: &S, table_name: &str)
where
    S: TenantPointRead + TenantPointWrite + DurableJournal,
{
    let document = super::sample_document(table_name, "First");

    let insert = store
        .insert_document(&document)
        .expect("insert should commit");
    assert_eq!(insert.sequence, SequenceNumber(1));

    let update = store
        .update_document_validated(
            &document.table,
            &document.id,
            &serde_json::Map::from_iter([("title".to_string(), serde_json::json!("Renamed"))]),
            |_, _| Ok(()),
        )
        .expect("update should commit");
    assert_eq!(update.sequence, SequenceNumber(2));
    assert_eq!(
        store
            .get(&document.table, &document.id)
            .expect("point read should succeed")
            .expect("updated document should exist")
            .fields
            .get("title"),
        Some(&serde_json::json!("Renamed"))
    );

    let (delete, removed) = store
        .delete_document_validated(&document.table, &document.id, |_| Ok(()))
        .expect("delete should commit");
    assert_eq!(delete.sequence, SequenceNumber(3));
    assert_eq!(removed.id, document.id);

    assert_eq!(
        store
            .journal_progress()
            .expect("journal progress should read"),
        JournalProgress {
            durable_head: SequenceNumber(3),
            applied_head: SequenceNumber(3),
        },
        "an applied write must move the durable head and the applied head together"
    );
    assert_eq!(
        store
            .latest_sequence()
            .expect("latest sequence should read"),
        SequenceNumber(3)
    );
    assert_eq!(
        store
            .applied_sequence()
            .expect("applied sequence should read"),
        SequenceNumber(3)
    );

    let commits = store
        .read_commit_log_from(SequenceNumber(1))
        .expect("commit log should read");
    assert_eq!(commits.len(), 3);
    assert_eq!(commits[0].writes[0].op_type, WriteOpType::Insert);
    assert_eq!(commits[1].writes[0].op_type, WriteOpType::Update);
    assert_eq!(commits[2].writes[0].op_type, WriteOpType::Delete);
}

/// The fixed logical state every provider replays for the position-parity row.
///
/// Every identifier and timestamp is pinned, so the only thing that can differ
/// between two providers replaying this batch is the provider itself.
const PARITY_TABLE: &str = "position_parity_tasks";
const PARITY_TABLE_ID: &str = "01JQPARITYTABLE0000000000";
const PARITY_FIRST_DOCUMENT: &str = "01JQPARITYDOCUMENT000001";
const PARITY_SECOND_DOCUMENT: &str = "01JQPARITYDOCUMENT000002";

fn parity_records() -> Vec<TenantEventRecord> {
    let table = TableName::new(PARITY_TABLE).expect("table name should be valid");
    let table_id: TableId = PARITY_TABLE_ID.parse().expect("table id should be valid");
    let first_id = DocumentId::from_key(PARITY_FIRST_DOCUMENT).expect("document id should build");
    let second_id = DocumentId::from_key(PARITY_SECOND_DOCUMENT).expect("document id should build");

    let first = Document::with_id_at(
        first_id.clone(),
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), serde_json::json!("first"))]),
        Timestamp(1_000),
    );
    let mut renamed = first.clone();
    renamed
        .fields
        .insert("title".to_string(), serde_json::json!("renamed"));
    renamed.update_time = Timestamp(1_001);
    let second = Document::with_id_at(
        second_id.clone(),
        table.clone(),
        serde_json::Map::from_iter([("title".to_string(), serde_json::json!("second"))]),
        Timestamp(1_002),
    );

    vec![
        parity_record(
            SequenceNumber(1),
            Timestamp(1_000),
            &table,
            &table_id,
            WriteOpType::Insert,
            &first_id,
            None,
            Some(first.clone()),
        ),
        parity_record(
            SequenceNumber(2),
            Timestamp(1_001),
            &table,
            &table_id,
            WriteOpType::Update,
            &first_id,
            Some(first),
            Some(renamed),
        ),
        parity_record(
            SequenceNumber(3),
            Timestamp(1_002),
            &table,
            &table_id,
            WriteOpType::Insert,
            &second_id,
            None,
            Some(second),
        ),
    ]
}

#[allow(clippy::too_many_arguments)]
fn parity_record(
    sequence: SequenceNumber,
    timestamp: Timestamp,
    table: &TableName,
    table_id: &TableId,
    op_type: WriteOpType,
    doc_id: &DocumentId,
    previous: Option<Document>,
    current: Option<Document>,
) -> TenantEventRecord {
    TenantEventRecord::new(
        sequence,
        timestamp,
        vec![WriteOp {
            table: table.clone(),
            table_id: table_id.clone(),
            op_type,
            doc_id: doc_id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous,
            current,
        }],
        None,
    )
    .expect("parity record should build")
}

/// Replaying one pinned batch must leave every provider at the same
/// materialized position, and exporting twice must return the same value.
///
/// The caller compares the returned position with
/// [`reference_materialized_position`], so a provider whose digest depends on
/// its own storage layout, its own iteration order, or its own clock fails.
pub(crate) fn exercise_materialized_position_is_provider_independent<S>(
    store: &S,
) -> MaterializedPosition
where
    S: DurableJournal + MaterializedPositionOracle,
{
    let records = parity_records();
    store
        .append_durable_records_batch(&records)
        .expect("pinned records should append");
    store
        .recover_durable_journal()
        .expect("pinned records should replay");

    let position = store
        .export_materialized_position()
        .expect("materialized position should export");
    assert_eq!(
        position.applied_sequence(),
        SequenceNumber(3),
        "the position must report the sequence the provider actually applied"
    );
    assert_eq!(
        position,
        store
            .export_materialized_position()
            .expect("materialized position should export again"),
        "two exports of one unchanged state must return one position"
    );

    position
}

/// The position an in-memory provider reaches from the same pinned batch.
pub(crate) fn reference_materialized_position() -> MaterializedPosition {
    let store = crate::MemoryTenantStore::new();
    exercise_materialized_position_is_provider_independent(&store)
}

/// Durable-but-unapplied records must stay invisible to reads until
/// `recover_durable_journal` replays them, and replaying must move the applied
/// head to the durable head exactly once.
///
/// This is the recovery row of the qualification matrix for the providers whose
/// bounds stop at `DurableJournal`. The remote providers assert the same
/// contract through `provider_scenarios`, which additionally reaches their
/// version tables.
pub(crate) fn exercise_durable_recovery_replays_unapplied_records<S>(store: &S, table_name: &str)
where
    S: TenantPointRead + DurableJournal,
{
    let table = TableName::new(table_name).expect("table name should be valid");
    let table_id = TableId::new();
    let document = super::sample_document(table_name, "Recovered");
    let record = parity_record(
        SequenceNumber(1),
        Timestamp(2_000),
        &table,
        &table_id,
        WriteOpType::Insert,
        &document.id,
        None,
        Some(document.clone()),
    );

    store
        .append_durable_records_batch(std::slice::from_ref(&record))
        .expect("durable append should succeed");
    let pending = store.journal_progress().expect("progress should load");
    assert_eq!(
        pending,
        JournalProgress {
            durable_head: SequenceNumber(1),
            applied_head: SequenceNumber(0),
        },
        "a durable record must not count as applied before recovery"
    );
    assert!(
        store
            .get(&table, &document.id)
            .expect("read should succeed")
            .is_none(),
        "a durable-but-unapplied record must not be visible to reads"
    );

    store
        .recover_durable_journal()
        .expect("recovery should replay the durable tail");

    let recovered = store
        .get(&table, &document.id)
        .expect("read should succeed")
        .expect("recovery must materialize the durable record");
    assert_eq!(
        recovered.fields.get("title"),
        document.fields.get("title"),
        "recovery must replay the record's content, not just its sequence"
    );
    assert_eq!(
        store.journal_progress().expect("progress should load"),
        JournalProgress {
            durable_head: SequenceNumber(1),
            applied_head: SequenceNumber(1),
        },
        "recovery must advance the applied head to the durable head"
    );

    store
        .recover_durable_journal()
        .expect("a second recovery pass should be a no-op");
    assert_eq!(
        store.journal_progress().expect("progress should load"),
        JournalProgress {
            durable_head: SequenceNumber(1),
            applied_head: SequenceNumber(1),
        },
        "replaying an already-applied tail must not advance either head"
    );
}
