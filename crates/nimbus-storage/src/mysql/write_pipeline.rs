use super::backend::{map_mysql_error, qualified_table};
use super::*;

impl MySqlWriteTransaction {
    pub fn append_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()> {
        self.check_cancel()?;
        if records.is_empty() {
            return Ok(());
        }

        let prepared = crate::sql::write_pipeline::PreparedJournalBatch::prepare(
            self.latest_sequence()?,
            records,
        )?;
        let value_groups = vec!["(?, ?)"; prepared.len()].join(", ");
        let query = format!(
            "INSERT INTO {} (sequence, record_blob) VALUES {value_groups}",
            qualified_table(&self.database_name, "commit_log"),
        );
        let mut params = Vec::with_capacity(prepared.len().saturating_mul(2));
        for (&sequence, payload) in prepared.sequences().iter().zip(prepared.payloads()) {
            params.push(MySqlValue::UInt(sequence));
            params.push(MySqlValue::Bytes(payload.clone()));
        }
        self.pipeline_metrics.record_batch_attempt(prepared.len());
        let started = std::time::Instant::now();
        let runtime_handle = self.provider.runtime_handle.clone();
        let metrics = self.pipeline_metrics.clone();
        self.check_cancel()?;
        let conn = self.session()?;
        metrics.record_journal_statement();
        let in_flight = metrics.operation_started();
        let result = Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, Params::Positional(params))
                .await
                .map_err(map_mysql_error)
        });
        drop(in_flight);
        metrics.record_elapsed(started);
        if let Err(error) = &result {
            metrics.record_error(error);
        }
        result?;
        self.provider
            .fault_injector
            .check(FaultPoint::JournalAppendBeforeDurableFlush)?;
        self.provider
            .fault_injector
            .check(FaultPoint::JournalFlushBeforeVisibility)?;
        Ok(())
    }
}
