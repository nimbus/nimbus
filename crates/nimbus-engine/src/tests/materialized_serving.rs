use super::*;

fn published_sequence(
    engine: &Engine,
    tenant_id: &TenantId,
    table: &TableName,
    context: &str,
) -> SequenceNumber {
    engine
        .materialized_table_publication_stats_for_testing(tenant_id, table)
        .expect("materialized publication stats should load")
        .unwrap_or_else(|| panic!("{context}"))
        .covered_sequence
}

mod concurrency;
mod eviction;
mod retention;
mod reuse;
