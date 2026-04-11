use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use neovex_core::{Filter, FilterOp, OrderBy, OrderDirection, Query, Result, TableName, TenantId};
use neovex_engine::Service;
use serde_json::json;

const TABLE_COUNT: usize = 3;
const ROW_COUNT: usize = 2_000;
const PAYLOAD_BYTES: usize = 1_024;
const KEEP_EVERY: usize = 97;

fn main() -> Result<()> {
    let data_dir = unique_example_dir();
    fs::create_dir_all(&data_dir).map_err(|error| {
        neovex_core::Error::Internal(format!(
            "failed to create example directory {}: {error}",
            data_dir.display()
        ))
    })?;

    let service = Service::new(&data_dir)?;
    let tenant_id = TenantId::new("demo".to_string())?;
    service.create_tenant(tenant_id.clone())?;
    let payload = "x".repeat(PAYLOAD_BYTES);
    let tables = (0..TABLE_COUNT)
        .map(|index| TableName::new(format!("tasks_{index}")))
        .collect::<Result<Vec<_>>>()?;
    for table in &tables {
        for rank in 0..ROW_COUNT {
            let status = if rank % KEEP_EVERY == 0 {
                "keep"
            } else {
                "skip"
            };
            service.insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("title".to_string(), json!(format!("task-{rank:05}"))),
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                    ("payload".to_string(), json!(payload)),
                ]),
            )?;
        }
    }

    let expected_count = (0..ROW_COUNT).filter(|rank| rank % KEEP_EVERY == 0).count();
    let cold_started_at = Instant::now();
    for table in &tables {
        let result = service.query_documents(&tenant_id, &full_scan_query(table.clone()))?;
        if result.len() != expected_count {
            return Err(neovex_core::Error::Internal(format!(
                "expected {expected_count} selective rows for table {table}, got {}",
                result.len()
            )));
        }
        black_box(result);
    }
    let cold_duration = cold_started_at.elapsed();

    let warm_started_at = Instant::now();
    for table in &tables {
        let result = service.query_documents(&tenant_id, &full_scan_query(table.clone()))?;
        if result.len() != expected_count {
            return Err(neovex_core::Error::Internal(format!(
                "expected {expected_count} selective rows for warm table {table}, got {}",
                result.len()
            )));
        }
        black_box(result);
    }
    let warm_duration = warm_started_at.elapsed();

    eprintln!("service_full_scan_cold total={cold_duration:?}");
    eprintln!("service_full_scan_materialized_surface total={warm_duration:?}");
    println!(
        "dataset tables={TABLE_COUNT} rows_per_table={ROW_COUNT} payload_bytes={PAYLOAD_BYTES} keep_every={KEEP_EVERY}"
    );
    println!(
        "cold full-scan service query:   {:?} avg (baseline)",
        average_duration(cold_duration)
    );
    println!(
        "warm materialized-surface query:{:?} avg ({:.2}x faster, {:+.2}% change)",
        average_duration(warm_duration),
        duration_ratio(cold_duration, warm_duration),
        percentage_change(cold_duration, warm_duration)
    );

    let _ = fs::remove_dir_all(&data_dir);
    Ok(())
}

fn unique_example_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "neovex-materialized-surface-eval-{}-{nanos}",
        std::process::id()
    ))
}

fn filter(field: &str, op: FilterOp, value: serde_json::Value) -> Filter {
    Filter {
        field: field.to_string(),
        op,
        value,
    }
}

fn full_scan_query(table: TableName) -> Query {
    Query {
        table,
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "title".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    }
}

fn average_duration(total: Duration) -> Duration {
    total.div_f64(TABLE_COUNT as f64)
}

fn duration_ratio(baseline: Duration, candidate: Duration) -> f64 {
    baseline.as_secs_f64() / candidate.as_secs_f64()
}

fn percentage_change(baseline: Duration, candidate: Duration) -> f64 {
    ((candidate.as_secs_f64() - baseline.as_secs_f64()) / baseline.as_secs_f64()) * 100.0
}
