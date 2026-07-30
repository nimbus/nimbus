//! DynamoDB Streams (T5): DescribeStream (D5.2), GetShardIterator (D5.3),
//! GetRecords (D5.4), ListStreams (D5.5).
//!
//! Each stream-enabled table exposes a **single** shard (DDB-DIV-006 — real
//! DynamoDB exposes a shard tree, ExtendDB 4 shards). The stream ARN is
//! `<table_arn>/stream/<label>` (label = table id); sequence numbers are
//! zero-padded i64 strings.

use std::sync::Arc;

use base64::Engine as Base64Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{
    DescribeStreamInput, DescribeStreamOutput, GetRecordsInput, GetRecordsOutput,
    GetShardIteratorInput, GetShardIteratorOutput, Item, ListStreamsInput, ListStreamsOutput,
    SequenceNumberRange, Shard, ShardIteratorType, StreamDescription, StreamEventName,
    StreamRecord, StreamRecordData, StreamStatus, StreamSummary, StreamViewType, TableDescription,
    UserIdentity,
};
use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, DocumentId, DocumentLocator, StructuredQuery, TableName,
    WriteKey, WritePrecondition, WriteSetMode,
};
use nimbus_engine::{Engine, MutationActor};
use nimbus_tenant::TenantIsolationContext;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::attribute_value::{fields_to_item, item_to_fields};
use crate::commands::{control_plane, item};
use crate::error::map_core_error;
use crate::tenant::{adapter_principal, caller_principal};

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

/// Delete every document in `table`, tolerating a never-materialized table.
fn delete_all(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table: &TableName,
) -> Result<(), DynamoDbError> {
    let principal = adapter_principal();
    let documents = match engine.query_documents_structured_with_principal(
        context.tenant_id(),
        table,
        &StructuredQuery::default(),
        &principal,
    ) {
        Ok(documents) => documents,
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            return Ok(());
        }
        Err(error) => return Err(map_core_error(error)),
    };
    for document in documents {
        engine
            .delete_document_with(
                context.tenant_id(),
                table.clone(),
                document.id,
                MutationActor::with_principal(&principal),
            )
            .map_err(map_core_error)?;
    }
    Ok(())
}

/// Drop all of `table_name`'s stream state — its captured events **and** its
/// sequence high-water counter — when the table is deleted (F4). A table later
/// recreated under the same name then starts a fresh stream at sequence 0
/// rather than inheriting a stale high-water mark.
pub(crate) fn reclaim_for_table(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<(), DynamoDbError> {
    delete_all(engine, context, &stream_events_table(table_name)?)?;
    delete_all(engine, context, &stream_seq_table(table_name)?)?;
    Ok(())
}

/// The next sequence number to assign for `table_name`'s stream (the monotonic
/// high-water mark). 0 before the first event; survives event reclamation.
///
/// # Errors
/// A mapped engine error if the counter cannot be read.
pub(crate) fn next_sequence_value(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<i64, DynamoDbError> {
    match engine.get_document_with_principal(
        context.tenant_id(),
        &stream_seq_table(table_name)?,
        seq_counter_id()?,
        &adapter_principal(),
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

/// Persist the next sequence number for `table_name`'s stream (upsert). Only
/// used by tests to seed the counter; the live write path advances the counter
/// atomically inside [`execute_atomic_write_batch_with_streams`]'s single-batch
/// write.
#[cfg(test)]
fn set_sequence_value(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
    next: i64,
) -> Result<(), DynamoDbError> {
    let table = stream_seq_table(table_name)?;
    let id = seq_counter_id()?;
    let mut fields = Map::new();
    fields.insert("next".to_owned(), Value::from(next));
    let principal = adapter_principal();
    match engine.get_document_with_principal(context.tenant_id(), &table, id.clone(), &principal) {
        Ok(_) => {
            engine
                .update_document_with(
                    context.tenant_id(),
                    table,
                    id,
                    fields,
                    MutationActor::with_principal(&principal),
                )
                .map_err(map_core_error)?;
        }
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            engine
                .insert_document_with(
                    context.tenant_id(),
                    table,
                    Some(id),
                    fields,
                    MutationActor::with_principal(&principal),
                )
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
    nimbus_core::clock::system_now_secs() as i64
}

fn event_name_str(event: StreamEventName) -> &'static str {
    match event {
        StreamEventName::Insert => "INSERT",
        StreamEventName::Modify => "MODIFY",
        StreamEventName::Remove => "REMOVE",
    }
}

fn event_name_from_str(value: &str) -> Result<StreamEventName, DynamoDbError> {
    match value {
        "INSERT" => Ok(StreamEventName::Insert),
        "MODIFY" => Ok(StreamEventName::Modify),
        "REMOVE" => Ok(StreamEventName::Remove),
        other => Err(DynamoDbError::InternalServerError(format!(
            "corrupt stream event name: {other}"
        ))),
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

/// An owned table-scoped stream change that can be folded into the same
/// `AtomicWriteBatch` as the data mutation that produced it.
#[derive(Clone)]
pub(crate) struct StreamChange {
    pub(crate) table_name: String,
    pub(crate) event_name: StreamEventName,
    pub(crate) keys: Item,
    pub(crate) old_image: Option<Item>,
    pub(crate) new_image: Option<Item>,
    pub(crate) user_identity: Option<UserIdentity>,
}

impl StreamChange {
    pub(crate) fn new(
        table_name: impl Into<String>,
        event_name: StreamEventName,
        keys: Item,
        old_image: Option<Item>,
        new_image: Option<Item>,
        user_identity: Option<UserIdentity>,
    ) -> Self {
        Self {
            table_name: table_name.into(),
            event_name,
            keys,
            old_image,
            new_image,
            user_identity,
        }
    }

    fn event(&self) -> ChangeEvent<'_> {
        ChangeEvent {
            event_name: self.event_name,
            keys: &self.keys,
            old_image: self.old_image.as_ref(),
            new_image: self.new_image.as_ref(),
            user_identity: self.user_identity.clone(),
        }
    }
}

/// Bound on optimistic retries when a concurrent writer claims the same stream
/// sequence number before us. Writers contend only on the single per-table
/// counter doc, so in practice one retry suffices; the bound guards against a
/// pathological live-lock.
const MAX_SEQUENCE_RETRIES: usize = 32;

/// Serialize a [`StoredEvent`] at sequence `seq` to its stored field map.
fn event_fields(change: &ChangeEvent<'_>, seq: i64) -> Result<Map<String, Value>, DynamoDbError> {
    let event = StoredEvent {
        seq,
        created: epoch_seconds(),
        event_name: event_name_str(change.event_name).to_owned(),
        keys: item_to_fields(change.keys)?,
        old_image: change.old_image.map(item_to_fields).transpose()?,
        new_image: change.new_image.map(item_to_fields).transpose()?,
        user_identity: change.user_identity.clone(),
    };
    match serde_json::to_value(&event) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(DynamoDbError::InternalServerError(
            "failed to serialize stream event".to_owned(),
        )),
    }
}

/// The atomic `Create` write for a stream event at `seq` — the event-store
/// document keyed by its zero-padded sequence number. `Create` (not `Overwrite`)
/// makes a colliding sequence from a concurrent writer fail the commit rather
/// than silently clobber its event.
pub(crate) fn stream_event_write(
    table_name: &str,
    seq: i64,
    change: &ChangeEvent<'_>,
) -> Result<AtomicWrite, DynamoDbError> {
    let event_id = DocumentId::from_key(sequence_number(seq)).map_err(map_core_error)?;
    Ok(AtomicWrite::Set {
        key: WriteKey::from(DocumentLocator::new(
            stream_events_table(table_name)?,
            event_id,
        )),
        document: event_fields(change, seq)?,
        typed_fields: Default::default(),
        mode: WriteSetMode::Create,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    })
}

/// The atomic `Overwrite` write advancing a stream's high-water counter to
/// `next`. Kept in a separate store so reclaiming expired events never resets it.
pub(crate) fn sequence_counter_write(
    table_name: &str,
    next: i64,
) -> Result<AtomicWrite, DynamoDbError> {
    let mut fields = Map::new();
    fields.insert("next".to_owned(), Value::from(next));
    Ok(AtomicWrite::Set {
        key: WriteKey::from(DocumentLocator::new(
            stream_seq_table(table_name)?,
            seq_counter_id()?,
        )),
        document: fields,
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    })
}

/// Append stream-event writes for every stream-enabled table touched by a data
/// mutation, so callers can commit the data write and stream effects in one
/// storage transaction. Sequence numbers are assigned from each table's
/// high-water counter; a colliding event `Create` write fails the whole batch,
/// allowing the caller to retry with a freshly read counter.
pub(crate) fn append_stream_writes(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    writes: &mut Vec<AtomicWrite>,
    changes: &[StreamChange],
) -> Result<(), DynamoDbError> {
    let mut tables: Vec<&str> = Vec::new();
    for change in changes {
        if !tables.contains(&change.table_name.as_str()) {
            tables.push(&change.table_name);
        }
    }
    for table_name in tables {
        if !stream_enabled(engine, context, table_name)? {
            continue;
        }
        let mut seq = next_sequence_value(engine, context, table_name)?;
        for change in changes
            .iter()
            .filter(|change| change.table_name == table_name)
        {
            writes.push(stream_event_write(table_name, seq, &change.event())?);
            seq += 1;
        }
        writes.push(sequence_counter_write(table_name, seq)?);
    }
    Ok(())
}

/// Execute data writes and their stream effects in a single atomic batch. If a
/// concurrent writer claims the planned stream sequence first, the entire batch
/// rolls back and this helper retries with the next observed sequence. Base
/// write precondition failures are re-checked against a fresh snapshot so they
/// are not mistaken for stream sequence contention.
pub(crate) fn execute_atomic_write_batch_with_streams<F>(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    base_writes: Vec<AtomicWrite>,
    changes: &[StreamChange],
    map_write_error: F,
) -> Result<(), DynamoDbError>
where
    F: Fn(nimbus_core::Error) -> DynamoDbError,
{
    for _ in 0..MAX_SEQUENCE_RETRIES {
        let mut writes = base_writes.clone();
        let base_write_count = writes.len();
        append_stream_writes(engine, context, &mut writes, changes)?;
        let has_stream_writes = writes.len() > base_write_count;
        if writes.is_empty() {
            return Ok(());
        }
        let batch = AtomicWriteBatch::new(writes).map_err(map_core_error)?;
        match engine
            .begin_mutation_execution_unit(context.tenant_id().clone(), caller_principal(context))
            .map_err(map_core_error)?
            .execute_atomic_write_batch(batch)
        {
            Ok(_) => return Ok(()),
            Err(nimbus_core::Error::AlreadyExists(_)) if has_stream_writes => {
                match stage_base_writes(engine, context, base_writes.clone()) {
                    Ok(()) => continue,
                    Err(error) => return Err(map_write_error(error)),
                }
            }
            Err(error) => return Err(map_write_error(error)),
        }
    }

    Err(DynamoDbError::InternalServerError(
        "stream sequence allocation exhausted retries under contention".to_owned(),
    ))
}

fn stage_base_writes(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    base_writes: Vec<AtomicWrite>,
) -> Result<(), nimbus_core::Error> {
    if base_writes.is_empty() {
        return Ok(());
    }
    let batch = AtomicWriteBatch::new(base_writes)?;
    engine
        .begin_mutation_execution_unit(context.tenant_id().clone(), caller_principal(context))?
        .stage_atomic_write_batch(batch)?;
    Ok(())
}

/// Whether `table_name` has a stream enabled (the gate for capturing events).
pub(crate) fn stream_enabled(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<bool, DynamoDbError> {
    let description = control_plane::load_table_description(engine, context, table_name)?;
    Ok(description
        .stream_specification
        .as_ref()
        .is_some_and(|spec| spec.stream_enabled))
}

/// Read captured events for a table's stream from `start_sequence`, ascending
/// by sequence and capped to one GetRecords page.
fn read_events_from(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
    start_sequence: i64,
    limit: usize,
) -> Result<Vec<StoredEvent>, DynamoDbError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let table = stream_events_table(table_name)?;
    let start_id = sequence_number(start_sequence);
    let documents = match engine.scan_documents_by_id_starting_at_cancellable(
        context.tenant_id(),
        &table,
        &start_id,
        limit,
        &mut || Ok(()),
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

#[cfg(test)]
fn read_events(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<Vec<StoredEvent>, DynamoDbError> {
    read_events_from(engine, context, table_name, 0, usize::MAX)
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: ListStreamsInput,
) -> Result<ListStreamsOutput, DynamoDbError> {
    let mut summaries: Vec<StreamSummary> =
        control_plane::list_table_descriptions(engine, context)?
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
    events: &[StoredEvent],
    cutoff: i64,
) -> Result<usize, DynamoDbError> {
    let table = stream_events_table(table_name)?;
    let writes = events
        .iter()
        .filter(|event| event.created < cutoff)
        .map(|event| {
            let id = DocumentId::from_key(sequence_number(event.seq)).map_err(map_core_error)?;
            Ok(item::delete_atomic_write(
                table.clone(),
                id,
                WritePrecondition::default(),
            ))
        })
        .collect::<Result<Vec<_>, DynamoDbError>>()?;
    let reclaimed = writes.len();
    if writes.is_empty() {
        return Ok(0);
    }
    let batch = AtomicWriteBatch::new(writes).map_err(map_core_error)?;
    engine
        .begin_mutation_execution_unit(context.tenant_id().clone(), adapter_principal())
        .map_err(map_core_error)?
        .execute_atomic_write_batch(batch)
        .map_err(map_core_error)?;
    Ok(reclaimed)
}

/// GetRecords: return the events at/after the iterator's position (≤1000),
/// shaped per the table's StreamViewType, plus an advanced NextShardIterator.
///
/// # Errors
/// `ValidationException` for a malformed iterator; `ResourceNotFoundException`
/// if the stream no longer exists.
pub fn get_records(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: GetRecordsInput,
) -> Result<GetRecordsOutput, DynamoDbError> {
    let iterator = decode_iterator(&input.shard_iterator)?;
    let description = resolve_stream_table(engine, context, &iterator.stream_arn)?;
    let view_type = description
        .stream_specification
        .as_ref()
        .and_then(|spec| spec.stream_view_type)
        .unwrap_or(StreamViewType::NewAndOldImages);

    let limit = input
        .limit
        .filter(|limit| *limit > 0)
        .map(|limit| (limit as usize).min(MAX_GET_RECORDS))
        .unwrap_or(MAX_GET_RECORDS);
    let cutoff = epoch_seconds() - STREAM_RETENTION_SECS;
    let window = read_events_from(
        engine,
        context,
        &description.table_name,
        iterator.next_sequence,
        limit,
    )?;

    // Expired events stay in the fetched window so the iterator still advances
    // past them — a re-poll never stalls on records the retention window has
    // dropped.
    let next_sequence = window
        .last()
        .map_or(iterator.next_sequence, |event| event.seq + 1);

    // The returned batch excludes anything past the 24h retention window.
    let records = window
        .iter()
        .filter(|event| event.created >= cutoff)
        .map(|event| shape_record(event, view_type))
        .collect::<Result<Vec<_>, _>>()?;

    // Reclaim expired event storage from the fetched page (the sequence counter
    // is a separate store, so the high-water mark is unaffected).
    reclaim_expired_events(engine, context, &description.table_name, &window, cutoff)?;

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
        event_name: event_name_from_str(&event.event_name)?,
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    stream_arn: &str,
) -> Result<TableDescription, DynamoDbError> {
    let table_name = table_name_from_stream_arn(stream_arn)?;
    let description = control_plane::load_table_description(engine, context, table_name)?;
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: DescribeStreamInput,
) -> Result<DescribeStreamOutput, DynamoDbError> {
    let description = resolve_stream_table(engine, context, &input.stream_arn)?;
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: GetShardIteratorInput,
) -> Result<GetShardIteratorOutput, DynamoDbError> {
    let description = resolve_stream_table(engine, context, &input.stream_arn)?;
    let expected_shard = shard_id(&description.table_id);
    if input.shard_id != expected_shard {
        return Err(DynamoDbError::ResourceNotFoundException(format!(
            "Requested resource not found: Shard: {} not found",
            input.shard_id
        )));
    }
    let next_sequence = match input.shard_iterator_type {
        ShardIteratorType::TrimHorizon => 0,
        ShardIteratorType::Latest => next_sequence_value(engine, context, &description.table_name)?,
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
mod tests;
