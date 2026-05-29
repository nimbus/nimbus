//! DynamoDB Streams (T5): DescribeStream (D5.2), GetShardIterator (D5.3),
//! GetRecords (D5.4), ListStreams (D5.5).
//!
//! Each stream-enabled table exposes a **single** shard (DDB-DIV-006 — real
//! DynamoDB exposes a shard tree, ExtendDB 4 shards). The stream ARN is
//! `<table_arn>/stream/<label>` (label = table id); sequence numbers are
//! zero-padded i64 strings.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{
    DescribeStreamInput, DescribeStreamOutput, GetShardIteratorInput, GetShardIteratorOutput,
    SequenceNumberRange, Shard, ShardIteratorType, StreamDescription, StreamStatus, StreamViewType,
    TableDescription,
};
use nimbus_core::{StructuredQuery, TableName};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;

use crate::commands::control_plane;
use crate::error::map_core_error;

/// Reserved prefix for a table's per-stream event store (one doc per event,
/// keyed by zero-padded sequence number).
const STREAM_TABLE_PREFIX: &str = "_ddb_stream_";

/// The tenant-scoped table holding `table_name`'s captured stream events.
pub(crate) fn stream_events_table(table_name: &str) -> Result<TableName, DynamoDbError> {
    TableName::new(format!("{STREAM_TABLE_PREFIX}{table_name}")).map_err(map_core_error)
}

/// The number of events currently captured for `table_name`'s stream (= the
/// next sequence number that will be assigned). 0 when no stream store exists.
pub(crate) fn stream_event_count(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<i64, DynamoDbError> {
    let table = stream_events_table(table_name)?;
    match service.query_documents_structured(
        context.tenant_id(),
        &table,
        &StructuredQuery::default(),
    ) {
        Ok(documents) => Ok(documents.len() as i64),
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => Ok(0),
        Err(error) => Err(map_core_error(error)),
    }
}

/// Encode an opaque shard iterator (base64url; format is private to the adapter).
/// The decode side (and `DecodedIterator`) lands with GetRecords (D5.4).
pub(crate) fn encode_iterator(stream_arn: &str, shard_id: &str, next_sequence: i64) -> String {
    URL_SAFE_NO_PAD.encode(format!("{stream_arn}\u{1f}{shard_id}\u{1f}{next_sequence}"))
}

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

/// GetShardIterator: return an opaque iterator positioned per the requested
/// type (TRIM_HORIZON = start, LATEST = end, AT/AFTER = a given sequence).
///
/// # Errors
/// `ResourceNotFoundException` for an unknown stream or shard;
/// `ValidationException` if AT/AFTER is missing a valid `SequenceNumber`.
pub fn get_shard_iterator(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: GetShardIteratorInput,
) -> Result<GetShardIteratorOutput, DynamoDbError> {
    let description = resolve_stream_table(service, context, &input.stream_arn)?;
    let expected_shard = shard_id(&description.table_id);
    if input.shard_id != expected_shard {
        return Err(DynamoDbError::ResourceNotFoundException(format!(
            "Requested resource not found: Shard: {} not found",
            input.shard_id
        )));
    }
    let next_sequence = match input.shard_iterator_type {
        ShardIteratorType::TrimHorizon => 0,
        ShardIteratorType::Latest => stream_event_count(service, context, &description.table_name)?,
        ShardIteratorType::AtSequenceNumber => parse_sequence(input.sequence_number.as_deref())?,
        ShardIteratorType::AfterSequenceNumber => {
            parse_sequence(input.sequence_number.as_deref())? + 1
        }
    };
    Ok(GetShardIteratorOutput {
        shard_iterator: Some(encode_iterator(
            &input.stream_arn,
            &input.shard_id,
            next_sequence,
        )),
    })
}

fn parse_sequence(sequence: Option<&str>) -> Result<i64, DynamoDbError> {
    sequence
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| {
            DynamoDbError::ValidationException(
                "AT_SEQUENCE_NUMBER and AFTER_SEQUENCE_NUMBER require a valid SequenceNumber"
                    .to_owned(),
            )
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

    fn shard_for(service: &Arc<Service>, context: &TenantIsolationContext, arn: &str) -> String {
        describe_stream(
            service,
            context,
            DescribeStreamInput {
                stream_arn: arn.to_owned(),
                limit: None,
                exclusive_start_shard_id: None,
            },
        )
        .expect("describe")
        .stream_description
        .shards[0]
            .shard_id
            .clone()
    }

    /// The `next_sequence` encoded in an iterator (decode is private to D5.4, so
    /// the test reverses the base64url encoding directly).
    fn iterator_next_sequence(iterator: &str) -> i64 {
        let raw = String::from_utf8(URL_SAFE_NO_PAD.decode(iterator).unwrap()).unwrap();
        raw.rsplit('\u{1f}').next().unwrap().parse().unwrap()
    }

    fn get_iter(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        arn: &str,
        shard: &str,
        iter_type: &str,
        seq: Option<&str>,
    ) -> Result<GetShardIteratorOutput, DynamoDbError> {
        let mut input = json!({
            "StreamArn": arn,
            "ShardId": shard,
            "ShardIteratorType": iter_type,
        });
        if let Some(seq) = seq {
            input["SequenceNumber"] = json!(seq);
        }
        get_shard_iterator(service, context, serde_json::from_value(input).unwrap())
    }

    #[test]
    fn get_shard_iterator_each_type() {
        let (service, ctx, _t) = fixture();
        let arn = create_streamed(&service, &ctx);
        let shard = shard_for(&service, &ctx, &arn);

        let trim = get_iter(&service, &ctx, &arn, &shard, "TRIM_HORIZON", None)
            .expect("trim horizon")
            .shard_iterator
            .expect("iterator");
        assert_eq!(iterator_next_sequence(&trim), 0, "TRIM_HORIZON starts at 0");

        let latest = get_iter(&service, &ctx, &arn, &shard, "LATEST", None)
            .expect("latest")
            .shard_iterator
            .expect("iterator");
        assert_eq!(
            iterator_next_sequence(&latest),
            0,
            "LATEST starts at the current end (0 with no records yet)"
        );

        let at = get_iter(
            &service,
            &ctx,
            &arn,
            &shard,
            "AT_SEQUENCE_NUMBER",
            Some("5"),
        )
        .expect("at")
        .shard_iterator
        .expect("iterator");
        assert_eq!(
            iterator_next_sequence(&at),
            5,
            "AT reads from the given sequence"
        );

        let after = get_iter(
            &service,
            &ctx,
            &arn,
            &shard,
            "AFTER_SEQUENCE_NUMBER",
            Some("5"),
        )
        .expect("after")
        .shard_iterator
        .expect("iterator");
        assert_eq!(
            iterator_next_sequence(&after),
            6,
            "AFTER reads past the sequence"
        );
    }

    #[test]
    fn get_shard_iterator_at_without_sequence_is_validation_error() {
        let (service, ctx, _t) = fixture();
        let arn = create_streamed(&service, &ctx);
        let shard = shard_for(&service, &ctx, &arn);
        let err = get_iter(&service, &ctx, &arn, &shard, "AT_SEQUENCE_NUMBER", None)
            .expect_err("missing sequence");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn get_shard_iterator_unknown_shard_is_resource_not_found() {
        let (service, ctx, _t) = fixture();
        let arn = create_streamed(&service, &ctx);
        let err = get_iter(&service, &ctx, &arn, "shardId-nope", "TRIM_HORIZON", None)
            .expect_err("unknown shard");
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
