//! DynamoDB Streams (T5): DescribeStream (D5.2), GetShardIterator (D5.3),
//! GetRecords (D5.4), ListStreams (D5.5).
//!
//! Each stream-enabled table exposes a **single** shard (DDB-DIV-006 — real
//! DynamoDB exposes a shard tree, ExtendDB 4 shards). The stream ARN is
//! `<table_arn>/stream/<label>` (label = table id); sequence numbers are
//! zero-padded i64 strings.

use std::sync::Arc;

use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{
    DescribeStreamInput, DescribeStreamOutput, GetRecordsInput, GetRecordsOutput,
    GetShardIteratorInput, GetShardIteratorOutput, Item, SequenceNumberRange, Shard,
    ShardIteratorType, StreamDescription, StreamEventName, StreamRecord, StreamRecordData,
    StreamStatus, StreamViewType, TableDescription,
};
use nimbus_core::{DocumentId, StructuredQuery, TableName};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::attribute_value::{fields_to_item, item_to_fields};
use crate::commands::control_plane;
use crate::error::map_core_error;

/// A captured change event, persisted as one document in the stream store. Keys
/// and images are stored in AttributeValue wire-JSON (like data items).
#[derive(Serialize, Deserialize)]
struct StoredEvent {
    seq: i64,
    created: i64,
    event_name: String,
    keys: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    old_image: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    new_image: Option<Map<String, Value>>,
}

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
pub(crate) fn encode_iterator(stream_arn: &str, shard_id: &str, next_sequence: i64) -> String {
    URL_SAFE_NO_PAD.encode(format!("{stream_arn}\u{1f}{shard_id}\u{1f}{next_sequence}"))
}

/// A decoded shard iterator: the stream + shard it walks and the next sequence
/// number to read.
struct DecodedIterator {
    stream_arn: String,
    shard_id: String,
    next_sequence: i64,
}

fn decode_iterator(token: &str) -> Result<DecodedIterator, DynamoDbError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| invalid_iterator())?;
    let raw = String::from_utf8(bytes).map_err(|_| invalid_iterator())?;
    let mut parts = raw.split('\u{1f}');
    let stream_arn = parts.next().ok_or_else(invalid_iterator)?.to_owned();
    let shard_id = parts.next().ok_or_else(invalid_iterator)?.to_owned();
    let next_sequence = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(invalid_iterator)?;
    Ok(DecodedIterator {
        stream_arn,
        shard_id,
        next_sequence,
    })
}

fn invalid_iterator() -> DynamoDbError {
    DynamoDbError::ValidationException("The shard iterator is invalid".to_owned())
}

fn epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

fn event_name_str(event: StreamEventName) -> &'static str {
    match event {
        StreamEventName::Insert => "INSERT",
        StreamEventName::Modify => "MODIFY",
        StreamEventName::Remove => "REMOVE",
    }
}

fn event_name_from_str(value: &str) -> StreamEventName {
    match value {
        "MODIFY" => StreamEventName::Modify,
        "REMOVE" => StreamEventName::Remove,
        _ => StreamEventName::Insert,
    }
}

/// Capture a change event for `table_name` if it has a stream enabled
/// (otherwise a no-op). Called by the write handlers after a successful
/// mutation. Sequence numbers are assigned from the current event count
/// (sufficient for the single-node write path; an atomic counter is a
/// concurrency follow-up).
///
/// # Errors
/// A mapped engine error if the event cannot be persisted.
pub(crate) fn capture_event(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
    event_name: StreamEventName,
    keys: &Item,
    old_image: Option<&Item>,
    new_image: Option<&Item>,
) -> Result<(), DynamoDbError> {
    let description = control_plane::load_table_description(service, context, table_name)?;
    if !description
        .stream_specification
        .as_ref()
        .is_some_and(|spec| spec.stream_enabled)
    {
        return Ok(());
    }
    let seq = stream_event_count(service, context, table_name)?;
    let event = StoredEvent {
        seq,
        created: epoch_seconds(),
        event_name: event_name_str(event_name).to_owned(),
        keys: item_to_fields(keys)?,
        old_image: old_image.map(item_to_fields).transpose()?,
        new_image: new_image.map(item_to_fields).transpose()?,
    };
    let fields = match serde_json::to_value(&event) {
        Ok(Value::Object(map)) => map,
        _ => {
            return Err(DynamoDbError::InternalServerError(
                "failed to serialize stream event".to_owned(),
            ));
        }
    };
    let id = DocumentId::from_key(sequence_number(seq)).map_err(map_core_error)?;
    service
        .insert_document_with_id(
            context.tenant_id(),
            stream_events_table(table_name)?,
            id,
            fields,
        )
        .map_err(map_core_error)?;
    Ok(())
}

/// Read all captured events for a table's stream, ascending by sequence.
fn read_events(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<Vec<StoredEvent>, DynamoDbError> {
    let table = stream_events_table(table_name)?;
    let documents = match service.query_documents_structured(
        context.tenant_id(),
        &table,
        &StructuredQuery::default(),
    ) {
        Ok(documents) => documents,
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(map_core_error(error)),
    };
    let mut events: Vec<StoredEvent> = documents
        .iter()
        .map(|document| {
            serde_json::from_value(Value::Object(document.fields.clone())).map_err(|error| {
                DynamoDbError::InternalServerError(format!("corrupt stream event: {error}"))
            })
        })
        .collect::<Result<_, _>>()?;
    events.sort_by_key(|event| event.seq);
    Ok(events)
}

/// The DynamoDB per-call record cap for GetRecords.
const MAX_GET_RECORDS: usize = 1000;

/// GetRecords: return the events at/after the iterator's position (≤1000),
/// shaped per the table's StreamViewType, plus an advanced NextShardIterator.
///
/// # Errors
/// `ValidationException` for a malformed iterator; `ResourceNotFoundException`
/// if the stream no longer exists.
pub fn get_records(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: GetRecordsInput,
) -> Result<GetRecordsOutput, DynamoDbError> {
    let iterator = decode_iterator(&input.shard_iterator)?;
    let description = resolve_stream_table(service, context, &iterator.stream_arn)?;
    let view_type = description
        .stream_specification
        .as_ref()
        .and_then(|spec| spec.stream_view_type)
        .unwrap_or(StreamViewType::NewAndOldImages);

    let mut events = read_events(service, context, &description.table_name)?;
    events.retain(|event| event.seq >= iterator.next_sequence);
    let limit = input
        .limit
        .filter(|limit| *limit > 0)
        .map(|limit| (limit as usize).min(MAX_GET_RECORDS))
        .unwrap_or(MAX_GET_RECORDS);
    events.truncate(limit);

    let next_sequence = events
        .last()
        .map_or(iterator.next_sequence, |event| event.seq + 1);
    let records = events
        .iter()
        .map(|event| shape_record(event, view_type))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(GetRecordsOutput {
        records,
        // The single shard never closes, so a NextShardIterator is always returned.
        next_shard_iterator: Some(encode_iterator(
            &iterator.stream_arn,
            &iterator.shard_id,
            next_sequence,
        )),
    })
}

/// Shape one stored event into a `StreamRecord` per the configured view type.
fn shape_record(
    event: &StoredEvent,
    view_type: StreamViewType,
) -> Result<StreamRecord, DynamoDbError> {
    let keys = fields_to_item(&event.keys)?;
    let new_image = match view_type {
        StreamViewType::NewImage | StreamViewType::NewAndOldImages => {
            event.new_image.as_ref().map(fields_to_item).transpose()?
        }
        _ => None,
    };
    let old_image = match view_type {
        StreamViewType::OldImage | StreamViewType::NewAndOldImages => {
            event.old_image.as_ref().map(fields_to_item).transpose()?
        }
        _ => None,
    };
    Ok(StreamRecord {
        event_id: sequence_number(event.seq),
        event_name: event_name_from_str(&event.event_name),
        event_version: "1.1".to_owned(),
        event_source: "aws:dynamodb".to_owned(),
        aws_region: "ddblocal".to_owned(),
        dynamodb: StreamRecordData {
            approximate_creation_date_time: event.created,
            keys,
            new_image,
            old_image,
            sequence_number: sequence_number(event.seq),
            size_bytes: 0,
            stream_view_type: view_type,
        },
        user_identity: None,
    })
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

    // ---- D5.4: GetRecords + event capture ----

    /// Create a stream-enabled table with the given view type; returns the ARN.
    fn streamed_table(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        view_type: &str,
    ) -> String {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "events",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": view_type }
        }))
        .unwrap();
        control_plane::create_table(service, context, input)
            .expect("create")
            .table_description
            .latest_stream_arn
            .expect("arn")
    }

    fn put(service: &Arc<Service>, context: &TenantIsolationContext, pk: &str, v: &str) {
        crate::commands::item::put_item(
            service,
            context,
            serde_json::from_value(json!({
                "TableName": "events",
                "Item": { "pk": {"S": pk}, "v": {"N": v} },
            }))
            .unwrap(),
        )
        .expect("put");
    }

    fn delete(service: &Arc<Service>, context: &TenantIsolationContext, pk: &str) {
        crate::commands::item::delete_item(
            service,
            context,
            serde_json::from_value(json!({ "TableName": "events", "Key": { "pk": {"S": pk} } }))
                .unwrap(),
        )
        .expect("delete");
    }

    /// TRIM_HORIZON iterator + GetRecords from the start.
    fn all_records(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        arn: &str,
    ) -> GetRecordsOutput {
        let shard = shard_for(service, context, arn);
        let iter = get_shard_iterator(
            service,
            context,
            GetShardIteratorInput {
                stream_arn: arn.to_owned(),
                shard_id: shard,
                shard_iterator_type: ShardIteratorType::TrimHorizon,
                sequence_number: None,
            },
        )
        .expect("iterator")
        .shard_iterator
        .expect("iterator");
        get_records(
            service,
            context,
            GetRecordsInput {
                shard_iterator: iter,
                limit: None,
            },
        )
        .expect("get records")
    }

    #[test]
    fn get_records_new_and_old_images_for_insert_modify_remove() {
        let (service, ctx, _t) = fixture();
        let arn = streamed_table(&service, &ctx, "NEW_AND_OLD_IMAGES");
        put(&service, &ctx, "a", "1"); // INSERT
        put(&service, &ctx, "a", "2"); // MODIFY
        delete(&service, &ctx, "a"); // REMOVE
        let out = all_records(&service, &ctx, &arn);
        assert_eq!(out.records.len(), 3);

        let insert = &out.records[0];
        assert_eq!(insert.event_name, StreamEventName::Insert);
        assert!(
            insert.dynamodb.old_image.is_none(),
            "INSERT has no old image"
        );
        assert_eq!(
            insert.dynamodb.new_image.as_ref().unwrap().get("v"),
            Some(&extenddb_core::types::AttributeValue::N("1".into()))
        );

        let modify = &out.records[1];
        assert_eq!(modify.event_name, StreamEventName::Modify);
        assert_eq!(
            modify.dynamodb.old_image.as_ref().unwrap().get("v"),
            Some(&extenddb_core::types::AttributeValue::N("1".into()))
        );
        assert_eq!(
            modify.dynamodb.new_image.as_ref().unwrap().get("v"),
            Some(&extenddb_core::types::AttributeValue::N("2".into()))
        );

        let remove = &out.records[2];
        assert_eq!(remove.event_name, StreamEventName::Remove);
        assert!(
            remove.dynamodb.new_image.is_none(),
            "REMOVE has no new image"
        );
        assert!(
            remove.dynamodb.old_image.is_some(),
            "REMOVE carries the old image"
        );

        assert!(
            out.next_shard_iterator.is_some(),
            "open shard always advances"
        );
    }

    #[test]
    fn get_records_keys_only_view_omits_images() {
        let (service, ctx, _t) = fixture();
        let arn = streamed_table(&service, &ctx, "KEYS_ONLY");
        put(&service, &ctx, "a", "1");
        let out = all_records(&service, &ctx, &arn);
        let record = &out.records[0];
        assert!(record.dynamodb.new_image.is_none() && record.dynamodb.old_image.is_none());
        assert_eq!(
            record.dynamodb.keys.get("pk"),
            Some(&extenddb_core::types::AttributeValue::S("a".into()))
        );
    }

    #[test]
    fn get_records_new_image_only() {
        let (service, ctx, _t) = fixture();
        let arn = streamed_table(&service, &ctx, "NEW_IMAGE");
        put(&service, &ctx, "a", "1");
        put(&service, &ctx, "a", "2");
        let out = all_records(&service, &ctx, &arn);
        let modify = &out.records[1];
        assert!(
            modify.dynamodb.old_image.is_none(),
            "NEW_IMAGE omits the old image"
        );
        assert!(modify.dynamodb.new_image.is_some());
    }

    #[test]
    fn get_records_iterator_advances_and_pages() {
        let (service, ctx, _t) = fixture();
        let arn = streamed_table(&service, &ctx, "NEW_IMAGE");
        for v in ["1", "2", "3"] {
            put(&service, &ctx, "a", v);
        }
        let shard = shard_for(&service, &ctx, &arn);
        // Page 1: limit 2.
        let iter1 = get_shard_iterator(
            &service,
            &ctx,
            GetShardIteratorInput {
                stream_arn: arn.clone(),
                shard_id: shard,
                shard_iterator_type: ShardIteratorType::TrimHorizon,
                sequence_number: None,
            },
        )
        .expect("iter")
        .shard_iterator
        .expect("iter");
        let page1 = get_records(
            &service,
            &ctx,
            GetRecordsInput {
                shard_iterator: iter1,
                limit: Some(2),
            },
        )
        .expect("page1");
        assert_eq!(page1.records.len(), 2);
        // Page 2: continue from NextShardIterator.
        let page2 = get_records(
            &service,
            &ctx,
            GetRecordsInput {
                shard_iterator: page1.next_shard_iterator.expect("next"),
                limit: Some(2),
            },
        )
        .expect("page2");
        assert_eq!(page2.records.len(), 1, "the iterator advanced past page 1");
    }

    #[test]
    fn capture_is_skipped_for_non_stream_tables() {
        let (service, ctx, _t) = fixture();
        // Table without a stream — writes produce no events, and there is no
        // stream store to read.
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "plain",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        }))
        .unwrap();
        control_plane::create_table(&service, &ctx, input).expect("create");
        crate::commands::item::put_item(
            &service,
            &ctx,
            serde_json::from_value(json!({ "TableName": "plain", "Item": { "pk": {"S": "a"} } }))
                .unwrap(),
        )
        .expect("put");
        assert_eq!(
            stream_event_count(&service, &ctx, "plain").unwrap(),
            0,
            "no events captured for a non-stream table"
        );
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
