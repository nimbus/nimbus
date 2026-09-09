use nimbus_core::{Error, Result, Timestamp};

use crate::store::MAX_DURABLE_JOURNAL_STREAM_LIMIT;

// Dialect-independent row serialization and document predicates live once in
// `crate::sql`; the MySQL module re-exports them so existing call sites stay
// unchanged.
pub(super) use crate::sql::predicate::{
    filter_documents_with_predicate, filter_index_documents_with_cancel,
    index_fields_for_table_schema, validate_index_prefix_len, validate_index_range_prefix,
};
pub(super) use crate::sql::row::{
    deserialize_json, row_to_document, serialize_document_fields, serialize_document_typed_fields,
    serialize_json,
};

pub(super) fn validate_durable_journal_stream_limit(limit: usize) -> Result<()> {
    if limit == 0 {
        return Err(Error::InvalidInput(
            "durable journal stream limit must be greater than zero".to_string(),
        ));
    }
    if limit > MAX_DURABLE_JOURNAL_STREAM_LIMIT {
        return Err(Error::InvalidInput(format!(
            "durable journal stream limit {limit} exceeds maximum {MAX_DURABLE_JOURNAL_STREAM_LIMIT}"
        )));
    }
    Ok(())
}

pub(super) fn claim_due_jobs_upper_bound(timestamp: Timestamp) -> u64 {
    timestamp.0
}
