use super::*;
use nimbus_engine::Service;

impl ConvexRegistry {
    pub(crate) async fn apply_schema_to_tenant_async(
        &self,
        service: &Arc<Service>,
        tenant_id: TenantId,
    ) -> Result<(), Error> {
        let Some(schema) = &self.schema else {
            return Ok(());
        };

        let mut tables = schema.tables.values().cloned().collect::<Vec<_>>();
        tables.sort_by(|left, right| left.table.cmp(&right.table));

        for table_schema in tables {
            service
                .set_table_schema_async(tenant_id.clone(), table_schema)
                .await?;
        }

        Ok(())
    }
}
