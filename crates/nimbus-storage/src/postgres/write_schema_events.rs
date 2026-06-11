use super::*;

pub(super) fn record_postgres_schema_set_events(
    transaction: &mut PostgresWriteTransaction,
    table_id: TableId,
    previous: Option<TableSchema>,
    table_schema: &TableSchema,
) {
    transaction.record_tenant_event(TenantEventKind::SchemaChange {
        change: Box::new(SchemaChangeEvent::SetTable {
            table: table_schema.table.clone(),
            table_id: table_id.clone(),
            previous,
            current: table_schema.clone(),
        }),
    });
    for index in &table_schema.indexes {
        transaction.record_tenant_event(TenantEventKind::IndexLifecycle {
            index: IndexLifecycleEvent {
                table: table_schema.table.clone(),
                table_id: table_id.clone(),
                index_id: index.id.clone(),
                state: index.state,
                definition: index.clone(),
            },
        });
    }
}

pub(super) fn durable_record_changes_schema_cache(record: &DurableMutationRecord) -> bool {
    record.events.iter().any(|event| {
        matches!(
            event,
            TenantEventKind::SchemaChange { .. }
                | TenantEventKind::TableLifecycle {
                    lifecycle: TableLifecycleEvent::HardDelete { .. },
                }
        )
    })
}
