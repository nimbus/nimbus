use std::sync::Arc;

use nimbus_core::{Document, DurableMutationRecord, Error, Result, TableName, TenantId};
use nimbus_storage::{DurableJournalBootstrap, TenantStore};

use crate::service::Service;

impl Service {
    pub(super) async fn read_durable_journal_suffix_to_sequence_async(
        self: &Arc<Self>,
        tenant_id: &TenantId,
        bootstrap: &DurableJournalBootstrap,
    ) -> Result<Vec<DurableMutationRecord>> {
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

pub(super) fn rebuild_authoritative_snapshot(
    bootstrap: &DurableJournalBootstrap,
    journal_tail: &[DurableMutationRecord],
) -> Result<crate::MaterializedJournalSnapshot> {
    let store = TenantStore::create_in_memory()?;
    store.rebuild_materialized_journal_from_snapshot(
        &bootstrap.snapshot,
        journal_tail,
        Some(bootstrap.bootstrap_cut),
    )?;
    store.export_materialized_journal_snapshot()
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
