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
    GetShardIteratorInput, GetShardIteratorOutput, Item, ListStreamsInput, ListStreamsOutput,
    SequenceNumberRange, Shard, ShardIteratorType, StreamDescription, StreamEventName,
    StreamRecord, StreamRecordData, StreamStatus, StreamSummary, StreamViewType, TableDescription,
    UserIdentity,
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
    /// Set for service-originated changes (TTL deletions carry the DynamoDB
    /// service principal — D6.2). Absent for ordinary client writes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_identity: Option<UserIdentity>,
}

/// The `userIdentity` DynamoDB attaches to a TTL-originated REMOVE record.
pub(crate) fn ttl_user_identity() -> UserIdentity {
    UserIdentity {
        identity_type: "Service".to_owned(),
        principal_id: "dynamodb.amazonaws.com".to_owned(),
    }
}

/// Reserved prefix for a table's per-stream event store (one doc per event,
/// keyed by zero-padded sequence number).
const STREAM_TABLE_PREFIX: &str = "_ddb_stream_";

/// The tenant-scoped table holding `table_name`'s captured stream events.
pub(crate) fn stream_events_table(table_name: &str) -> Result<TableName, DynamoDbError> {
    TableName::new(format!("{STREAM_TABLE_PREFIX}{table_name}")).map_err(map_core_error)
}

/// Reserved prefix for a stream's monotonic sequence counter store (one doc).
/// Kept separate from the event store so reclaiming expired events never resets
/// the high-water mark — DynamoDB sequence numbers are monotonic for the life
/// of the stream, independent of retention.
const STREAM_SEQ_PREFIX: &str = "_ddb_streamseq_";

/// The tenant-scoped table holding `table_name`'s stream sequence counter.
fn stream_seq_table(table_name: &str) -> Result<TableName, DynamoDbError> {
    TableName::new(format!("{STREAM_SEQ_PREFIX}{table_name}")).map_err(map_core_error)
}

/// The fixed document id of a stream's sequence counter.
fn seq_counter_id() -> Result<DocumentId, DynamoDbError> {
    DocumentId::from_key("counter").map_err(map_core_error)
}

/// The next sequence number to assign for `table_name`'s stream (the monotonic
/// high-water mark). 0 before the first event; survives event reclamation.
///
/// # Errors
/// A mapped engine error if the counter cannot be read.
pub(crate) fn next_sequence_value(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<i64, DynamoDbError> {
    match service.get_document(
        context.tenant_id(),
        &stream_seq_table(table_name)?,
        seq_counter_id()?,
    ) {
        Ok(document) => Ok(document
            .fields
            .get("next")
            .and_then(Value::as_i64)
            .unwrap_or(0)),
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => Ok(0),
        Err(error) => Err(map_core_error(error)),
    }
}

/// Persist the next sequence number for `table_name`'s stream (upsert).
fn set_sequence_value(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
    next: i64,
) -> Result<(), DynamoDbError> {
    let table = stream_seq_table(table_name)?;
    let id = seq_counter_id()?;
    let mut fields = Map::new();
    fields.insert("next".to_owned(), Value::from(next));
    match service.get_document(context.tenant_id(), &table, id.clone()) {
        Ok(_) => {
            service
                .update_document(context.tenant_id(), table, id, fields)
                .map_err(map_core_error)?;
        }
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            service
                .insert_document_with_id(context.tenant_id(), table, id, fields)
                .map_err(map_core_error)?;
        }
        Err(error) => return Err(map_core_error(error)),
    }
    Ok(())
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

/// A change to capture on a table's stream. Bundles the per-event inputs so the
/// capture entrypoint stays a small, readable call.
pub(crate) struct ChangeEvent<'a> {
    pub event_name: StreamEventName,
    pub keys: &'a Item,
    pub old_image: Option<&'a Item>,
    pub new_image: Option<&'a Item>,
    /// Set for service-originated changes (TTL deletions); `None` for ordinary
    /// client writes.
    pub user_identity: Option<UserIdentity>,
}

/// Capture a change event for `table_name` if it has a stream enabled
/// (otherwise a no-op). Called by the write handlers after a successful
/// mutation. Sequence numbers are assigned from the persistent high-water
/// counter and the counter is then advanced — monotonic across event
/// reclamation (a read-modify-write that is correct on the single-node write
/// path; making the counter bump atomic under concurrent writers is a
/// follow-up).
///
/// # Errors
/// A mapped engine error if the event cannot be persisted.
pub(crate) fn capture_event(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
    change: ChangeEvent<'_>,
) -> Result<(), DynamoDbError> {
    let description = control_plane::load_table_description(service, context, table_name)?;
    if !description
        .stream_specification
        .as_ref()
        .is_some_and(|spec| spec.stream_enabled)
    {
        return Ok(());
    }
    let seq = next_sequence_value(service, context, table_name)?;
    let event = StoredEvent {
        seq,
        created: epoch_seconds(),
        event_name: event_name_str(change.event_name).to_owned(),
        keys: item_to_fields(change.keys)?,
        old_image: change.old_image.map(item_to_fields).transpose()?,
        new_image: change.new_image.map(item_to_fields).transpose()?,
        user_identity: change.user_identity,
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
    // Advance the high-water mark so the next event gets a fresh sequence even
    // after expired events are reclaimed.
    set_sequence_value(service, context, table_name, seq + 1)?;
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
/// Stream record retention window (DynamoDB retains stream records for 24h).
const STREAM_RETENTION_SECS: i64 = 86_400;
/// The DynamoDB per-call cap for ListStreams.
const MAX_LIST_STREAMS: usize = 100;

/// ListStreams: enumerate stream-enabled tables (optionally filtered by
/// `TableName`), paginated by `ExclusiveStartStreamArn`/`Limit`.
///
/// # Errors
/// A mapped engine error if the catalog cannot be read.
pub fn list_streams(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: ListStreamsInput,
) -> Result<ListStreamsOutput, DynamoDbError> {
    let mut summaries: Vec<StreamSummary> =
        control_plane::list_table_descriptions(service, context)?
            .into_iter()
            .filter(|description| {
                description
                    .stream_specification
                    .as_ref()
                    .is_some_and(|spec| spec.stream_enabled)
            })
            .filter(|description| {
                input
                    .table_name
                    .as_ref()
                    .is_none_or(|name| &description.table_name == name)
            })
            .filter_map(|description| {
                Some(StreamSummary {
                    stream_arn: description.latest_stream_arn?,
                    stream_label: description.latest_stream_label?,
                    table_name: description.table_name,
                })
            })
            .collect();
    summaries.sort_by(|a, b| a.stream_arn.cmp(&b.stream_arn));

    if let Some(start) = &input.exclusive_start_stream_arn {
        summaries.retain(|summary| summary.stream_arn.as_str() > start.as_str());
    }
    let limit = input
        .limit
        .filter(|limit| *limit > 0)
        .map(|limit| (limit as usize).min(MAX_LIST_STREAMS))
        .unwrap_or(MAX_LIST_STREAMS);
    let truncated = summaries.len() > limit;
    summaries.truncate(limit);
    let last_evaluated_stream_arn = truncated
        .then(|| summaries.last().map(|summary| summary.stream_arn.clone()))
        .flatten();

    Ok(ListStreamsOutput {
        streams: summaries,
        last_evaluated_stream_arn,
    })
}

/// Reclaim event docs older than `cutoff`, returning the count reclaimed. The
/// `events` are the already-read store contents; doc ids are reconstructed
/// deterministically from their sequence numbers, so no extra query is needed.
/// The sequence counter is a separate store and is left untouched, so the
/// high-water mark stays monotonic.
///
/// # Errors
/// A mapped engine error if an expired event cannot be deleted.
fn reclaim_expired_events(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
    events: &[StoredEvent],
    cutoff: i64,
) -> Result<usize, DynamoDbError> {
    let table = stream_events_table(table_name)?;
    let mut reclaimed = 0;
    for event in events.iter().filter(|event| event.created < cutoff) {
        let id = DocumentId::from_key(sequence_number(event.seq)).map_err(map_core_error)?;
        service
            .delete_document(context.tenant_id(), table.clone(), id)
            .map_err(map_core_error)?;
        reclaimed += 1;
    }
    Ok(reclaimed)
}

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

    let all_events = read_events(service, context, &description.table_name)?;
    let limit = input
        .limit
        .filter(|limit| *limit > 0)
        .map(|limit| (limit as usize).min(MAX_GET_RECORDS))
        .unwrap_or(MAX_GET_RECORDS);
    let cutoff = epoch_seconds() - STREAM_RETENTION_SECS;

    // The window the iterator advances over: events at/after the iterator,
    // capped at the page limit. Expired events stay in the window so the
    // iterator still advances past them — a re-poll never stalls on records the
    // retention window has dropped.
    let window: Vec<&StoredEvent> = all_events
        .iter()
        .filter(|event| event.seq >= iterator.next_sequence)
        .take(limit)
        .collect();
    let next_sequence = window
        .last()
        .map_or(iterator.next_sequence, |event| event.seq + 1);

    // The returned batch excludes anything past the 24h retention window.
    let records = window
        .into_iter()
        .filter(|event| event.created >= cutoff)
        .map(|event| shape_record(event, view_type))
        .collect::<Result<Vec<_>, _>>()?;

    // Reclaim expired event storage on poll (the sequence counter is a separate
    // store, so the high-water mark is unaffected).
    reclaim_expired_events(
        service,
        context,
        &description.table_name,
        &all_events,
        cutoff,
    )?;

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
        user_identity: event.user_identity.clone(),
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
        ShardIteratorType::Latest => {
            next_sequence_value(service, context, &description.table_name)?
        }
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
            next_sequence_value(&service, &ctx, "plain").unwrap(),
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

    // ---- D5.5: ListStreams + retention ----

    /// Create a stream-enabled table with the given name; returns its ARN.
    fn create_streamed_named(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        name: &str,
    ) -> String {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": name,
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_IMAGE" }
        }))
        .unwrap();
        control_plane::create_table(service, context, input)
            .expect("create")
            .table_description
            .latest_stream_arn
            .expect("arn")
    }

    /// Create a table without a stream (it must be excluded from ListStreams).
    fn create_plain_named(service: &Arc<Service>, context: &TenantIsolationContext, name: &str) {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": name,
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        }))
        .unwrap();
        control_plane::create_table(service, context, input).expect("create");
    }

    fn list(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        table_name: Option<&str>,
        limit: Option<i64>,
        start: Option<&str>,
    ) -> ListStreamsOutput {
        list_streams(
            service,
            context,
            ListStreamsInput {
                table_name: table_name.map(str::to_owned),
                limit,
                exclusive_start_stream_arn: start.map(str::to_owned),
            },
        )
        .expect("list streams")
    }

    /// Persist a stream event directly with a chosen sequence/timestamp, so
    /// retention can be tested without waiting out the 24h window.
    fn inject_event(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        table_name: &str,
        seq: i64,
        created: i64,
    ) {
        let event = StoredEvent {
            seq,
            created,
            event_name: "INSERT".to_owned(),
            keys: json!({ "pk": { "S": format!("k{seq}") } })
                .as_object()
                .unwrap()
                .clone(),
            old_image: None,
            new_image: Some(
                json!({ "pk": { "S": format!("k{seq}") }, "v": { "N": "1" } })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            user_identity: None,
        };
        let Value::Object(fields) = serde_json::to_value(&event).unwrap() else {
            panic!("event serializes to an object");
        };
        let id = DocumentId::from_key(sequence_number(seq)).unwrap();
        service
            .insert_document_with_id(
                context.tenant_id(),
                stream_events_table(table_name).unwrap(),
                id,
                fields,
            )
            .expect("inject event");
    }

    #[test]
    fn list_streams_enumerates_only_stream_enabled_tables() {
        let (service, ctx, _t) = fixture();
        let alpha = create_streamed_named(&service, &ctx, "alpha");
        let beta = create_streamed_named(&service, &ctx, "beta");
        create_plain_named(&service, &ctx, "gamma");

        let out = list(&service, &ctx, None, None, None);
        assert_eq!(out.streams.len(), 2, "only the two streamed tables");
        let arns: Vec<&str> = out.streams.iter().map(|s| s.stream_arn.as_str()).collect();
        assert!(arns.contains(&alpha.as_str()) && arns.contains(&beta.as_str()));
        let tables: Vec<&str> = out.streams.iter().map(|s| s.table_name.as_str()).collect();
        assert!(tables.contains(&"alpha") && tables.contains(&"beta"));
        assert!(!tables.contains(&"gamma"), "plain table excluded");
        assert!(
            out.streams.iter().all(|s| !s.stream_label.is_empty()),
            "every summary carries a label"
        );
        assert!(out.last_evaluated_stream_arn.is_none(), "fully enumerated");
    }

    #[test]
    fn list_streams_filters_by_table_name() {
        let (service, ctx, _t) = fixture();
        create_streamed_named(&service, &ctx, "alpha");
        create_streamed_named(&service, &ctx, "beta");

        let out = list(&service, &ctx, Some("alpha"), None, None);
        assert_eq!(out.streams.len(), 1);
        assert_eq!(out.streams[0].table_name, "alpha");

        let none = list(&service, &ctx, Some("nonexistent"), None, None);
        assert!(none.streams.is_empty(), "no match for an unknown table");
    }

    #[test]
    fn list_streams_paginates_with_limit_and_exclusive_start() {
        let (service, ctx, _t) = fixture();
        for name in ["alpha", "beta", "gamma"] {
            create_streamed_named(&service, &ctx, name);
        }
        let page1 = list(&service, &ctx, None, Some(2), None);
        assert_eq!(page1.streams.len(), 2);
        let cursor = page1
            .last_evaluated_stream_arn
            .clone()
            .expect("more pages remain");
        assert_eq!(
            cursor, page1.streams[1].stream_arn,
            "cursor is the last returned ARN"
        );

        let page2 = list(&service, &ctx, None, Some(2), Some(&cursor));
        assert_eq!(page2.streams.len(), 1, "the final page");
        assert!(
            page2.last_evaluated_stream_arn.is_none(),
            "no more pages after the last"
        );
        // The two pages cover all three streams with no overlap.
        let mut seen: Vec<&str> = page1
            .streams
            .iter()
            .chain(page2.streams.iter())
            .map(|s| s.stream_arn.as_str())
            .collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 3, "all three streams, no duplicates");
    }

    #[test]
    fn get_records_skips_expired_events_and_reclaims_their_storage() {
        let (service, ctx, _t) = fixture();
        let arn = create_streamed_named(&service, &ctx, "events");
        let now = epoch_seconds();
        inject_event(
            &service,
            &ctx,
            "events",
            0,
            now - STREAM_RETENTION_SECS - 100,
        ); // expired
        inject_event(&service, &ctx, "events", 1, now); // fresh
        assert_eq!(read_events(&service, &ctx, "events").unwrap().len(), 2);

        let out = all_records(&service, &ctx, &arn);
        assert_eq!(out.records.len(), 1, "the expired event is not returned");
        assert_eq!(
            out.records[0].dynamodb.keys.get("pk"),
            Some(&extenddb_core::types::AttributeValue::S("k1".into())),
            "the surviving record is the fresh one"
        );
        let next = out.next_shard_iterator.expect("iterator advances");
        assert_eq!(
            iterator_next_sequence(&next),
            2,
            "the iterator advances past the expired event so re-polling never stalls"
        );
        assert_eq!(
            read_events(&service, &ctx, "events").unwrap().len(),
            1,
            "the expired event's storage is reclaimed on poll"
        );
    }

    #[test]
    fn reclaiming_expired_events_preserves_the_monotonic_sequence() {
        let (service, ctx, _t) = fixture();
        let arn = create_streamed_named(&service, &ctx, "events");
        let now = epoch_seconds();
        // Two events that have both aged out of the retention window, with the
        // sequence counter advanced past them (as real capture would leave it).
        inject_event(
            &service,
            &ctx,
            "events",
            0,
            now - STREAM_RETENTION_SECS - 100,
        );
        inject_event(
            &service,
            &ctx,
            "events",
            1,
            now - STREAM_RETENTION_SECS - 100,
        );
        set_sequence_value(&service, &ctx, "events", 2).expect("counter");

        // A poll returns nothing (all expired) and reclaims both event docs.
        let out = all_records(&service, &ctx, &arn);
        assert!(out.records.is_empty(), "all events expired");
        assert_eq!(
            read_events(&service, &ctx, "events").unwrap().len(),
            0,
            "expired storage reclaimed"
        );

        // The high-water mark is preserved: the next captured event keeps
        // climbing rather than colliding with a consumer's advanced iterator.
        assert_eq!(
            next_sequence_value(&service, &ctx, "events").unwrap(),
            2,
            "reclamation does not reset the counter"
        );
        put(&service, &ctx, "z", "9");
        let fresh = read_events(&service, &ctx, "events").unwrap();
        assert_eq!(fresh.len(), 1);
        assert_eq!(
            fresh[0].seq, 2,
            "the new event continues past the reclaimed sequences"
        );
    }
}
