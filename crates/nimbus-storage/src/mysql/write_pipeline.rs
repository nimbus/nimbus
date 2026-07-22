use super::backend::{map_mysql_error, qualified_table};
use super::*;

// COM_STMT_EXECUTE carries command (1), statement id (4), flags (1), iteration
// count (4), and the new-parameter-bound flag (1) before its parameter data.
const MYSQL_EXECUTE_PACKET_FIXED_BYTES: u64 = 11;
// Each record contributes two parameter types (4 bytes total) and one u64
// sequence value (8 bytes). The length-encoded blob is counted separately.
const MYSQL_EXECUTE_PACKET_BYTES_PER_RECORD: u64 = 12;
const MYSQL_QUERY_PACKET_COMMAND_BYTES: u64 = 1;
const MYSQL_QUERY_VALUE_GROUP_BYTES: u64 = 6;
const MYSQL_QUERY_VALUE_SEPARATOR_BYTES: u64 = 2;

fn mysql_length_encoded_integer_bytes(value: usize) -> u64 {
    match value {
        0..=250 => 1,
        251..=65_535 => 3,
        65_536..=16_777_215 => 4,
        _ => 9,
    }
}

fn mysql_journal_payload_prefix_bytes(
    prepared: &crate::sql::write_pipeline::PreparedJournalBatch,
) -> Result<Vec<u64>> {
    let mut prefix_bytes = Vec::with_capacity(prepared.len().saturating_add(1));
    prefix_bytes.push(0_u64);
    for payload in prepared.payloads() {
        let encoded = mysql_length_encoded_integer_bytes(payload.len())
            .checked_add(u64::try_from(payload.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| Error::Internal("MySQL journal payload size overflowed".to_string()))?;
        let cumulative = prefix_bytes
            .last()
            .copied()
            .expect("payload prefix always starts at zero")
            .checked_add(encoded)
            .ok_or_else(|| Error::Internal("MySQL journal payload size overflowed".to_string()))?;
        prefix_bytes.push(cumulative);
    }
    Ok(prefix_bytes)
}

fn mysql_journal_packet_bytes(
    payload_prefix_bytes: &[u64],
    range: std::ops::Range<usize>,
    query_prefix_bytes: usize,
) -> Result<(u64, u64)> {
    let records = u64::try_from(range.len())
        .map_err(|_| Error::Internal("MySQL journal chunk size exceeds u64".to_string()))?;
    let separators = records.saturating_sub(1);
    let query_bytes = MYSQL_QUERY_PACKET_COMMAND_BYTES
        .checked_add(u64::try_from(query_prefix_bytes).unwrap_or(u64::MAX))
        .and_then(|value| value.checked_add(records.checked_mul(MYSQL_QUERY_VALUE_GROUP_BYTES)?))
        .and_then(|value| {
            value.checked_add(separators.checked_mul(MYSQL_QUERY_VALUE_SEPARATOR_BYTES)?)
        })
        .ok_or_else(|| Error::Internal("MySQL journal query size overflowed".to_string()))?;

    let parameter_count = records
        .checked_mul(2)
        .ok_or_else(|| Error::Internal("MySQL journal parameter count overflowed".to_string()))?;
    let null_bitmap_bytes = parameter_count.div_ceil(8);
    let mut execute_bytes = MYSQL_EXECUTE_PACKET_FIXED_BYTES
        .checked_add(null_bitmap_bytes)
        .and_then(|value| {
            value.checked_add(records.checked_mul(MYSQL_EXECUTE_PACKET_BYTES_PER_RECORD)?)
        })
        .ok_or_else(|| Error::Internal("MySQL journal execute size overflowed".to_string()))?;
    let payload_bytes = payload_prefix_bytes
        .get(range.end)
        .zip(payload_prefix_bytes.get(range.start))
        .and_then(|(end, start)| end.checked_sub(*start))
        .ok_or_else(|| Error::Internal("MySQL journal payload range is invalid".to_string()))?;
    execute_bytes = execute_bytes
        .checked_add(payload_bytes)
        .ok_or_else(|| Error::Internal("MySQL journal execute size overflowed".to_string()))?;
    Ok((query_bytes, execute_bytes))
}

fn mysql_journal_statement_ranges(
    prepared: &crate::sql::write_pipeline::PreparedJournalBatch,
    query_prefix_bytes: usize,
    max_allowed_packet: u64,
) -> Result<Vec<std::ops::Range<usize>>> {
    let payload_prefix_bytes = mysql_journal_payload_prefix_bytes(prepared)?;
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < prepared.len() {
        let mut end = start + 1;
        let single_packet_bytes =
            mysql_journal_packet_bytes(&payload_prefix_bytes, start..end, query_prefix_bytes)?;
        if single_packet_bytes.0 > max_allowed_packet || single_packet_bytes.1 > max_allowed_packet
        {
            return Err(Error::InvalidInput(format!(
                "serialized durable journal record at batch index {start} requires MySQL packets of {} and {} bytes, exceeding max_allowed_packet {max_allowed_packet}",
                single_packet_bytes.0, single_packet_bytes.1
            )));
        }
        while end < prepared.len() {
            let candidate = mysql_journal_packet_bytes(
                &payload_prefix_bytes,
                start..end + 1,
                query_prefix_bytes,
            )?;
            if candidate.0 > max_allowed_packet || candidate.1 > max_allowed_packet {
                break;
            }
            end += 1;
        }
        ranges.push(start..end);
        start = end;
    }
    Ok(ranges)
}

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
        let query_prefix = format!(
            "INSERT INTO {} (sequence, record_blob) VALUES ",
            qualified_table(&self.database_name, "commit_log"),
        );
        let statement_ranges = mysql_journal_statement_ranges(
            &prepared,
            query_prefix.len(),
            self.provider.max_allowed_packet,
        )?;
        self.pipeline_metrics.record_batch_attempt(prepared.len());
        let runtime_handle = self.provider.runtime_handle.clone();
        let metrics = self.pipeline_metrics.clone();
        for range in statement_ranges {
            self.check_cancel()?;
            let value_groups = vec!["(?, ?)"; range.len()].join(", ");
            let query = format!("{query_prefix}{value_groups}");
            let mut params = Vec::with_capacity(range.len().saturating_mul(2));
            for index in range {
                params.push(MySqlValue::UInt(prepared.sequences()[index]));
                params.push(MySqlValue::Bytes(prepared.payloads()[index].clone()));
            }
            let conn = self.session()?;
            metrics.record_journal_statement();
            let started = std::time::Instant::now();
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
        }
        self.provider
            .fault_injector
            .check(FaultPoint::JournalAppendBeforeDurableFlush)?;
        self.provider
            .fault_injector
            .check(FaultPoint::JournalFlushBeforeVisibility)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nimbus_core::{SequenceNumber, TenantEventRecord, Timestamp};

    use super::*;
    use crate::sql::write_pipeline::PreparedJournalBatch;

    fn prepared_batch(records: usize, reason_bytes: usize) -> PreparedJournalBatch {
        let records = (1..=records)
            .map(|sequence| {
                let sequence = u64::try_from(sequence).expect("test sequence fits u64");
                TenantEventRecord::barrier(
                    SequenceNumber(sequence),
                    Timestamp(sequence),
                    "x".repeat(reason_bytes),
                )
                .expect("barrier record should build")
            })
            .collect::<Vec<_>>();
        PreparedJournalBatch::prepare(SequenceNumber(0), &records)
            .expect("test journal batch should prepare")
    }

    #[test]
    fn mysql_journal_packet_plan_preserves_individually_valid_records() {
        let prepared = prepared_batch(4, 400);
        let prefix_bytes = 96;
        let payload_prefix_bytes = mysql_journal_payload_prefix_bytes(&prepared)
            .expect("payload prefix sizes should compute");
        let max_single_packet = (0..prepared.len())
            .map(|index| {
                let (query, execute) = mysql_journal_packet_bytes(
                    &payload_prefix_bytes,
                    index..index + 1,
                    prefix_bytes,
                )
                .expect("single-record packet size should compute");
                query.max(execute)
            })
            .max()
            .expect("test batch is non-empty");

        let ranges = mysql_journal_statement_ranges(&prepared, prefix_bytes, max_single_packet)
            .expect("individually valid records should be chunked");
        assert_eq!(ranges, [0..1, 1..2, 2..3, 3..4]);
    }

    #[test]
    fn mysql_journal_packet_plan_keeps_an_ordinary_batch_in_one_statement() {
        let prepared = prepared_batch(8, 32);
        let ranges = mysql_journal_statement_ranges(&prepared, 96, 64 * 1024 * 1024)
            .expect("ordinary batch should fit the fixture packet limit");
        assert_eq!(ranges, [0..8]);
    }

    #[test]
    fn mysql_journal_packet_plan_rejects_one_oversize_record() {
        let prepared = prepared_batch(1, 400);
        let payload_prefix_bytes = mysql_journal_payload_prefix_bytes(&prepared)
            .expect("payload prefix sizes should compute");
        let (_, execute_bytes) = mysql_journal_packet_bytes(&payload_prefix_bytes, 0..1, 96)
            .expect("single-record packet size should compute");
        let error = mysql_journal_statement_ranges(&prepared, 96, execute_bytes - 1)
            .expect_err("one record larger than the packet ceiling must fail clearly");
        assert!(error.to_string().contains("max_allowed_packet"));
    }
}
