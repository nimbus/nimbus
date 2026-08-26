use nimbus_core::{
    Error, HistoricalReadErrorKind, Result, SequenceNumber, StorageErrorKind, TenantEventRecord,
};
use serde::{Deserialize, Serialize};

/// Published lower bounds for history that readers may still request.
///
/// A floor is the last sequence that physical retention may have removed.
/// Readers therefore require a cursor or snapshot at or above the matching
/// floor. The three values publish in the same transaction as their deletes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionReadFloors {
    pub document_versions: SequenceNumber,
    pub index_versions: SequenceNumber,
    pub journal: SequenceNumber,
}

impl RetentionReadFloors {
    pub const fn new(
        document_versions: SequenceNumber,
        index_versions: SequenceNumber,
        journal: SequenceNumber,
    ) -> Self {
        Self {
            document_versions,
            index_versions,
            journal,
        }
    }

    pub(crate) fn max(self, other: Self) -> Self {
        Self {
            document_versions: self.document_versions.max(other.document_versions),
            index_versions: self.index_versions.max(other.index_versions),
            journal: self.journal.max(other.journal),
        }
    }

    pub(crate) fn historical_index(self) -> SequenceNumber {
        self.document_versions.max(self.index_versions)
    }
}

pub(crate) fn validate_retention_after_page(
    required_sequence: SequenceNumber,
    authoritative_floor: SequenceNumber,
    context: &str,
) -> Result<()> {
    if required_sequence.0 < authoritative_floor.0 {
        return Err(Error::historical_read(
            HistoricalReadErrorKind::RetentionExpired,
            format!(
                "{context} sequence {} is behind the retention floor {}",
                required_sequence.0, authoritative_floor.0
            ),
        ));
    }
    Ok(())
}

pub(crate) fn validate_contiguous_journal_page(
    after: SequenceNumber,
    records: &[TenantEventRecord],
    latest_sequence: SequenceNumber,
    has_more: bool,
) -> Result<()> {
    let mut expected = after.0.saturating_add(1);
    for record in records {
        record.validate_integrity()?;
        if record.sequence.0 != expected {
            return Err(Error::storage(
                StorageErrorKind::Corruption,
                format!(
                    "durable journal page after sequence {} expected sequence {expected}, got {}",
                    after.0, record.sequence.0
                ),
            ));
        }
        expected = expected.saturating_add(1);
    }
    if let Some(record) = records.last()
        && record.sequence.0 > latest_sequence.0
    {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "durable journal page after sequence {} observed sequence {} beyond latest sequence {}",
                after.0, record.sequence.0, latest_sequence.0
            ),
        ));
    }
    if !has_more && expected <= latest_sequence.0 {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "durable journal page after sequence {} ended at {} before latest sequence {}",
                after.0,
                expected.saturating_sub(1),
                latest_sequence.0
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use nimbus_core::Timestamp;

    use super::*;

    #[test]
    fn empty_logical_event_record_is_not_a_missing_sequence() {
        let record = TenantEventRecord::from_events(SequenceNumber(1), Timestamp(1), Vec::new())
            .expect("empty logical event record should build");

        validate_contiguous_journal_page(SequenceNumber(0), &[record], SequenceNumber(1), false)
            .expect("a present empty logical event must satisfy journal contiguity");
    }

    #[test]
    fn absent_record_before_the_durable_head_is_corruption() {
        let error =
            validate_contiguous_journal_page(SequenceNumber(0), &[], SequenceNumber(1), false)
                .expect_err("a missing durable sequence must fail closed");

        assert_eq!(error.storage_kind(), Some(StorageErrorKind::Corruption));
    }

    #[test]
    fn retention_floor_violation_has_the_typed_historical_classification() {
        let error =
            validate_retention_after_page(SequenceNumber(3), SequenceNumber(4), "test page")
                .expect_err("a target below the floor must fail");

        assert_eq!(
            error.historical_read_kind(),
            Some(HistoricalReadErrorKind::RetentionExpired)
        );
    }
}
