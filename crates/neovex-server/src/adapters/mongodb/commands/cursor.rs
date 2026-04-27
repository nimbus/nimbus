use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

use super::super::connection::ConnectionState;
use super::super::error::{BAD_VALUE, MongoError, NAMESPACE_NOT_FOUND};

static NEXT_CURSOR_ID: AtomicI64 = AtomicI64::new(1);

pub fn next_cursor_id() -> i64 {
    NEXT_CURSOR_ID.fetch_add(1, Ordering::Relaxed)
}

struct StoredCursor {
    ns: String,
    documents: Vec<bson::Document>,
    position: usize,
    batch_size: usize,
}

#[derive(Default)]
pub(crate) struct CursorStore {
    cursors: HashMap<i64, StoredCursor>,
}

impl CursorStore {
    pub fn create(
        &mut self,
        ns: String,
        documents: Vec<bson::Document>,
        batch_size: usize,
    ) -> (i64, Vec<bson::Bson>) {
        let first_batch: Vec<bson::Bson> = documents
            .iter()
            .take(batch_size)
            .cloned()
            .map(bson::Bson::Document)
            .collect();

        let consumed = first_batch.len();
        if consumed >= documents.len() {
            return (0, first_batch);
        }

        let cursor_id = next_cursor_id();
        self.cursors.insert(
            cursor_id,
            StoredCursor {
                ns,
                documents,
                position: consumed,
                batch_size,
            },
        );

        (cursor_id, first_batch)
    }

    fn get_more(&mut self, cursor_id: i64, batch_size: Option<usize>) -> Option<GetMoreResult> {
        let cursor = self.cursors.get_mut(&cursor_id)?;
        let effective_batch_size = batch_size.unwrap_or(cursor.batch_size);
        let remaining = cursor.documents.len() - cursor.position;
        let take = effective_batch_size.min(remaining);

        let next_batch: Vec<bson::Bson> = cursor.documents[cursor.position..cursor.position + take]
            .iter()
            .cloned()
            .map(bson::Bson::Document)
            .collect();

        cursor.position += take;
        let ns = cursor.ns.clone();

        if cursor.position >= cursor.documents.len() {
            self.cursors.remove(&cursor_id);
            Some(GetMoreResult {
                ns,
                next_batch,
                cursor_id: 0,
            })
        } else {
            Some(GetMoreResult {
                ns,
                next_batch,
                cursor_id,
            })
        }
    }

    pub fn kill(&mut self, cursor_id: i64) -> bool {
        self.cursors.remove(&cursor_id).is_some()
    }

    pub fn kill_all(&mut self) {
        self.cursors.clear();
    }
}

struct GetMoreResult {
    ns: String,
    next_batch: Vec<bson::Bson>,
    cursor_id: i64,
}

pub fn get_more(
    body: &bson::Document,
    conn: &mut ConnectionState,
) -> Result<bson::Document, MongoError> {
    let cursor_id = body.get_i64("getMore").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing cursor id in getMore command".into(),
    })?;

    if cursor_id == 0 {
        return Err(MongoError::Command {
            code: BAD_VALUE.code,
            code_name: BAD_VALUE.code_name.into(),
            message: "cursor id 0 is not valid for getMore".into(),
        });
    }

    let batch_size = match body.get("batchSize") {
        Some(bson::Bson::Int32(n)) if *n > 0 => Some(*n as usize),
        Some(bson::Bson::Int64(n)) if *n > 0 => Some(*n as usize),
        _ => None,
    };

    match conn.cursor_store.get_more(cursor_id, batch_size) {
        Some(result) => Ok(bson::doc! {
            "cursor": {
                "nextBatch": result.next_batch,
                "id": result.cursor_id,
                "ns": &result.ns,
            },
            "ok": 1.0,
        }),
        None => Err(MongoError::Command {
            code: NAMESPACE_NOT_FOUND.code,
            code_name: NAMESPACE_NOT_FOUND.code_name.into(),
            message: format!("cursor id {cursor_id} not found"),
        }),
    }
}

pub fn kill_cursors(
    body: &bson::Document,
    conn: &mut ConnectionState,
) -> Result<bson::Document, MongoError> {
    let cursor_ids = body.get_array("cursors").map_err(|_| MongoError::Command {
        code: BAD_VALUE.code,
        code_name: BAD_VALUE.code_name.into(),
        message: "missing cursors array in killCursors command".into(),
    })?;

    let mut cursors_killed = Vec::new();
    let mut cursors_not_found = Vec::new();

    for id_bson in cursor_ids {
        if let Some(id) = id_bson.as_i64() {
            if conn.cursor_store.kill(id) {
                cursors_killed.push(bson::Bson::Int64(id));
            } else {
                cursors_not_found.push(bson::Bson::Int64(id));
            }
        }
    }

    Ok(bson::doc! {
        "cursorsKilled": cursors_killed,
        "cursorsNotFound": cursors_not_found,
        "cursorsAlive": Vec::<bson::Bson>::new(),
        "cursorsUnknown": Vec::<bson::Bson>::new(),
        "ok": 1.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bson_docs(count: usize) -> Vec<bson::Document> {
        (0..count)
            .map(|i| bson::doc! { "_id": format!("d{i}"), "idx": i as i32 })
            .collect()
    }

    #[test]
    fn cursor_store_returns_zero_id_when_all_fit() {
        let mut store = CursorStore::default();
        let (id, batch) = store.create("db.col".into(), make_bson_docs(3), 10);
        assert_eq!(id, 0);
        assert_eq!(batch.len(), 3);
    }

    #[test]
    fn cursor_store_creates_cursor_for_overflow() {
        let mut store = CursorStore::default();
        let (id, batch) = store.create("db.col".into(), make_bson_docs(5), 2);
        assert_ne!(id, 0);
        assert_eq!(batch.len(), 2);
    }

    #[test]
    fn get_more_returns_next_batch() {
        let mut store = CursorStore::default();
        let (id, _) = store.create("db.col".into(), make_bson_docs(5), 2);

        let result = store.get_more(id, None).unwrap();
        assert_eq!(result.next_batch.len(), 2);
        assert_ne!(result.cursor_id, 0);

        let result2 = store.get_more(result.cursor_id, None).unwrap();
        assert_eq!(result2.next_batch.len(), 1);
        assert_eq!(result2.cursor_id, 0);
    }

    #[test]
    fn get_more_with_custom_batch_size() {
        let mut store = CursorStore::default();
        let (id, _) = store.create("db.col".into(), make_bson_docs(10), 3);

        let result = store.get_more(id, Some(5)).unwrap();
        assert_eq!(result.next_batch.len(), 5);
    }

    #[test]
    fn get_more_exhausted_cursor_returns_none() {
        let mut store = CursorStore::default();
        let (id, _) = store.create("db.col".into(), make_bson_docs(2), 1);

        let result = store.get_more(id, None).unwrap();
        assert_eq!(result.cursor_id, 0);
        assert!(store.get_more(id, None).is_none());
    }

    #[test]
    fn kill_removes_cursor() {
        let mut store = CursorStore::default();
        let (id, _) = store.create("db.col".into(), make_bson_docs(5), 2);
        assert!(store.kill(id));
        assert!(store.get_more(id, None).is_none());
    }

    #[test]
    fn kill_nonexistent_returns_false() {
        let mut store = CursorStore::default();
        assert!(!store.kill(999));
    }

    #[test]
    fn kill_all_clears_everything() {
        let mut store = CursorStore::default();
        let (id1, _) = store.create("db.a".into(), make_bson_docs(5), 2);
        let (id2, _) = store.create("db.b".into(), make_bson_docs(5), 2);
        store.kill_all();
        assert!(store.get_more(id1, None).is_none());
        assert!(store.get_more(id2, None).is_none());
    }

    #[test]
    fn get_more_command_cursor_zero_is_invalid() {
        let mut conn = ConnectionState::new(([127, 0, 0, 1], 12345).into());
        let body = bson::doc! { "getMore": 0_i64 };
        let err = get_more(&body, &mut conn).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn get_more_command_nonexistent_cursor() {
        let mut conn = ConnectionState::new(([127, 0, 0, 1], 12345).into());
        let body = bson::doc! { "getMore": 999_i64 };
        let err = get_more(&body, &mut conn).unwrap_err();
        match err {
            MongoError::Command { code, .. } => assert_eq!(code, NAMESPACE_NOT_FOUND.code),
            other => panic!("expected Command, got {:?}", other),
        }
    }

    #[test]
    fn kill_cursors_command_mixed() {
        let mut conn = ConnectionState::new(([127, 0, 0, 1], 12345).into());
        let (cursor_id, _) = conn
            .cursor_store
            .create("db.col".into(), make_bson_docs(5), 2);

        let body = bson::doc! {
            "killCursors": "col",
            "cursors": [cursor_id, 9999_i64],
        };
        let result = kill_cursors(&body, &mut conn).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
        assert_eq!(result.get_array("cursorsKilled").unwrap().len(), 1);
        assert_eq!(result.get_array("cursorsNotFound").unwrap().len(), 1);
    }

    #[test]
    fn get_more_command_returns_correct_format() {
        let mut conn = ConnectionState::new(([127, 0, 0, 1], 12345).into());
        let (cursor_id, _) = conn
            .cursor_store
            .create("db.col".into(), make_bson_docs(5), 2);

        let body = bson::doc! { "getMore": cursor_id, "batchSize": 2 };
        let result = get_more(&body, &mut conn).unwrap();
        assert_eq!(result.get_f64("ok").unwrap(), 1.0);
        let cursor = result.get_document("cursor").unwrap();
        assert_eq!(cursor.get_array("nextBatch").unwrap().len(), 2);
        assert_eq!(cursor.get_str("ns").unwrap(), "db.col");
        assert_ne!(cursor.get_i64("id").unwrap(), 0);
    }
}
