use std::sync::Arc;

use nimbus_core::{
    Document, Error, HistoricalReadShape, Result, TableName, TenantEventRecord, TenantId,
};
use nimbus_storage::DurableJournalBootstrap;

use crate::PinnedServingReadSnapshot;
use crate::engine::Engine;

impl Engine {
    pub fn pin_serving_read_shape(
        &self,
        tenant_id: &TenantId,
        read_shape: HistoricalReadShape,
    ) -> Result<PinnedServingReadSnapshot> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let required_sequence = read_shape.read_snapshot().sequence().sequence();
        let table = read_shape.table().clone();
        let _operation = runtime.enter_operation(tenant_id)?;
        let snapshot = if let Some(snapshot) =
            runtime.materialized_serving_snapshot_for_table(&table, required_sequence)
        {
            snapshot
        } else {
            runtime.load_materialized_serving_snapshot_cancellable(
                runtime.store(),
                &table,
                required_sequence,
                &mut || Ok(()),
            )?
        };
        snapshot.pin_read_shape(read_shape)
    }

    pub(super) async fn read_durable_journal_suffix_to_sequence_async(
        self: &Arc<Self>,
        tenant_id: &TenantId,
        bootstrap: &DurableJournalBootstrap,
    ) -> Result<Vec<TenantEventRecord>> {
        let mut after = bootstrap.resume_after;
        let mut tail = Vec::new();
        while after.0 < bootstrap.bootstrap_cut.0 {
            let page = self
                .stream_durable_journal_async(tenant_id.clone(), after, 256)
                .await?;
            let page_records = page
                .records
                .into_iter()
                .take_while(|record| record.sequence.0 <= bootstrap.bootstrap_cut.0)
                .collect::<Vec<_>>();
            let Some(last_record) = page_records.last() else {
                return Err(Error::Internal(format!(
                    "journal stream made no progress while verifying consistency for tenant {} up to sequence {} from {}",
                    tenant_id, bootstrap.bootstrap_cut.0, after.0
                )));
            };
            after = last_record.sequence;
            tail.extend(page_records);
        }
        Ok(tail)
    }
}

pub(crate) fn snapshot_table_documents(
    snapshot: &crate::tenant::ServingSnapshot,
    table: &TableName,
    context: &str,
) -> Result<Vec<Document>> {
    snapshot.table_documents(table).ok_or_else(|| {
        Error::Internal(format!(
            "materialized serving snapshot missing table {table} during {context}"
        ))
    })
}
