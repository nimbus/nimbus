use std::hint::black_box;
use std::time::{Duration, Instant};

use nimbus_core::{Document, Error, Filter, FilterOp, Result, TableName};
use nimbus_storage::{ShadowMaterializer, ShadowMaterializerConfig, TenantStore};
use serde_json::json;

const ROW_COUNT: usize = 20_000;
const PAYLOAD_BYTES: usize = 1_024;
const KEEP_EVERY: usize = 97;
const WARMUP_ITERATIONS: usize = 3;
const MEASURE_ITERATIONS: usize = 20;

fn main() -> Result<()> {
    let table = TableName::new("tasks")?;
    let selective_filter = filter("status", FilterOp::Eq, json!("keep"));
    let store = TenantStore::create_in_memory()?;
    populate_store(&store, &table)?;

    let expected_selective = (0..ROW_COUNT).filter(|rank| rank % KEEP_EVERY == 0).count();
    let expected_broad_rank_sum = (0..ROW_COUNT)
        .map(|rank| i64::try_from(rank).expect("rank should fit in i64"))
        .sum::<i64>();

    let selective_full_result = count_status_full_deserialize(&store, &table, "keep")?;
    let selective_pushdown_result =
        count_status_with_pushdown(&store, &table, std::slice::from_ref(&selective_filter))?;

    let shadow = ShadowMaterializer::from_checkpoint_and_journal(
        store.export_materialized_journal_snapshot()?,
        Vec::new(),
        ShadowMaterializerConfig::default(),
    )?;
    let materialized_documents = shadow.current_documents();
    let selective_materialized_result =
        count_status_in_materialized_docs(&materialized_documents, "keep");

    if selective_full_result != expected_selective
        || selective_pushdown_result != expected_selective
        || selective_materialized_result != expected_selective
    {
        return Err(Error::Internal(
            "selective benchmark variants returned mismatched result counts".to_string(),
        ));
    }

    let broad_full_result = sum_rank_full_deserialize(&store, &table)?;
    let broad_materialized_result = sum_rank_in_materialized_docs(&materialized_documents);
    if broad_full_result != expected_broad_rank_sum
        || broad_materialized_result != expected_broad_rank_sum
    {
        return Err(Error::Internal(
            "broad benchmark variants returned mismatched rank sums".to_string(),
        ));
    }

    let selective_full_duration = measure("selective_full_deserialize", || {
        let result =
            count_status_full_deserialize(&store, &table, "keep").expect("scan should work");
        black_box(result);
    });
    let selective_pushdown_duration = measure("selective_pushdown", || {
        let result =
            count_status_with_pushdown(&store, &table, std::slice::from_ref(&selective_filter))
                .expect("pushdown scan should work");
        black_box(result);
    });
    let selective_materialized_duration = measure("selective_materialized_docs", || {
        let result = count_status_in_materialized_docs(&materialized_documents, "keep");
        black_box(result);
    });

    let broad_full_duration = measure("broad_full_deserialize", || {
        let result = sum_rank_full_deserialize(&store, &table).expect("scan should work");
        black_box(result);
    });
    let broad_materialized_duration = measure("broad_materialized_docs", || {
        let result = sum_rank_in_materialized_docs(&materialized_documents);
        black_box(result);
    });

    println!("dataset rows={ROW_COUNT} payload_bytes={PAYLOAD_BYTES} keep_every={KEEP_EVERY}");
    println!(
        "selective full_deserialize:   {:?} avg (baseline)",
        average_duration(selective_full_duration)
    );
    println!(
        "selective pushdown:           {:?} avg ({:.2}x faster than baseline)",
        average_duration(selective_pushdown_duration),
        duration_ratio(selective_full_duration, selective_pushdown_duration)
    );
    println!(
        "selective materialized_docs:  {:?} avg ({:.2}x faster than baseline)",
        average_duration(selective_materialized_duration),
        duration_ratio(selective_full_duration, selective_materialized_duration)
    );
    println!(
        "broad full_deserialize:       {:?} avg (baseline)",
        average_duration(broad_full_duration)
    );
    println!(
        "broad materialized_docs:      {:?} avg ({:.2}x faster than baseline)",
        average_duration(broad_materialized_duration),
        duration_ratio(broad_full_duration, broad_materialized_duration)
    );

    Ok(())
}

fn populate_store(store: &TenantStore, table: &TableName) -> Result<()> {
    let payload = "x".repeat(PAYLOAD_BYTES);
    for rank in 0..ROW_COUNT {
        let status = if rank % KEEP_EVERY == 0 {
            "keep"
        } else {
            "skip"
        };
        let document = Document::new(
            table.clone(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!(format!("task-{rank:05}"))),
                ("status".to_string(), json!(status)),
                ("rank".to_string(), json!(rank)),
                ("payload".to_string(), json!(payload)),
            ]),
        );
        store.insert(&document)?;
    }
    Ok(())
}

fn filter(field: &str, op: FilterOp, value: serde_json::Value) -> Filter {
    Filter {
        field: field.to_string(),
        op,
        value,
    }
}

fn count_status_full_deserialize(
    store: &TenantStore,
    table: &TableName,
    status: &str,
) -> Result<usize> {
    let status = json!(status);
    Ok(store
        .scan_table_matching_cancellable(table, &mut || Ok(()), |document| {
            Ok(document.get_field("status") == Some(&status))
        })?
        .len())
}

fn count_status_with_pushdown(
    store: &TenantStore,
    table: &TableName,
    filters: &[Filter],
) -> Result<usize> {
    Ok(store
        .scan_table_matching_with_filters_cancellable(
            table,
            filters,
            &mut || Ok(()),
            |_document| Ok(true),
        )?
        .len())
}

fn count_status_in_materialized_docs(documents: &[Document], status: &str) -> usize {
    let status = json!(status);
    documents
        .iter()
        .filter(|document| document.get_field("status") == Some(&status))
        .count()
}

fn sum_rank_full_deserialize(store: &TenantStore, table: &TableName) -> Result<i64> {
    Ok(store
        .scan_table_matching_cancellable(table, &mut || Ok(()), |_document| Ok(true))?
        .into_iter()
        .map(|document| document_rank(&document))
        .sum())
}

fn sum_rank_in_materialized_docs(documents: &[Document]) -> i64 {
    documents.iter().map(document_rank).sum()
}

fn document_rank(document: &Document) -> i64 {
    document
        .get_field("rank")
        .and_then(serde_json::Value::as_i64)
        .expect("rank field should be present")
}

fn measure<F>(label: &str, mut run: F) -> Duration
where
    F: FnMut(),
{
    for _ in 0..WARMUP_ITERATIONS {
        run();
    }

    let start = Instant::now();
    for _ in 0..MEASURE_ITERATIONS {
        run();
    }
    let elapsed = start.elapsed();
    eprintln!("{label} total={elapsed:?}");
    elapsed
}

fn average_duration(total: Duration) -> Duration {
    total.div_f64(MEASURE_ITERATIONS as f64)
}

fn duration_ratio(baseline: Duration, candidate: Duration) -> f64 {
    baseline.as_secs_f64() / candidate.as_secs_f64()
}
