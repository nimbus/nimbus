//! What the event store forgets.
//!
//! Records outside the 24h retention window are skipped and their storage
//! reclaimed. Reclamation must not disturb the sequence high-water mark, which
//! is why the counter lives in its own store.

use super::*;

/// Persist a stream event directly with a chosen sequence/timestamp, so
/// retention can be tested without waiting out the 24h window.
fn inject_event(
    engine: &Arc<Engine>,
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
        old_image_times: None,
        new_image: Some(
            json!({ "pk": { "S": format!("k{seq}") }, "v": { "N": "1" } })
                .as_object()
                .unwrap()
                .clone(),
        ),
        new_image_retained_update: None,
        user_identity: None,
        committed_at: 0,
    };
    let Value::Object(fields) = serde_json::to_value(&event).unwrap() else {
        panic!("event serializes to an object");
    };
    let id = DocumentId::from_key(sequence_number(seq)).unwrap();
    engine
        .insert_document_with_id(
            context.tenant_id(),
            stream_events_table(table_name).unwrap(),
            id,
            fields,
        )
        .expect("inject event");
}

#[test]
fn get_records_skips_expired_events_and_reclaims_their_storage() {
    let (engine, ctx, _t) = fixture();
    let arn = create_streamed_named(&engine, &ctx, "events");
    let now = epoch_seconds();
    inject_event(
        &engine,
        &ctx,
        "events",
        0,
        now - STREAM_RETENTION_SECS - 100,
    ); // expired
    inject_event(&engine, &ctx, "events", 1, now); // fresh
    assert_eq!(read_events(&engine, &ctx, "events").unwrap().len(), 2);

    let out = all_records(&engine, &ctx, &arn);
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
        read_events(&engine, &ctx, "events").unwrap().len(),
        1,
        "the expired event's storage is reclaimed on poll"
    );
}

#[test]
fn reclaiming_expired_events_preserves_the_monotonic_sequence() {
    let (engine, ctx, _t) = fixture();
    let arn = create_streamed_named(&engine, &ctx, "events");
    let now = epoch_seconds();
    // Two events that have both aged out of the retention window, with the
    // sequence counter advanced past them (as real capture would leave it).
    inject_event(
        &engine,
        &ctx,
        "events",
        0,
        now - STREAM_RETENTION_SECS - 100,
    );
    inject_event(
        &engine,
        &ctx,
        "events",
        1,
        now - STREAM_RETENTION_SECS - 100,
    );
    set_sequence_value(&engine, &ctx, "events", 2).expect("counter");

    // A poll returns nothing (all expired) and reclaims both event docs.
    let out = all_records(&engine, &ctx, &arn);
    assert!(out.records.is_empty(), "all events expired");
    assert_eq!(
        read_events(&engine, &ctx, "events").unwrap().len(),
        0,
        "expired storage reclaimed"
    );

    // The high-water mark is preserved: the next captured event keeps
    // climbing rather than colliding with a consumer's advanced iterator.
    assert_eq!(
        next_sequence_value(&engine, &ctx, "events").unwrap(),
        2,
        "reclamation does not reset the counter"
    );
    put(&engine, &ctx, "z", "9");
    let fresh = read_events(&engine, &ctx, "events").unwrap();
    assert_eq!(fresh.len(), 1);
    assert_eq!(
        fresh[0].seq, 2,
        "the new event continues past the reclaimed sequences"
    );
}
