use std::collections::BTreeSet;
use std::sync::Arc;

use nimbus_core::{Error, Result, Schema, TableName, TableSchema, TenantId, policy_revision_id};

use crate::tenant::TenantRuntime;

use super::Engine;

impl Engine {
    /// Stores a table schema for a tenant.
    pub fn set_table_schema(&self, tenant_id: &TenantId, table_schema: TableSchema) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let table = table_schema.table.clone();
        apply_set_table_schema(&runtime, tenant_id, table_schema)?;
        self.notify_table_schema_change_observers(tenant_id, &table);
        Ok(())
    }

    /// Stores a table schema for a tenant asynchronously.
    pub async fn set_table_schema_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table_schema: TableSchema,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let table = table_schema.table.clone();
        let tenant_id_for_task = tenant_id.clone();
        let runtime_for_task = runtime.clone();
        tokio::task::spawn_blocking(move || {
            apply_set_table_schema(&runtime_for_task, &tenant_id_for_task, table_schema)
        })
        .await
        .map_err(map_schema_task_join_error)??;
        self.notify_table_schema_change_observers(&tenant_id, &table);
        Ok(())
    }

    /// Returns the full tenant schema.
    pub fn get_schema(&self, tenant_id: &TenantId) -> Result<Schema> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.schema().as_ref().clone())
    }

    /// Returns the full tenant schema asynchronously.
    pub async fn get_schema_async(self: &Arc<Self>, tenant_id: TenantId) -> Result<Schema> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        Ok(runtime.schema().as_ref().clone())
    }

    /// Returns a single table schema for a tenant.
    pub fn get_table_schema(&self, tenant_id: &TenantId, table: &TableName) -> Result<TableSchema> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime
            .schema()
            .get_table(table)
            .cloned()
            .ok_or(Error::SchemaNotFound(table.clone()))
    }

    /// Returns a single table schema for a tenant asynchronously.
    pub async fn get_table_schema_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
    ) -> Result<TableSchema> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .schema()
            .get_table(&table)
            .cloned()
            .ok_or(Error::SchemaNotFound(table))
    }

    /// Deletes a single table schema for a tenant.
    pub fn delete_table_schema(&self, tenant_id: &TenantId, table: &TableName) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        apply_delete_table_schema(&runtime, tenant_id, table)?;
        self.notify_table_schema_change_observers(tenant_id, table);
        Ok(())
    }

    /// Deletes a single table schema for a tenant asynchronously.
    pub async fn delete_table_schema_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let tenant_id_for_task = tenant_id.clone();
        let runtime_for_task = runtime.clone();
        let table_for_task = table.clone();
        tokio::task::spawn_blocking(move || {
            apply_delete_table_schema(&runtime_for_task, &tenant_id_for_task, &table_for_task)
        })
        .await
        .map_err(map_schema_task_join_error)??;
        self.notify_table_schema_change_observers(&tenant_id, &table);
        Ok(())
    }
    pub(super) async fn refresh_loaded_schema_from_store_async(
        &self,
        runtime: &Arc<TenantRuntime>,
    ) -> Result<()> {
        let next_schema = runtime
            .store()
            .load_schema_async(runtime.read_storage())
            .await?;
        for table in apply_loaded_schema_snapshot(runtime, next_schema)? {
            self.notify_table_schema_change_observers(runtime.tenant_id(), &table);
        }
        Ok(())
    }
}

fn apply_loaded_schema_snapshot(
    runtime: &Arc<TenantRuntime>,
    next_schema: Schema,
) -> Result<Vec<TableName>> {
    let previous_schema = runtime.schema();
    if previous_schema.as_ref() == &next_schema {
        return Ok(Vec::new());
    }

    let mut changed_policy_tables = Vec::new();
    let mut changed_schema_tables = Vec::new();
    let table_names = previous_schema
        .tables
        .keys()
        .chain(next_schema.tables.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for table in table_names {
        let previous_revision = effective_policy_revision(previous_schema.get_table(&table))?;
        let next_revision = effective_policy_revision(next_schema.get_table(&table))?;
        if previous_schema.get_table(&table) != next_schema.get_table(&table) {
            changed_schema_tables.push(table.clone());
        }
        if previous_revision != next_revision {
            changed_policy_tables.push((table, next_revision));
        }
    }

    runtime.replace_schema_snapshot(Arc::new(next_schema));

    if !changed_policy_tables.is_empty() {
        runtime.clear_document_cache();
        for (table, revision) in changed_policy_tables {
            runtime
                .subscription_registry()
                .terminate_policy_revision_mismatches(
                    &table,
                    &revision,
                    "authorization policy changed; resubscribe",
                );
        }
    }

    Ok(changed_schema_tables)
}

fn apply_set_table_schema(
    runtime: &Arc<TenantRuntime>,
    tenant_id: &TenantId,
    table_schema: TableSchema,
) -> Result<()> {
    let _sequence_guard = runtime.lock_mutation_sequence();
    let _operation = runtime.enter_operation(tenant_id)?;
    let table = table_schema.table.clone();
    let mut table_schema = table_schema;
    let previous_schema = runtime.schema();
    table_schema.reconcile_index_metadata(previous_schema.get_table(&table));
    table_schema.validate_indexes()?;
    table_schema.validate_access_policy()?;
    let previous_policy_revision = previous_schema
        .get_table(&table)
        .map(TableSchema::access_policy_revision)
        .transpose()?;
    let next_policy_revision = table_schema.access_policy_revision()?;

    runtime.store().replace_table_schema(&table_schema)?;
    runtime.sync_mutation_journal_progress(runtime.store().journal_progress()?);

    let mut schema = previous_schema;
    Arc::make_mut(&mut schema)
        .tables
        .insert(table.clone(), table_schema);
    runtime.replace_schema_snapshot(schema);

    if previous_policy_revision.as_deref() != Some(next_policy_revision.as_str()) {
        runtime.clear_document_cache();
        runtime
            .subscription_registry()
            .terminate_policy_revision_mismatches(
                &table,
                &next_policy_revision,
                "authorization policy changed; resubscribe",
            );
    }
    Ok(())
}

fn apply_delete_table_schema(
    runtime: &Arc<TenantRuntime>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Result<()> {
    let _sequence_guard = runtime.lock_mutation_sequence();
    let _operation = runtime.enter_operation(tenant_id)?;
    let previous_schema = runtime.schema();
    let previous_policy_revision = previous_schema
        .get_table(table)
        .map(TableSchema::access_policy_revision)
        .transpose()?;

    runtime.store().delete_table_schema(table)?;
    runtime.sync_mutation_journal_progress(runtime.store().journal_progress()?);

    let mut schema = previous_schema;
    Arc::make_mut(&mut schema).tables.remove(table);
    runtime.replace_schema_snapshot(schema);

    let removed_policy_revision = policy_revision_id(None)?;
    if previous_policy_revision.as_deref() != Some(removed_policy_revision.as_str()) {
        runtime.clear_document_cache();
        runtime
            .subscription_registry()
            .terminate_policy_revision_mismatches(
                table,
                &removed_policy_revision,
                "authorization policy changed; resubscribe",
            );
    }
    Ok(())
}

fn map_schema_task_join_error(error: tokio::task::JoinError) -> Error {
    Error::Internal(format!("schema storage task failed: {error}"))
}

fn effective_policy_revision(table_schema: Option<&TableSchema>) -> Result<String> {
    match table_schema {
        Some(table_schema) => table_schema.access_policy_revision(),
        None => policy_revision_id(None),
    }
}
