//! Dialect-free schema-event helpers shared by the SQL backends.
//!
//! Neither function touches a session, a statement, or a driver type: one turns
//! a schema replacement into the tenant events it implies, the other classifies
//! a durable record. The PostgreSQL and MySQL copies were byte-identical apart
//! from the function name and the transaction type they took.
//!
//! The sqlite backend keeps its own private replica of
//! [`durable_record_changes_schema_cache`] (`sqlite/journal.rs`). It is
//! semantically identical, but sqlite is out of scope for this unification pass,
//! so it is left in place rather than repointed.

use nimbus_core::{
    IndexLifecycleEvent, SchemaChangeEvent, TableId, TableLifecycleEvent, TableSchema,
    TenantEventKind, TenantEventRecord,
};

use crate::sql::write_core::{SqlWriteBackend, sql_record_tenant_event};

/// Records the tenant events for replacing `table_schema`: one `SetTable`
/// change followed by one `IndexLifecycle` event per index, in definition order.
pub(crate) fn sql_record_schema_set_events<B: SqlWriteBackend>(
    backend: &mut B,
    table_id: TableId,
    previous: Option<TableSchema>,
    table_schema: &TableSchema,
) {
    sql_record_tenant_event(
        backend,
        TenantEventKind::SchemaChange {
            change: Box::new(SchemaChangeEvent::SetTable {
                table: table_schema.table.clone(),
                table_id: table_id.clone(),
                previous,
                current: table_schema.clone(),
            }),
        },
    );
    for index in &table_schema.indexes {
        sql_record_tenant_event(
            backend,
            TenantEventKind::IndexLifecycle {
                index: IndexLifecycleEvent {
                    table: table_schema.table.clone(),
                    table_id: table_id.clone(),
                    index_id: index.id.clone(),
                    state: index.state,
                    definition: index.clone(),
                },
            },
        );
    }
}

/// Whether applying `record` invalidates a process-local schema cache. Only
/// schema changes and hard table deletes do.
pub(crate) fn durable_record_changes_schema_cache(record: &TenantEventRecord) -> bool {
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
