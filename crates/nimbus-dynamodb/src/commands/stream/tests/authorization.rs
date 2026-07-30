//! Who may read a stream record, and against what.
//!
//! A record is item-level information, so GetRecords answers the source
//! table's read rule before returning one. That pulls in two things the rest of
//! the stream tests do not touch: the lifecycle timestamps images are
//! reconstructed with, since a rule may name `_creationTime`/`_updateTime`, and
//! the budget that bounds how far a fill walks past records it must withhold.

use super::*;

/// The work one GetRecords call performs is bounded by what the caller asked
/// for, not by the maximum page size.
///
/// Filling a page walks past records the caller may not read, so a dense run of
/// withheld events is an amplification lever: without a limit-relative budget a
/// caller could poll for a single record and make the server scan a full page's
/// worth of events, every poll. The budget caps one call at
/// `EVENT_EXAMINATION_AMPLIFICATION` events examined per record requested, and
/// the returned iterator advances over exactly those — a short page means
/// "poll again", so the stream still drains.
#[test]
fn a_small_page_request_is_bounded_when_records_are_withheld() {
    let (engine, ctx, _t) = fixture();
    let arn = streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
    let written = 40;
    for index in 0..written {
        put(&engine, &ctx, &format!("k{index}"), "1");
    }
    withhold_events_reads(&engine, &ctx);

    let shard = shard_for(&engine, &ctx, &arn);
    let iterator = get_shard_iterator(
        &engine,
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
    let start = iterator_next_sequence(&iterator);

    let _ = take_store_reads();
    let page = get_records(
        &engine,
        &ctx,
        GetRecordsInput {
            shard_iterator: iterator,
            limit: Some(1),
        },
    )
    .expect("page");
    let reads = take_store_reads();
    assert!(
        page.records.is_empty(),
        "the policy withholds every record, so the page is empty"
    );

    let advanced = iterator_next_sequence(&page.next_shard_iterator.expect("next"));
    let budget = i64::try_from(EVENT_EXAMINATION_AMPLIFICATION).expect("budget fits in i64");
    assert_eq!(
        advanced - start,
        budget,
        "a one-record poll must examine {budget} stored events, not walk the whole run of \
         {written} withheld ones"
    );
    assert!(
        reads <= EVENT_EXAMINATION_AMPLIFICATION,
        "spending the budget took {reads} store reads, over the ceiling of \
         {EVENT_EXAMINATION_AMPLIFICATION}"
    );
}

/// The store-read ceiling holds for the distribution that most tempts a
/// slot-sized read: a first window that fills the page to one slot short,
/// followed by a run the caller may not read.
///
/// Sizing each read by the slots left over-fits exactly here. The page needs
/// one more record, so every refill asks the store for one event and the
/// examination budget drains a scan at a time — for a full 1000-record page
/// that is ~3,001 backend scans for a single request, repeatable by replaying
/// the iterator. Reading budget-sized chunks keeps it at
/// `EVENT_EXAMINATION_AMPLIFICATION` reads no matter how the caller arranges
/// the records.
#[test]
fn the_store_read_ceiling_holds_when_a_page_stalls_one_slot_short() {
    let (engine, ctx, _t) = fixture();
    let arn = streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
    let limit = 10usize;
    // One short of a full page, so the fill cannot finish inside the first
    // window and must keep refilling with a single slot to place.
    let readable = limit - 1;
    for index in 0..readable {
        put_tagged(&engine, &ctx, &format!("r{index}"), "read");
    }
    // Enough withheld events to exhaust the examination budget without the
    // store running dry, which would end the fill early for the wrong reason.
    let withheld = limit * EVENT_EXAMINATION_AMPLIFICATION;
    for index in 0..withheld {
        put_tagged(&engine, &ctx, &format!("w{index}"), "hide");
    }
    admit_only_readable_events(&engine, &ctx);

    let shard = shard_for(&engine, &ctx, &arn);
    let iterator = get_shard_iterator(
        &engine,
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
    let start = iterator_next_sequence(&iterator);

    let _ = take_store_reads();
    let page = get_records(
        &engine,
        &ctx,
        GetRecordsInput {
            shard_iterator: iterator,
            limit: Some(i64::try_from(limit).expect("limit fits")),
        },
    )
    .expect("page");
    let reads = take_store_reads();

    assert_eq!(
        reads, EVENT_EXAMINATION_AMPLIFICATION,
        "one call must spend its examination budget in {EVENT_EXAMINATION_AMPLIFICATION} store \
         reads; {reads} means the reads were sized by the page slots left, not by the budget"
    );
    assert_eq!(
        page.records.len(),
        readable,
        "the authorized prefix is returned as a short page"
    );
    let advanced = iterator_next_sequence(&page.next_shard_iterator.expect("next"));
    assert_eq!(
        advanced - start,
        i64::try_from(limit * EVENT_EXAMINATION_AMPLIFICATION).expect("budget fits in i64"),
        "the iterator advances over every event the fill consumed, so the next poll resumes \
         past them rather than re-walking the withheld run"
    );
}

/// Resolve record authorization for the `events` table as the test caller.
fn record_authorization(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
) -> RecordAuthorization {
    let table = TableName::new("events").unwrap();
    let filter = engine
        .document_read_filter(context.tenant_id(), &table, &caller_principal(context))
        .expect("read filter should resolve");
    RecordAuthorization {
        filter,
        table,
        key_schema: control_plane::load_key_schema(engine, context, "events").unwrap(),
    }
}

/// Put a read policy on `events` comparing `field` against `value` with `op`.
///
/// Every caller wants a rule that is restricted without being trivially
/// unsatisfiable, so authorization neither short-circuits as unrestricted nor
/// as impossible and every verdict comes from evaluating an actual image.
fn set_events_read_rule(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    field: &str,
    op: nimbus_core::AccessOperator,
    value: Value,
) {
    engine
        .set_table_schema(
            context.tenant_id(),
            nimbus_core::TableSchema {
                table: TableName::new("events").unwrap(),
                fields: Vec::new(),
                indexes: Vec::new(),
                access_policy: Some(nimbus_core::TableAccessPolicy {
                    read: nimbus_core::AccessRule {
                        require_authenticated: false,
                        predicates: vec![nimbus_core::AccessPredicate {
                            left: nimbus_core::AccessValue::DocumentField {
                                field: field.to_owned(),
                            },
                            op,
                            right: nimbus_core::AccessValue::Literal { value },
                        }],
                    },
                    ..nimbus_core::TableAccessPolicy::default()
                }),
            },
        )
        .expect("policy should be storable");
}

/// A read policy on `events` that every real item satisfies.
fn restrict_events_reads(engine: &Arc<Engine>, context: &TenantIsolationContext) {
    set_events_read_rule(
        engine,
        context,
        "pk",
        nimbus_core::AccessOperator::Neq,
        Value::Null,
    );
}

/// A read policy on `events` that no item written by these tests satisfies,
/// while still being a real predicate the filter has to evaluate per document.
fn withhold_events_reads(engine: &Arc<Engine>, context: &TenantIsolationContext) {
    set_events_read_rule(
        engine,
        context,
        "pk",
        nimbus_core::AccessOperator::Eq,
        Value::String("no-such-item".to_owned()),
    );
}

/// A read policy on `events` that admits exactly the items [`put_tagged`] wrote
/// with `tag = "read"`, so one stream can carry both authorized and withheld
/// events in an order the test chooses.
///
/// The literal is AttributeValue wire JSON because that is how the adapter
/// persists attributes — see `item_to_fields`.
fn admit_only_readable_events(engine: &Arc<Engine>, context: &TenantIsolationContext) {
    set_events_read_rule(
        engine,
        context,
        "tag",
        nimbus_core::AccessOperator::Eq,
        json!({"S": "read"}),
    );
}

/// Write an item carrying a `tag` attribute [`admit_only_readable_events`] can
/// discriminate on.
fn put_tagged(engine: &Arc<Engine>, context: &TenantIsolationContext, pk: &str, tag: &str) {
    crate::commands::item::put_item(
        engine,
        context,
        serde_json::from_value(json!({
            "TableName": "events",
            "Item": { "pk": {"S": pk}, "tag": {"S": tag} },
        }))
        .unwrap(),
    )
    .expect("put");
}

/// The item `pk` as the engine currently holds it, lifecycle stamps included.
fn stored_document(engine: &Arc<Engine>, ctx: &TenantIsolationContext, pk: &str) -> Document {
    let key: Item = [("pk".to_string(), AttributeValue::S(pk.to_owned()))]
        .into_iter()
        .collect();
    let key_schema = control_plane::load_key_schema(engine, ctx, "events").unwrap();
    let id = crate::commands::item::primary_key_id(&key, &key_schema).unwrap();
    engine
        .get_document(ctx.tenant_id(), &TableName::new("events").unwrap(), id)
        .expect("the item is stored")
}

/// Wall-clock milliseconds since the epoch, the unit the engine stamps commits
/// in.
fn now_millis() -> u64 {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the system clock is after the unix epoch");
    u64::try_from(since_epoch.as_millis()).expect("milliseconds since the epoch fit in u64")
}

/// Block until the commit clock has left `stamp` behind, so the next write is
/// stamped strictly later.
///
/// Commit timestamps are wall-clock milliseconds, so two writes inside one
/// millisecond share a stamp and `_updateTime` stays equal to `_creationTime`
/// — which would make the lifecycle behaviour under test unobservable. Waiting
/// on the observable quantity rather than sleeping a fixed span stays correct
/// on a coarse clock or a loaded machine, where a fixed span is a guess; the
/// deadline turns a clock that never advances into a loud failure instead of a
/// hang. The engine takes `max(now, previous)` for a commit stamp, so a wall
/// clock past `stamp` puts every later commit past it too.
fn wait_for_commit_clock_past(stamp: Timestamp) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while now_millis() <= stamp.0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the commit clock did not advance past {} within 5s, so a later write cannot be \
             told apart from the one before it",
            stamp.0
        );
        std::thread::yield_now();
    }
}

/// The images an event carries are rebuilt with the lifecycle times the engine
/// itself assigned — the old image's captured with it, the new image's from the
/// commit that produced the event.
///
/// This pins the load-bearing claim behind that reconstruction: the event
/// document is created in the same `AtomicWriteBatch` as the data write, so its
/// own creation stamp *is* that mutation's commit timestamp, which is the new
/// image's `_updateTime`.
#[test]
fn reconstructed_image_times_match_the_engine_document_times() {
    let (engine, ctx, _temp) = fixture();
    streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
    put(&engine, &ctx, "a", "1");
    wait_for_commit_clock_past(stored_document(&engine, &ctx, "a").creation_time);
    put(&engine, &ctx, "a", "2");

    let stored = stored_document(&engine, &ctx, "a");
    assert_ne!(
        stored.creation_time, stored.update_time,
        "the second write must land in a later millisecond, or this test cannot tell a \
         reconstructed update time from a reconstructed creation time"
    );

    let authorization = record_authorization(&engine, &ctx);
    let events = read_events(&engine, &ctx, "events").unwrap();
    assert_eq!(events.len(), 2, "one INSERT and one MODIFY");

    let inserted = authorization.documents_for(&events[0]).unwrap();
    assert_eq!(inserted.len(), 1, "an INSERT carries only its new image");
    assert_eq!(
        (inserted[0].creation_time, inserted[0].update_time),
        (stored.creation_time, stored.creation_time),
        "a created document's creation and update times are both the commit that created it"
    );

    let modified = authorization.documents_for(&events[1]).unwrap();
    assert_eq!(modified.len(), 2, "a MODIFY carries both images");
    assert_eq!(
        (modified[0].creation_time, modified[0].update_time),
        (stored.creation_time, stored.creation_time),
        "the old image is the document as it stood before the update"
    );
    assert_eq!(
        (modified[1].creation_time, modified[1].update_time),
        (stored.creation_time, stored.update_time),
        "the new image keeps the original creation time and takes the commit timestamp as its \
         update time — exactly what the engine stamped on the stored document"
    );
}

/// A PutItem that rewrites identical content emits a MODIFY record, but the
/// engine leaves `_updateTime` where it was — so the reconstructed new image
/// must keep the retained stamp rather than take the commit timestamp.
///
/// Taking the commit timestamp here would make the reconstructed document
/// disagree with what a read of the table returns, and a rule over lifecycle
/// times would then reach a different verdict for the record than for the item
/// it describes.
#[test]
fn a_no_op_write_reconstructs_the_retained_update_time() {
    let (engine, ctx, _temp) = fixture();
    streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
    put(&engine, &ctx, "a", "1");
    let created = stored_document(&engine, &ctx, "a").creation_time;
    // Without the wait the rewrite could land in the creating millisecond and
    // the stamps would match for the uninteresting reason.
    wait_for_commit_clock_past(created);
    put(&engine, &ctx, "a", "1");

    let stored = stored_document(&engine, &ctx, "a");
    assert_eq!(
        (stored.creation_time, stored.update_time),
        (created, created),
        "rewriting identical content is a lifecycle no-op: the engine keeps the update time"
    );

    let authorization = record_authorization(&engine, &ctx);
    let events = read_events(&engine, &ctx, "events").unwrap();
    assert_eq!(
        events.len(),
        2,
        "the no-op rewrite still emits a MODIFY record"
    );
    assert_eq!(
        events[1].event_name, "MODIFY",
        "the second event is the rewrite"
    );

    let modified = authorization.documents_for(&events[1]).unwrap();
    assert_eq!(modified.len(), 2, "a MODIFY carries both images");
    for (index, image) in modified.iter().enumerate() {
        assert_eq!(
            (image.creation_time, image.update_time),
            (stored.creation_time, stored.update_time),
            "image {index} of a no-op rewrite must match the stored document, which the commit \
             did not restamp"
        );
    }
}

/// An event carrying neither image is unreadable rather than public.
///
/// Authorization is a statement about the images an event discloses. One with
/// no image to evaluate cannot be shown to satisfy the rule, and a record is
/// still item-level information even when its images are absent.
#[test]
fn a_record_carrying_neither_image_is_withheld() {
    let (engine, ctx, _temp) = fixture();
    streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
    put(&engine, &ctx, "a", "1");
    restrict_events_reads(&engine, &ctx);

    let authorization = record_authorization(&engine, &ctx);
    let real = read_events(&engine, &ctx, "events").unwrap();
    assert!(
        authorization.allows(&real[0]).unwrap(),
        "the policy admits a real item, so a withheld verdict below is about the missing \
         images and not about the policy denying everything"
    );

    let imageless = StoredEvent {
        seq: 99,
        created: 0,
        event_name: "MODIFY".to_owned(),
        keys: json!({ "pk": { "S": "a" } }).as_object().unwrap().clone(),
        old_image: None,
        old_image_times: None,
        new_image: None,
        new_image_retained_update: None,
        user_identity: None,
        committed_at: 0,
    };
    assert!(
        !authorization.allows(&imageless).unwrap(),
        "an event with no image to authorize must be withheld"
    );
}

/// A stored event carrying an old image but no lifecycle times for it is
/// corrupt, not old: the format writes them unconditionally. Authorizing
/// against absent metadata would silently compare a rule to placeholders, so
/// the read fails instead.
#[test]
fn an_old_image_without_lifecycle_times_is_rejected() {
    let (engine, ctx, _temp) = fixture();
    streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
    put(&engine, &ctx, "a", "1");
    restrict_events_reads(&engine, &ctx);

    let authorization = record_authorization(&engine, &ctx);
    let corrupt = StoredEvent {
        seq: 99,
        created: 0,
        event_name: "MODIFY".to_owned(),
        keys: json!({ "pk": { "S": "a" } }).as_object().unwrap().clone(),
        old_image: Some(
            json!({ "pk": { "S": "a" }, "v": { "N": "1" } })
                .as_object()
                .unwrap()
                .clone(),
        ),
        old_image_times: None,
        new_image: None,
        new_image_retained_update: None,
        user_identity: None,
        committed_at: 0,
    };

    match authorization.allows(&corrupt) {
        Err(DynamoDbError::InternalServerError(message)) => assert!(
            message.contains("old image without lifecycle times"),
            "unexpected error: {message}"
        ),
        other => panic!("expected a corrupt-event error, got {other:?}"),
    }
}
