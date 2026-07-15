use nimbus_core::{
    IndexLifecycleEvent, Result, Schema, SchemaChangeEvent, TableName, TableSchema, TenantEventKind,
};

use super::MemoryTenantStore;

impl MemoryTenantStore {
    pub fn load_schema(&self) -> Result<Schema> {
        Ok(self.read_state()?.schema.clone())
    }

    pub fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        table_schema.validate_indexes()?;
        table_schema.validate_access_policy()?;
        let timestamp = self.now();
        self.transact(|state| {
            let previous = state.schema.tables.get(&table_schema.table).cloned();
            let mut current = table_schema.clone();
            current.reconcile_index_metadata(previous.as_ref());
            let table_id = state.resolve_or_create_table_id(&current.table)?;
            state
                .schema
                .tables
                .insert(current.table.clone(), current.clone());
            let mut events = vec![TenantEventKind::SchemaChange {
                change: Box::new(SchemaChangeEvent::SetTable {
                    table: current.table.clone(),
                    table_id: table_id.clone(),
                    previous,
                    current: current.clone(),
                }),
            }];
            events.extend(current.indexes.iter().cloned().map(|definition| {
                TenantEventKind::IndexLifecycle {
                    index: IndexLifecycleEvent {
                        table: current.table.clone(),
                        table_id: table_id.clone(),
                        index_id: definition.id.clone(),
                        state: definition.state,
                        definition,
                    },
                }
            }));
            state.append_events(timestamp, Vec::new(), events)?;
            Ok(())
        })
    }

    pub fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        let timestamp = self.now();
        self.transact(|state| {
            let previous = state.schema.tables.remove(table);
            let table_id = state.active_tables.get(table).cloned();
            state.append_events(
                timestamp,
                Vec::new(),
                vec![TenantEventKind::SchemaChange {
                    change: Box::new(SchemaChangeEvent::DeleteTable {
                        table: table.clone(),
                        table_id,
                        previous,
                    }),
                }],
            )?;
            Ok(())
        })
    }
}
