use neovex_core::{Error, Result, Schema, TableName, TableSchema, TenantId};

use super::Service;

impl Service {
    /// Stores a table schema for a tenant.
    pub fn set_table_schema(&self, tenant_id: &TenantId, table_schema: TableSchema) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        table_schema.validate_indexes()?;

        runtime.store.replace_table_schema(&table_schema)?;
        let mut schema = runtime
            .schema
            .write()
            .expect("schema lock should not be poisoned");
        schema
            .tables
            .insert(table_schema.table.clone(), table_schema);
        Ok(())
    }

    /// Returns the full tenant schema.
    pub fn get_schema(&self, tenant_id: &TenantId) -> Result<Schema> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
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

    /// Deletes a single table schema for a tenant.
    pub fn delete_table_schema(&self, tenant_id: &TenantId, table: &TableName) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime.store.delete_table_schema(table)?;
        runtime
            .schema
            .write()
            .expect("schema lock should not be poisoned")
            .tables
            .remove(table);
        Ok(())
    }
}
