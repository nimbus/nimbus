use std::collections::HashMap;
use std::sync::Arc;

use neovex_core::{
    PrincipalContext, Query, SubscriptionDocumentChangeKind, SubscriptionResultSnapshot, TableName,
    TenantId,
};
use neovex_engine::{Service, SubscriptionCleanupHandle, SubscriptionUpdate};
use tokio::sync::mpsc;

use super::super::bson_bridge;
use super::super::error::MongoError;
use super::cursor::next_cursor_id;

const DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY: usize = 64;

pub struct ChangeStreamCursor {
    pub ns: String,
    pub receiver: mpsc::Receiver<SubscriptionUpdate>,
    pub subscription_id: u64,
    pub tenant_id: TenantId,
    pub last_snapshot: Option<SubscriptionResultSnapshot>,
    pub resume_after: Option<ResumeToken>,
    _cleanup: SubscriptionCleanupHandle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResumeToken {
    pub time: u32,
    pub increment: u32,
    pub document_id: String,
}

impl ResumeToken {
    pub fn parse(data: &str) -> Option<Self> {
        let mut parts = data.splitn(3, '_');
        let time = parts.next()?.parse::<u32>().ok()?;
        let increment = parts.next()?.parse::<u32>().ok()?;
        let document_id = parts.next()?.to_string();
        Some(Self {
            time,
            increment,
            document_id,
        })
    }

    pub fn to_cluster_time(&self) -> bson::Timestamp {
        bson::Timestamp {
            time: self.time,
            increment: self.increment,
        }
    }
}

pub fn parse_resume_token_from_doc(doc: &bson::Document) -> Option<ResumeToken> {
    let data = doc.get_str("_data").ok()?;
    ResumeToken::parse(data)
}

pub fn extract_resume_option(change_stream_doc: &bson::Document) -> Option<ResumeToken> {
    if let Ok(doc) = change_stream_doc.get_document("resumeAfter") {
        return parse_resume_token_from_doc(doc);
    }
    if let Ok(doc) = change_stream_doc.get_document("startAfter") {
        return parse_resume_token_from_doc(doc);
    }
    None
}

#[derive(Default)]
pub struct ChangeStreamStore {
    cursors: HashMap<i64, ChangeStreamCursor>,
}

impl ChangeStreamStore {
    pub fn insert(&mut self, cursor_id: i64, cursor: ChangeStreamCursor) {
        self.cursors.insert(cursor_id, cursor);
    }

    pub fn get_mut(&mut self, cursor_id: i64) -> Option<&mut ChangeStreamCursor> {
        self.cursors.get_mut(&cursor_id)
    }

    pub fn remove(&mut self, cursor_id: i64) -> bool {
        self.cursors.remove(&cursor_id).is_some()
    }

    pub fn contains(&self, cursor_id: i64) -> bool {
        self.cursors.contains_key(&cursor_id)
    }

    pub fn kill_all(&mut self) {
        self.cursors.clear();
    }
}

pub fn open_change_stream(
    collection: &str,
    db_name: &str,
    service: &Arc<Service>,
    resume_after: Option<ResumeToken>,
) -> Result<(i64, ChangeStreamCursor), MongoError> {
    let tenant_id = TenantId::new(db_name).map_err(MongoError::from)?;
    let table = TableName::new(collection).map_err(MongoError::from)?;

    let query = Query {
        table: table.clone(),
        filters: vec![],
        order: None,
        limit: None,
    };

    let (sender, receiver) = mpsc::channel(DEFAULT_SUBSCRIPTION_CHANNEL_CAPACITY);
    let request_id = format!("change_stream_{}_{}", db_name, collection);

    let registration = service
        .subscribe_with_principal(
            &tenant_id,
            query,
            &PrincipalContext::system(),
            request_id,
            sender,
        )
        .map_err(MongoError::from)?;

    let cursor_id = next_cursor_id();
    let ns = format!("{}.{}", db_name, collection);

    let (sub_id, cleanup) = registration.into_parts();

    let cursor = ChangeStreamCursor {
        ns,
        receiver,
        subscription_id: sub_id,
        tenant_id,
        last_snapshot: None,
        resume_after,
        _cleanup: cleanup,
    };

    Ok((cursor_id, cursor))
}

pub fn collect_change_events(cursor: &mut ChangeStreamCursor) -> Vec<bson::Document> {
    let mut events = Vec::new();

    while let Ok(update) = cursor.receiver.try_recv() {
        match update {
            SubscriptionUpdate::Result { snapshot, .. } => {
                let new_events =
                    snapshot_to_change_events(&cursor.ns, cursor.last_snapshot.as_ref(), &snapshot);
                events.extend(new_events);
                cursor.last_snapshot = Some(snapshot);
            }
            SubscriptionUpdate::Error { .. } => {}
        }
    }

    if let Some(ref token) = cursor.resume_after {
        events = filter_events_after_resume(events, token);
        if events.is_empty() {
            return events;
        }
        cursor.resume_after = None;
    }

    events
}

pub fn filter_events_after_resume(
    events: Vec<bson::Document>,
    token: &ResumeToken,
) -> Vec<bson::Document> {
    let resume_time = token.to_cluster_time();
    events
        .into_iter()
        .filter(|event| {
            let Some(ct) = event.get_timestamp("clusterTime").ok() else {
                return true;
            };
            if ct.time > resume_time.time {
                return true;
            }
            if ct.time == resume_time.time && ct.increment > resume_time.increment {
                return true;
            }
            false
        })
        .collect()
}

pub fn invalidate_event(ns: &str) -> bson::Document {
    let parts: Vec<&str> = ns.splitn(2, '.').collect();
    let (db, coll) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        ("", ns)
    };

    bson::doc! {
        "_id": { "_data": "invalidate" },
        "operationType": "invalidate",
        "ns": { "db": db, "coll": coll },
        "clusterTime": bson::Timestamp { time: 0, increment: 0 },
    }
}

pub fn snapshot_to_change_events_pub(
    ns: &str,
    previous: Option<&SubscriptionResultSnapshot>,
    current: &SubscriptionResultSnapshot,
) -> Vec<bson::Document> {
    snapshot_to_change_events(ns, previous, current)
}

fn snapshot_to_change_events(
    ns: &str,
    previous: Option<&SubscriptionResultSnapshot>,
    current: &SubscriptionResultSnapshot,
) -> Vec<bson::Document> {
    let diff = neovex_core::diff_subscription_snapshots(previous, current);
    let mut events = Vec::new();

    let parts: Vec<&str> = ns.splitn(2, '.').collect();
    let (db, coll) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        ("", ns)
    };

    let cluster_time = current
        .commit
        .map(|c| bson::Timestamp {
            time: (c.timestamp.0 / 1000) as u32,
            increment: c.sequence.0 as u32,
        })
        .unwrap_or(bson::Timestamp {
            time: 0,
            increment: 0,
        });

    for change in &diff.changes {
        let event = match change.kind {
            SubscriptionDocumentChangeKind::Added => {
                let doc = change.current.as_ref().unwrap();
                let bson_doc = bson_bridge::document_to_bson_doc(doc);
                let document_key =
                    bson::doc! { "_id": bson_doc.get("_id").cloned().unwrap_or(bson::Bson::Null) };
                let resume_token = make_resume_token(&cluster_time, &document_key);

                bson::doc! {
                    "_id": resume_token,
                    "operationType": "insert",
                    "ns": { "db": db, "coll": coll },
                    "documentKey": document_key,
                    "fullDocument": bson_doc,
                    "clusterTime": cluster_time,
                }
            }
            SubscriptionDocumentChangeKind::Modified => {
                let current_doc = change.current.as_ref().unwrap();
                let bson_doc = bson_bridge::document_to_bson_doc(current_doc);
                let document_key =
                    bson::doc! { "_id": bson_doc.get("_id").cloned().unwrap_or(bson::Bson::Null) };
                let resume_token = make_resume_token(&cluster_time, &document_key);

                let update_desc = compute_update_description(
                    change
                        .previous
                        .as_ref()
                        .map(bson_bridge::document_to_bson_doc),
                    &bson_doc,
                );

                bson::doc! {
                    "_id": resume_token,
                    "operationType": "update",
                    "ns": { "db": db, "coll": coll },
                    "documentKey": document_key,
                    "fullDocument": bson_doc,
                    "updateDescription": update_desc,
                    "clusterTime": cluster_time,
                }
            }
            SubscriptionDocumentChangeKind::Removed => {
                let doc = change.previous.as_ref().unwrap();
                let bson_doc = bson_bridge::document_to_bson_doc(doc);
                let document_key =
                    bson::doc! { "_id": bson_doc.get("_id").cloned().unwrap_or(bson::Bson::Null) };
                let resume_token = make_resume_token(&cluster_time, &document_key);

                bson::doc! {
                    "_id": resume_token,
                    "operationType": "delete",
                    "ns": { "db": db, "coll": coll },
                    "documentKey": document_key,
                    "clusterTime": cluster_time,
                }
            }
        };
        events.push(event);
    }

    events
}

fn make_resume_token(
    cluster_time: &bson::Timestamp,
    document_key: &bson::Document,
) -> bson::Document {
    bson::doc! {
        "_data": format!("{:010}_{:010}_{}", cluster_time.time, cluster_time.increment,
            document_key.get_str("_id").unwrap_or("unknown")),
    }
}

fn compute_update_description(
    previous: Option<bson::Document>,
    current: &bson::Document,
) -> bson::Document {
    let mut updated_fields = bson::Document::new();
    let mut removed_fields: Vec<bson::Bson> = Vec::new();

    let previous = previous.unwrap_or_default();

    for (key, value) in current {
        if key == "_id" {
            continue;
        }
        match previous.get(key) {
            Some(old_value) if old_value != value => {
                updated_fields.insert(key, value.clone());
            }
            None => {
                updated_fields.insert(key, value.clone());
            }
            _ => {}
        }
    }

    for key in previous.keys() {
        if key == "_id" {
            continue;
        }
        if !current.contains_key(key) {
            removed_fields.push(bson::Bson::String(key.clone()));
        }
    }

    bson::doc! {
        "updatedFields": updated_fields,
        "removedFields": removed_fields,
        "truncatedArrays": bson::Bson::Array(vec![]),
    }
}

#[cfg(test)]
mod tests {
    use neovex_core::{
        Document, DocumentId, SequenceNumber, SubscriptionResultSnapshot, TableName,
    };
    use serde_json::json;

    use super::*;

    fn make_doc(id: &str, fields: serde_json::Map<String, serde_json::Value>) -> Document {
        Document::with_id(
            DocumentId::from_key(id.to_string()).expect("id should parse"),
            TableName::new("users").expect("table should be valid"),
            fields,
        )
    }

    #[test]
    fn insert_event_has_correct_operation_type() {
        let doc = make_doc(
            "u1",
            serde_json::Map::from_iter([("name".to_string(), json!("Alice"))]),
        );
        let snapshot = SubscriptionResultSnapshot::bootstrap(SequenceNumber(1), vec![doc]);

        let events = snapshot_to_change_events("testdb.users", None, &snapshot);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].get_str("operationType").unwrap(), "insert");
        let full_doc = events[0].get_document("fullDocument").unwrap();
        assert_eq!(full_doc.get_str("name").unwrap(), "Alice");
    }

    #[test]
    fn update_event_includes_update_description() {
        let prev_doc = make_doc(
            "u1",
            serde_json::Map::from_iter([
                ("name".to_string(), json!("Alice")),
                ("age".to_string(), json!(30)),
            ]),
        );
        let curr_doc = make_doc(
            "u1",
            serde_json::Map::from_iter([
                ("name".to_string(), json!("Alice")),
                ("age".to_string(), json!(31)),
            ]),
        );

        let previous = SubscriptionResultSnapshot::bootstrap(SequenceNumber(1), vec![prev_doc]);
        let current = SubscriptionResultSnapshot::bootstrap(SequenceNumber(2), vec![curr_doc]);

        let events = snapshot_to_change_events("testdb.users", Some(&previous), &current);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].get_str("operationType").unwrap(), "update");
        let update_desc = events[0].get_document("updateDescription").unwrap();
        let updated = update_desc.get_document("updatedFields").unwrap();
        assert_eq!(updated.get_i32("age").unwrap_or(-1), 31);
    }

    #[test]
    fn delete_event_has_document_key() {
        let doc = make_doc(
            "u1",
            serde_json::Map::from_iter([("name".to_string(), json!("Alice"))]),
        );

        let previous = SubscriptionResultSnapshot::bootstrap(SequenceNumber(1), vec![doc]);
        let current = SubscriptionResultSnapshot::bootstrap(SequenceNumber(2), vec![]);

        let events = snapshot_to_change_events("testdb.users", Some(&previous), &current);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].get_str("operationType").unwrap(), "delete");
        assert!(events[0].get_document("documentKey").is_ok());
    }

    #[test]
    fn event_includes_namespace() {
        let doc = make_doc("u1", serde_json::Map::new());
        let snapshot = SubscriptionResultSnapshot::bootstrap(SequenceNumber(1), vec![doc]);

        let events = snapshot_to_change_events("mydb.mycoll", None, &snapshot);
        let ns = events[0].get_document("ns").unwrap();
        assert_eq!(ns.get_str("db").unwrap(), "mydb");
        assert_eq!(ns.get_str("coll").unwrap(), "mycoll");
    }

    #[test]
    fn event_includes_resume_token() {
        let doc = make_doc("u1", serde_json::Map::new());
        let snapshot = SubscriptionResultSnapshot::bootstrap(SequenceNumber(1), vec![doc]);

        let events = snapshot_to_change_events("testdb.users", None, &snapshot);
        let resume_id = events[0].get_document("_id").unwrap();
        assert!(resume_id.get_str("_data").is_ok());
    }

    #[test]
    fn compute_update_description_tracks_changes() {
        let prev = bson::doc! { "_id": "u1", "name": "Alice", "age": 30, "dept": "eng" };
        let curr = bson::doc! { "_id": "u1", "name": "Alice", "age": 31, "status": "active" };

        let desc = compute_update_description(Some(prev), &curr);
        let updated = desc.get_document("updatedFields").unwrap();
        let removed = desc.get_array("removedFields").unwrap();

        assert!(updated.get("age").is_some());
        assert!(updated.get("status").is_some());
        assert!(updated.get("name").is_none());
        assert!(removed.iter().any(|r| r.as_str() == Some("dept")));
    }

    #[test]
    fn empty_diff_produces_no_events() {
        let doc = make_doc("u1", serde_json::Map::new());
        let snapshot = SubscriptionResultSnapshot::bootstrap(SequenceNumber(1), vec![doc]);

        let events = snapshot_to_change_events("testdb.users", Some(&snapshot), &snapshot);
        assert!(events.is_empty());
    }

    #[test]
    fn change_stream_store_insert_and_contains() {
        let mut store = ChangeStreamStore::default();
        assert!(!store.contains(1));
        assert!(!store.remove(1));
    }

    #[test]
    fn make_resume_token_format() {
        let ts = bson::Timestamp {
            time: 12345,
            increment: 1,
        };
        let doc_key = bson::doc! { "_id": "abc" };
        let token = make_resume_token(&ts, &doc_key);
        let data = token.get_str("_data").unwrap();
        assert!(data.contains("12345"));
        assert!(data.contains("abc"));
    }

    #[test]
    fn parse_resume_token_valid() {
        let token = ResumeToken::parse("0000012345_0000000001_abc").unwrap();
        assert_eq!(token.time, 12345);
        assert_eq!(token.increment, 1);
        assert_eq!(token.document_id, "abc");
    }

    #[test]
    fn parse_resume_token_invalid_returns_none() {
        assert!(ResumeToken::parse("invalid").is_none());
        assert!(ResumeToken::parse("abc_def_ghi").is_none());
        assert!(ResumeToken::parse("").is_none());
    }

    #[test]
    fn parse_resume_token_roundtrip() {
        let ts = bson::Timestamp {
            time: 99,
            increment: 7,
        };
        let doc_key = bson::doc! { "_id": "mydoc" };
        let token_doc = make_resume_token(&ts, &doc_key);
        let data = token_doc.get_str("_data").unwrap();
        let parsed = ResumeToken::parse(data).unwrap();
        assert_eq!(parsed.time, 99);
        assert_eq!(parsed.increment, 7);
        assert_eq!(parsed.document_id, "mydoc");
    }

    #[test]
    fn resume_token_to_cluster_time() {
        let token = ResumeToken {
            time: 100,
            increment: 5,
            document_id: "x".into(),
        };
        let ct = token.to_cluster_time();
        assert_eq!(ct.time, 100);
        assert_eq!(ct.increment, 5);
    }

    #[test]
    fn parse_resume_token_from_bson_doc() {
        let doc = bson::doc! { "_data": "0000000100_0000000005_doc1" };
        let token = parse_resume_token_from_doc(&doc).unwrap();
        assert_eq!(token.time, 100);
        assert_eq!(token.increment, 5);
        assert_eq!(token.document_id, "doc1");
    }

    #[test]
    fn extract_resume_after_option() {
        let cs_doc = bson::doc! {
            "resumeAfter": { "_data": "0000000050_0000000002_abc" }
        };
        let token = extract_resume_option(&cs_doc).unwrap();
        assert_eq!(token.time, 50);
        assert_eq!(token.increment, 2);
        assert_eq!(token.document_id, "abc");
    }

    #[test]
    fn extract_start_after_option() {
        let cs_doc = bson::doc! {
            "startAfter": { "_data": "0000000070_0000000003_xyz" }
        };
        let token = extract_resume_option(&cs_doc).unwrap();
        assert_eq!(token.time, 70);
        assert_eq!(token.increment, 3);
        assert_eq!(token.document_id, "xyz");
    }

    #[test]
    fn extract_resume_option_prefers_resume_after() {
        let cs_doc = bson::doc! {
            "resumeAfter": { "_data": "0000000001_0000000001_a" },
            "startAfter": { "_data": "0000000002_0000000002_b" }
        };
        let token = extract_resume_option(&cs_doc).unwrap();
        assert_eq!(token.time, 1);
        assert_eq!(token.document_id, "a");
    }

    #[test]
    fn extract_resume_option_empty_doc() {
        let cs_doc = bson::Document::new();
        assert!(extract_resume_option(&cs_doc).is_none());
    }

    #[test]
    fn filter_events_after_resume_filters_old_events() {
        let events = vec![
            bson::doc! {
                "operationType": "insert",
                "clusterTime": bson::Timestamp { time: 10, increment: 1 },
            },
            bson::doc! {
                "operationType": "insert",
                "clusterTime": bson::Timestamp { time: 20, increment: 1 },
            },
            bson::doc! {
                "operationType": "insert",
                "clusterTime": bson::Timestamp { time: 30, increment: 1 },
            },
        ];
        let token = ResumeToken {
            time: 20,
            increment: 1,
            document_id: "x".into(),
        };
        let filtered = filter_events_after_resume(events, &token);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].get_timestamp("clusterTime").unwrap().time, 30);
    }

    #[test]
    fn filter_events_after_resume_keeps_all_when_token_is_old() {
        let events = vec![bson::doc! {
            "operationType": "insert",
            "clusterTime": bson::Timestamp { time: 100, increment: 1 },
        }];
        let token = ResumeToken {
            time: 50,
            increment: 0,
            document_id: "x".into(),
        };
        let filtered = filter_events_after_resume(events, &token);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn filter_events_same_time_different_increment() {
        let events = vec![
            bson::doc! {
                "operationType": "insert",
                "clusterTime": bson::Timestamp { time: 10, increment: 1 },
            },
            bson::doc! {
                "operationType": "insert",
                "clusterTime": bson::Timestamp { time: 10, increment: 3 },
            },
        ];
        let token = ResumeToken {
            time: 10,
            increment: 2,
            document_id: "x".into(),
        };
        let filtered = filter_events_after_resume(events, &token);
        assert_eq!(filtered.len(), 1);
        assert_eq!(
            filtered[0].get_timestamp("clusterTime").unwrap().increment,
            3
        );
    }

    #[test]
    fn invalidate_event_format() {
        let event = invalidate_event("testdb.users");
        assert_eq!(event.get_str("operationType").unwrap(), "invalidate");
        let ns = event.get_document("ns").unwrap();
        assert_eq!(ns.get_str("db").unwrap(), "testdb");
        assert_eq!(ns.get_str("coll").unwrap(), "users");
        assert!(event.get_document("_id").is_ok());
    }

    #[test]
    fn resume_token_in_change_event_is_parseable() {
        use neovex_core::{CommitEntry, SequenceNumber, Timestamp};
        let doc = make_doc(
            "u1",
            serde_json::Map::from_iter([("name".to_string(), json!("Alice"))]),
        );
        let snapshot = SubscriptionResultSnapshot::from_delivery(
            SequenceNumber(5),
            Some(&CommitEntry {
                sequence: SequenceNumber(5),
                timestamp: Timestamp(12000),
                writes: vec![],
            }),
            vec![doc],
            vec![],
        );

        let events = snapshot_to_change_events("testdb.users", None, &snapshot);
        assert_eq!(events.len(), 1);

        let resume_doc = events[0].get_document("_id").unwrap();
        let data = resume_doc.get_str("_data").unwrap();
        let parsed = ResumeToken::parse(data).unwrap();
        assert_eq!(parsed.time, 12);
        assert_eq!(parsed.increment, 5);
        assert_eq!(parsed.document_id, "u1");
    }
}
