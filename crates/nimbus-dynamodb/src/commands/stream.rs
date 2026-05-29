//! DynamoDB Streams (T5): DescribeStream (D5.2), GetShardIterator (D5.3),
//! GetRecords (D5.4), ListStreams (D5.5).
//!
//! Each stream-enabled table exposes a **single** shard (DDB-DIV-006 — real
//! DynamoDB exposes a shard tree, ExtendDB 4 shards). The stream ARN is
//! `<table_arn>/stream/<label>` (label = table id); sequence numbers are
//! zero-padded i64 strings.

use std::sync::Arc;

use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{
    DescribeStreamInput, DescribeStreamOutput, SequenceNumberRange, Shard, StreamDescription,
    StreamStatus, StreamViewType, TableDescription,
};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;

use crate::commands::control_plane;

/// Format an i64 stream sequence number as a stable zero-padded string.
#[must_use]
pub(crate) fn sequence_number(value: i64) -> String {
    format!("{value:027}")
}

/// The single shard's id for a stream (stable per stream/table).
#[must_use]
pub(crate) fn shard_id(table_id: &str) -> String {
    format!("shardId-00000000000000000000-{table_id}")
}

/// Resolve the table referenced by a stream ARN and verify the ARN is the
/// table's current stream. Returns the table description.
///
/// # Errors
/// `ResourceNotFoundException` for a malformed ARN, an unknown table, or an ARN
/// that does not match the table's enabled stream.
pub(crate) fn resolve_stream_table(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    stream_arn: &str,
) -> Result<TableDescription, DynamoDbError> {
    let table_name = table_name_from_stream_arn(stream_arn)?;
    let description = control_plane::load_table_description(service, context, table_name)?;
    if description.latest_stream_arn.as_deref() != Some(stream_arn) {
        return Err(stream_not_found(stream_arn));
    }
    Ok(description)
}

/// Extract the table name from `…:table/<name>/stream/<label>`.
fn table_name_from_stream_arn(arn: &str) -> Result<&str, DynamoDbError> {
    let after_table = arn
        .split("table/")
        .nth(1)
        .ok_or_else(|| stream_not_found(arn))?;
    let name = after_table
        .split("/stream/")
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| stream_not_found(arn))?;
    Ok(name)
}

fn stream_not_found(stream_arn: &str) -> DynamoDbError {
    DynamoDbError::ResourceNotFoundException(format!(
        "Requested resource not found: Stream: {stream_arn} not found"
    ))
}

/// DescribeStream: return a single-shard description for the stream-enabled
/// table the ARN refers to.
///
/// # Errors
/// `ResourceNotFoundException` if the ARN does not match an enabled stream.
pub fn describe_stream(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: DescribeStreamInput,
) -> Result<DescribeStreamOutput, DynamoDbError> {
    let description = resolve_stream_table(service, context, &input.stream_arn)?;
    let stream_view_type = description
        .stream_specification
        .as_ref()
        .and_then(|spec| spec.stream_view_type)
        .unwrap_or(StreamViewType::NewAndOldImages);
    let label = description
        .latest_stream_label
        .clone()
        .unwrap_or_else(|| description.table_id.clone());

    let shard = Shard {
        shard_id: shard_id(&description.table_id),
        parent_shard_id: None,
        sequence_number_range: SequenceNumberRange {
            starting_sequence_number: sequence_number(0),
            ending_sequence_number: None, // open shard — still accepting writes
        },
    };

    Ok(DescribeStreamOutput {
        stream_description: StreamDescription {
            stream_arn: input.stream_arn,
            stream_label: label,
            stream_status: StreamStatus::Enabled,
            stream_view_type,
            table_name: description.table_name,
            key_schema: description.key_schema,
            shards: vec![shard],
            last_evaluated_shard_id: None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use extenddb_core::types::CreateTableInput;
    use nimbus_core::TenantId;
    use serde_json::json;

    fn fixture() -> (Arc<Service>, TenantIsolationContext, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir");
        let service = Arc::new(Service::new(temp.path()).expect("service"));
        let context = crate::tenant::tenant_context(TenantId::new("acme").unwrap(), "test");
        crate::tenant::ensure_tenant(&service, &context).expect("tenant");
        (service, context, temp)
    }

    /// Create a stream-enabled table and return its stream ARN.
    fn create_streamed(service: &Arc<Service>, context: &TenantIsolationContext) -> String {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "events",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_IMAGE" }
        }))
        .unwrap();
        control_plane::create_table(service, context, input)
            .expect("create")
            .table_description
            .latest_stream_arn
            .expect("stream arn")
    }

    #[test]
    fn describe_stream_returns_single_open_shard() {
        let (service, ctx, _t) = fixture();
        let arn = create_streamed(&service, &ctx);
        let out = describe_stream(
            &service,
            &ctx,
            DescribeStreamInput {
                stream_arn: arn.clone(),
                limit: None,
                exclusive_start_shard_id: None,
            },
        )
        .expect("describe stream");
        let desc = out.stream_description;
        assert_eq!(desc.stream_arn, arn);
        assert_eq!(desc.stream_status, StreamStatus::Enabled);
        assert_eq!(desc.stream_view_type, StreamViewType::NewImage);
        assert_eq!(desc.table_name, "events");
        assert_eq!(desc.shards.len(), 1, "single shard");
        assert!(desc.shards[0].shard_id.starts_with("shardId-"));
        assert!(
            desc.shards[0]
                .sequence_number_range
                .ending_sequence_number
                .is_none(),
            "open shard"
        );
    }

    #[test]
    fn describe_stream_unknown_arn_is_resource_not_found() {
        let (service, ctx, _t) = fixture();
        create_streamed(&service, &ctx);
        let err = describe_stream(
            &service,
            &ctx,
            DescribeStreamInput {
                stream_arn:
                    "arn:aws:dynamodb:ddblocal:000000000000:table/events/stream/wrong-label"
                        .to_owned(),
                limit: None,
                exclusive_start_shard_id: None,
            },
        )
        .expect_err("unknown stream rejected");
        assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
    }

    #[test]
    fn describe_stream_for_missing_table_is_resource_not_found() {
        let (service, ctx, _t) = fixture();
        let err = describe_stream(
            &service,
            &ctx,
            DescribeStreamInput {
                stream_arn: "arn:aws:dynamodb:ddblocal:000000000000:table/ghost/stream/x"
                    .to_owned(),
                limit: None,
                exclusive_start_shard_id: None,
            },
        )
        .expect_err("missing table rejected");
        assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
    }
}
