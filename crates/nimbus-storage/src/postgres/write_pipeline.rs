use super::backend::{
    apply_durable_record_in_session, encode_u64, i64_from_sequence, map_postgres_error,
};
use super::config::qualified_table;
use super::*;
use crate::sql::schema_events::durable_record_changes_schema_cache;

impl PostgresWriteTransaction {
    pub fn append_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()> {
        self.check_cancel()?;
        if records.is_empty() {
            return Ok(());
        }

        let prepared = crate::sql::write_pipeline::PreparedJournalBatch::prepare(
            self.latest_sequence()?,
            records,
        )?;
        let query = format!(
            "INSERT INTO {} (sequence, record_blob) \
             SELECT sequence, record_blob \
             FROM UNNEST($1::BIGINT[], $2::BYTEA[]) AS batch(sequence, record_blob)",
            qualified_table(&self.schema_name, "commit_log")
        );
        let sequences = prepared
            .sequences()
            .iter()
            .copied()
            .map(|sequence| i64_from_sequence(SequenceNumber(sequence)))
            .collect::<Result<Vec<_>>>()?;
        let payloads = prepared.payloads().to_vec();
        self.pipeline_metrics.record_batch_attempt(prepared.len());
        let metrics = self.pipeline_metrics.as_ref();
        let check_cancel = self.check_cancel.as_ref();
        let client = self.session()?;
        self.block_on(crate::sql::write_pipeline::run_ordered_bounded(
            metrics,
            1,
            check_cancel,
            [Box::pin(async move {
                metrics.record_journal_statement();
                client
                    .execute(query.as_str(), &[&sequences, &payloads])
                    .await
                    .map_err(map_postgres_error)?;
                Ok(())
            })
                as crate::sql::write_pipeline::OrderedSqlFuture<'_>],
        ))?;
        self.provider.fault_injector.check_for_tenant(
            FaultPoint::JournalAppendBeforeDurableFlush,
            &self.tenant_id,
            records,
        )?;
        self.provider.fault_injector.check_for_tenant(
            FaultPoint::JournalFlushBeforeVisibility,
            &self.tenant_id,
            records,
        )?;
        self.notification.journal_changed = true;
        Ok(())
    }

    /// Provider pipeline used after the lease CAS and applied-prefix check have
    /// both succeeded. The journal insert is polled first; `tokio-postgres`
    /// therefore sends it before the apply stream while allowing both requests
    /// to occupy the same connection pipeline.
    pub(super) fn append_and_apply_durable_records_batch(
        &mut self,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        self.check_cancel()?;
        let prepared = crate::sql::write_pipeline::PreparedJournalBatch::prepare(
            self.latest_sequence()?,
            records,
        )?;
        let query = format!(
            "INSERT INTO {} (sequence, record_blob) \
             SELECT sequence, record_blob \
             FROM UNNEST($1::BIGINT[], $2::BYTEA[]) AS batch(sequence, record_blob)",
            qualified_table(&self.schema_name, "commit_log")
        );
        let sequences = prepared
            .sequences()
            .iter()
            .copied()
            .map(|sequence| i64_from_sequence(SequenceNumber(sequence)))
            .collect::<Result<Vec<_>>>()?;
        let payloads = prepared.payloads().to_vec();
        let last_sequence = records
            .last()
            .expect("prepared SQL journal batch is non-empty")
            .sequence;
        let schema_name = self.schema_name.clone();
        let changes_schema_cache = records.iter().any(durable_record_changes_schema_cache);
        self.pipeline_metrics.record_batch_attempt(prepared.len());
        let metrics = self.pipeline_metrics.as_ref();
        let check_cancel = self.check_cancel.as_ref();
        let client = self.session()?;
        let append = Box::pin(async move {
            metrics.record_journal_statement();
            client
                .execute(query.as_str(), &[&sequences, &payloads])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        }) as crate::sql::write_pipeline::OrderedSqlFuture<'_>;
        let apply = Box::pin(async move {
            for record in records {
                check_cancel()?;
                apply_durable_record_in_session(client, &schema_name, record).await?;
            }
            check_cancel()?;
            let watermark_query = format!(
                "INSERT INTO {} (key, value_blob) VALUES ($1, $2) \
                 ON CONFLICT(key) DO UPDATE SET value_blob = EXCLUDED.value_blob",
                qualified_table(&schema_name, "metadata")
            );
            let key = APPLIED_SEQUENCE_KEY.to_string();
            let value = encode_u64(last_sequence.0).to_vec();
            client
                .execute(watermark_query.as_str(), &[&key, &value])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        }) as crate::sql::write_pipeline::OrderedSqlFuture<'_>;
        self.block_on(crate::sql::write_pipeline::run_ordered_bounded(
            metrics,
            crate::sql::write_pipeline::POSTGRES_MAX_IN_FLIGHT_OPERATIONS,
            check_cancel,
            [append, apply],
        ))?;
        self.provider.fault_injector.check_for_tenant(
            FaultPoint::JournalAppendBeforeDurableFlush,
            &self.tenant_id,
            records,
        )?;
        self.provider.fault_injector.check_for_tenant(
            FaultPoint::JournalFlushBeforeVisibility,
            &self.tenant_id,
            records,
        )?;
        self.notification.journal_changed = true;
        self.record_durable_schema_change_effects(changes_schema_cache);
        Ok(())
    }
}
