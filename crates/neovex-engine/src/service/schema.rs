use std::sync::Arc;

use neovex_core::{Error, Result, Schema, TableName, TableSchema, TenantId};
use neovex_storage::TenantWriteStorage;

use super::Service;

impl Service {
    /// Stores a table schema for a tenant.
    pub fn set_table_schema(&self, tenant_id: &TenantId, table_schema: TableSchema) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let table = table_schema.table.clone();
        table_schema.validate_indexes()?;
        table_schema.validate_access_policy()?;
        let previous_policy_revision = runtime
            .schema()
            .get_table(&table)
            .map(TableSchema::access_policy_revision)
            .transpose()?;
        let next_policy_revision = table_schema.access_policy_revision()?;

        runtime.store.replace_table_schema(&table_schema)?;
        let mut schema = runtime
            .schema
            .write()
            .expect("schema lock should not be poisoned");
        schema.tables.insert(table.clone(), table_schema);
        drop(schema);

        if previous_policy_revision.as_deref() != Some(next_policy_revision.as_str()) {
            runtime.clear_document_cache();
            runtime.subscriptions.terminate_policy_revision_mismatches(
                &table,
                &next_policy_revision,
                "authorization policy changed; resubscribe",
            );
        }
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
        table_schema.validate_indexes()?;
        table_schema.validate_access_policy()?;
        let previous_policy_revision = runtime
            .schema()
            .get_table(&table)
            .map(TableSchema::access_policy_revision)
            .transpose()?;
        let next_policy_revision = table_schema.access_policy_revision()?;
        let tenant_id_for_task = tenant_id.clone();
        let runtime_for_task = runtime.clone();
        let table_schema_for_task = table_schema.clone();
        runtime
            .read_storage
            .execute_write(move |transaction| {
                let _operation = runtime_for_task.enter_operation(&tenant_id_for_task)?;
                transaction.replace_table_schema(&table_schema_for_task)
            })
            .await?;
        let mut schema = runtime
            .schema
            .write()
            .expect("schema lock should not be poisoned");
        schema.tables.insert(table.clone(), table_schema);
        drop(schema);

        if previous_policy_revision.as_deref() != Some(next_policy_revision.as_str()) {
            runtime.clear_document_cache();
            runtime.subscriptions.terminate_policy_revision_mismatches(
                &table,
                &next_policy_revision,
                "authorization policy changed; resubscribe",
            );
        }
        Ok(())
    }

    /// Returns the full tenant schema.
    pub fn get_schema(&self, tenant_id: &TenantId) -> Result<Schema> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.schema())
    }

    /// Returns the full tenant schema asynchronously.
    pub async fn get_schema_async(self: &Arc<Self>, tenant_id: TenantId) -> Result<Schema> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        Ok(runtime.schema())
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
        let _operation = runtime.enter_operation(tenant_id)?;
        let previous_policy_revision = runtime
            .schema()
            .get_table(table)
            .map(TableSchema::access_policy_revision)
            .transpose()?;
        runtime.store.delete_table_schema(table)?;
        runtime
            .schema
            .write()
            .expect("schema lock should not be poisoned")
            .tables
            .remove(table);
        let removed_policy_revision = neovex_core::policy_revision_id(None)?;
        if previous_policy_revision.as_deref() != Some(removed_policy_revision.as_str()) {
            runtime.clear_document_cache();
            runtime.subscriptions.terminate_policy_revision_mismatches(
                table,
                &removed_policy_revision,
                "authorization policy changed; resubscribe",
            );
        }
        Ok(())
    }

    /// Deletes a single table schema for a tenant asynchronously.
    pub async fn delete_table_schema_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let previous_policy_revision = runtime
            .schema()
            .get_table(&table)
            .map(TableSchema::access_policy_revision)
            .transpose()?;
        let tenant_id_for_task = tenant_id.clone();
        let runtime_for_task = runtime.clone();
        let table_for_task = table.clone();
        runtime
            .read_storage
            .execute_write(move |transaction| {
                let _operation = runtime_for_task.enter_operation(&tenant_id_for_task)?;
                transaction.delete_table_schema(&table_for_task)
            })
            .await?;
        runtime
            .schema
            .write()
            .expect("schema lock should not be poisoned")
            .tables
            .remove(&table);
        let removed_policy_revision = neovex_core::policy_revision_id(None)?;
        if previous_policy_revision.as_deref() != Some(removed_policy_revision.as_str()) {
            runtime.clear_document_cache();
            runtime.subscriptions.terminate_policy_revision_mismatches(
                &table,
                &removed_policy_revision,
                "authorization policy changed; resubscribe",
            );
        }
        Ok(())
    }
}
