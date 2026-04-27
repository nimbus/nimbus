use super::super::super::connection::ConnectionState;
use super::*;
use neovex_testing::ServiceFixture;

fn test_conn() -> ConnectionState {
    ConnectionState::new(([127, 0, 0, 1], 12345).into())
}

#[test]
fn insert_single_document() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! {
        "insert": "users",
        "$db": "testdb",
        "documents": [
            { "_id": "user1", "name": "Alice", "age": 30 }
        ],
    };
    let result = insert(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 1);
    assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    assert!(result.get_array("writeErrors").is_err());
}

#[test]
fn insert_multiple_documents() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! {
        "insert": "users",
        "$db": "testdb",
        "documents": [
            { "_id": "u1", "name": "Alice" },
            { "_id": "u2", "name": "Bob" },
            { "_id": "u3", "name": "Charlie" },
        ],
    };
    let result = insert(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 3);
}

#[test]
fn insert_auto_generates_id() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! {
        "insert": "users",
        "$db": "testdb",
        "documents": [
            { "name": "NoId" }
        ],
    };
    let result = insert(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 1);
}

#[test]
fn insert_ordered_stops_on_duplicate() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body1 = bson::doc! {
        "insert": "users",
        "$db": "testdb",
        "documents": [{ "_id": "dup", "name": "First" }],
    };
    insert(&body1, &mut test_conn(), &fixture.service()).unwrap();

    let body2 = bson::doc! {
        "insert": "users",
        "$db": "testdb",
        "ordered": true,
        "documents": [
            { "_id": "dup", "name": "Duplicate" },
            { "_id": "new1", "name": "Should not insert" },
        ],
    };
    let result = insert(&body2, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 0);
    let errors = result.get_array("writeErrors").unwrap();
    assert_eq!(errors.len(), 1);
}

#[test]
fn insert_unordered_continues_past_errors() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body1 = bson::doc! {
        "insert": "users",
        "$db": "testdb",
        "documents": [{ "_id": "dup2", "name": "First" }],
    };
    insert(&body1, &mut test_conn(), &fixture.service()).unwrap();

    let body2 = bson::doc! {
        "insert": "users",
        "$db": "testdb",
        "ordered": false,
        "documents": [
            { "_id": "dup2", "name": "Duplicate" },
            { "_id": "new2", "name": "Should insert" },
        ],
    };
    let result = insert(&body2, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 1);
    let errors = result.get_array("writeErrors").unwrap();
    assert_eq!(errors.len(), 1);
}

#[test]
fn insert_missing_collection_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! {
        "documents": [{ "name": "test" }],
    };
    let err = insert(&body, &mut test_conn(), &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn insert_missing_documents_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! {
        "insert": "users",
    };
    let err = insert(&body, &mut test_conn(), &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn insert_into_admin_db_uses_default_tenant() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! {
        "insert": "testcol",
        "$db": "admin",
        "documents": [{ "_id": "a1", "val": 1 }],
    };
    let result = insert(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 1);
}

fn seed_users(fixture: &ServiceFixture<Service>) {
    let body = bson::doc! {
        "insert": "users",
        "$db": "testdb",
        "documents": [
            { "_id": "u1", "name": "Alice", "age": 30 },
            { "_id": "u2", "name": "Bob", "age": 25 },
            { "_id": "u3", "name": "Charlie", "age": 35 },
        ],
    };
    insert(&body, &mut test_conn(), &fixture.service()).unwrap();
}

#[test]
fn find_all_documents() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! { "find": "users", "$db": "testdb" };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_f64("ok").unwrap(), 1.0);

    let cursor = result.get_document("cursor").unwrap();
    let first_batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(first_batch.len(), 3);
    assert_eq!(cursor.get_i64("id").unwrap(), 0);
    assert_eq!(cursor.get_str("ns").unwrap(), "testdb.users");
}

#[test]
fn find_with_equality_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "filter": { "name": "Alice" },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 1);
    let doc = batch[0].as_document().unwrap();
    assert_eq!(doc.get_str("name").unwrap(), "Alice");
}

#[test]
fn find_with_comparison_operators() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "filter": { "age": { "$gt": 25 } },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 2);
}

#[test]
fn find_with_gte_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "filter": { "age": { "$gte": 30 } },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 2);
}

#[test]
fn find_with_lt_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "filter": { "age": { "$lt": 30 } },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 1);
    let doc = batch[0].as_document().unwrap();
    assert_eq!(doc.get_str("name").unwrap(), "Bob");
}

#[test]
fn find_with_ne_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "filter": { "name": { "$ne": "Alice" } },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 2);
}

#[test]
fn find_with_combined_range() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "filter": { "age": { "$gte": 25, "$lte": 30 } },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 2);
}

#[test]
fn find_with_limit() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "limit": 2,
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 2);
}

#[test]
fn find_with_skip() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "skip": 2,
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 1);
}

#[test]
fn find_with_sort_ascending() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "sort": { "age": 1 },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 3);
    let first = batch[0].as_document().unwrap();
    let last = batch[2].as_document().unwrap();
    assert_eq!(first.get_str("name").unwrap(), "Bob");
    assert_eq!(last.get_str("name").unwrap(), "Charlie");
}

#[test]
fn find_with_sort_descending() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "sort": { "age": -1 },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 3);
    let first = batch[0].as_document().unwrap();
    let last = batch[2].as_document().unwrap();
    assert_eq!(first.get_str("name").unwrap(), "Charlie");
    assert_eq!(last.get_str("name").unwrap(), "Bob");
}

#[test]
fn find_with_compound_sort() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! {
        "insert": "items",
        "$db": "testdb",
        "documents": [
            { "_id": "i1", "category": "b", "priority": 2 },
            { "_id": "i2", "category": "a", "priority": 3 },
            { "_id": "i3", "category": "a", "priority": 1 },
            { "_id": "i4", "category": "b", "priority": 1 },
        ],
    };
    insert(&body, &mut test_conn(), &fixture.service()).unwrap();

    let find_body = bson::doc! {
        "find": "items",
        "$db": "testdb",
        "sort": { "category": 1, "priority": -1 },
    };
    let result = find(&find_body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 4);
    let ids: Vec<&str> = batch
        .iter()
        .map(|b| b.as_document().unwrap().get_str("_id").unwrap())
        .collect();
    assert_eq!(ids, vec!["i2", "i3", "i1", "i4"]);
}

#[test]
fn find_with_inclusion_projection() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "filter": { "name": "Alice" },
        "projection": { "name": 1 },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 1);
    let doc = batch[0].as_document().unwrap();
    assert_eq!(doc.get_str("name").unwrap(), "Alice");
    assert!(doc.get("_id").is_some());
    assert!(doc.get("age").is_none());
}

#[test]
fn find_with_exclusion_projection() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "filter": { "name": "Alice" },
        "projection": { "age": 0 },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    let doc = batch[0].as_document().unwrap();
    assert_eq!(doc.get_str("name").unwrap(), "Alice");
    assert!(doc.get("_id").is_some());
    assert!(doc.get("age").is_none());
}

#[test]
fn find_with_id_exclusion_in_inclusion_projection() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "filter": { "name": "Alice" },
        "projection": { "name": 1, "_id": 0 },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    let doc = batch[0].as_document().unwrap();
    assert_eq!(doc.get_str("name").unwrap(), "Alice");
    assert!(doc.get("_id").is_none());
}

#[test]
fn find_empty_collection_returns_empty_batch() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let ensure_body = bson::doc! {
        "insert": "empty_col",
        "$db": "testdb",
        "documents": [{ "_id": "tmp" }],
    };
    insert(&ensure_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "find": "empty_col",
        "$db": "testdb",
        "filter": { "nonexistent": "value" },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 0);
}

#[test]
fn find_no_match_returns_empty_batch() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "filter": { "name": "Nonexistent" },
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 0);
}

#[test]
fn find_missing_collection_name_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! { "$db": "testdb" };
    let err = find(&body, &mut test_conn(), &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn find_with_batch_size() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "batchSize": 1,
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 1);
}

#[test]
fn find_with_sort_limit_skip_combined() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "sort": { "age": 1 },
        "skip": 1,
        "limit": 1,
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 1);
    let doc = batch[0].as_document().unwrap();
    assert_eq!(doc.get_str("name").unwrap(), "Alice");
}

#[test]
fn find_unsupported_operator_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "find": "users",
        "$db": "testdb",
        "filter": { "tags": { "$in": ["a", "b"] } },
    };
    let err = find(&body, &mut test_conn(), &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, message, .. } => {
            assert_eq!(code, BAD_VALUE.code);
            assert!(message.contains("$in"));
        }
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn find_default_db_uses_default_tenant() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "items",
        "documents": [{ "_id": "i1", "val": 42 }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! { "find": "items" };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 1);
}

fn find_doc(fixture: &ServiceFixture<Service>, filter: bson::Document) -> Vec<bson::Document> {
    find_in(fixture, "users", filter)
}

fn find_in(
    fixture: &ServiceFixture<Service>,
    collection: &str,
    filter: bson::Document,
) -> Vec<bson::Document> {
    let body = bson::doc! {
        "find": collection,
        "$db": "testdb",
        "filter": filter,
    };
    let result = find(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    cursor
        .get_array("firstBatch")
        .unwrap()
        .iter()
        .filter_map(|b| b.as_document().cloned())
        .collect()
}

#[test]
fn update_replacement() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "u1" },
            "u": { "name": "Alice Updated", "score": 100 },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 1);
    assert_eq!(result.get_i32("nModified").unwrap(), 1);
    assert_eq!(result.get_f64("ok").unwrap(), 1.0);

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].get_str("name").unwrap(), "Alice Updated");
    assert_eq!(docs[0].get_i32("score").unwrap(), 100);
    assert!(docs[0].get("age").is_none());
}

#[test]
fn update_set_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "u1" },
            "u": { "$set": { "age": 31, "email": "alice@test.com" } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 1);
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert_eq!(docs[0].get_i32("age").unwrap(), 31);
    assert_eq!(docs[0].get_str("email").unwrap(), "alice@test.com");
    assert_eq!(docs[0].get_str("name").unwrap(), "Alice");
}

#[test]
fn update_unset_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "u1" },
            "u": { "$unset": { "age": "" } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert!(docs[0].get("age").is_none() || docs[0].get("age") == Some(&bson::Bson::Null));
}

#[test]
fn update_no_match_returns_zero() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "nonexistent" },
            "u": { "$set": { "x": 1 } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 0);
    assert_eq!(result.get_i32("nModified").unwrap(), 0);
}

#[test]
fn update_upsert_creates_document() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "u4" },
            "u": { "$set": { "name": "Dave" } },
            "upsert": true,
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 1);
    assert!(result.get_array("upserted").is_ok());

    let docs = find_doc(&fixture, bson::doc! { "_id": "u4" });
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].get_str("name").unwrap(), "Dave");
}

#[test]
fn update_multi_updates_all_matching() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "age": { "$gte": 30 } },
            "u": { "$set": { "senior": true } },
            "multi": true,
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 2);
    assert_eq!(result.get_i32("nModified").unwrap(), 2);
}

#[test]
fn update_multi_replacement_rejected() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": {},
            "u": { "name": "replaced" },
            "multi": true,
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    let errors = result.get_array("writeErrors").unwrap();
    assert_eq!(errors.len(), 1);
}

#[test]
fn update_missing_collection_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! { "updates": [] };
    let err = update(&body, &mut test_conn(), &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn update_missing_updates_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! { "update": "users" };
    let err = update(&body, &mut test_conn(), &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn update_inc_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "u1" },
            "u": { "$inc": { "age": 5 } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert_eq!(docs[0].get_i32("age").unwrap(), 35);
}

#[test]
fn update_unsupported_operator_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "u1" },
            "u": { "$unknownOp": { "x": 1 } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    let errors = result.get_array("writeErrors").unwrap();
    assert_eq!(errors.len(), 1);
}

#[test]
fn update_set_on_insert_with_upsert() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "u5" },
            "u": { "$set": { "name": "Eve" }, "$setOnInsert": { "role": "admin" } },
            "upsert": true,
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 1);
    assert!(result.get_array("upserted").is_ok());

    let docs = find_doc(&fixture, bson::doc! { "_id": "u5" });
    assert_eq!(docs[0].get_str("name").unwrap(), "Eve");
    assert_eq!(docs[0].get_str("role").unwrap(), "admin");
}

#[test]
fn update_mul_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "u1" },
            "u": { "$mul": { "age": 2 } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    let age = docs[0].get("age").unwrap();
    let age_f64 = match age {
        bson::Bson::Double(f) => *f,
        bson::Bson::Int32(n) => *n as f64,
        bson::Bson::Int64(n) => *n as f64,
        other => panic!("unexpected age type: {:?}", other),
    };
    assert!((age_f64 - 60.0).abs() < 0.01);
}

#[test]
fn update_push_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "items",
        "$db": "testdb",
        "documents": [{ "_id": "i1", "tags": ["a", "b"] }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "update": "items",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "i1" },
            "u": { "$push": { "tags": "c" } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_in(&fixture, "items", bson::doc! { "_id": "i1" });
    let tags = docs[0].get_array("tags").unwrap();
    assert_eq!(tags.len(), 3);
    assert_eq!(tags[2].as_str().unwrap(), "c");
}

#[test]
fn update_push_each_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "items",
        "$db": "testdb",
        "documents": [{ "_id": "i2", "tags": ["a"] }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "update": "items",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "i2" },
            "u": { "$push": { "tags": { "$each": ["b", "c"] } } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_in(&fixture, "items", bson::doc! { "_id": "i2" });
    let tags = docs[0].get_array("tags").unwrap();
    assert_eq!(tags.len(), 3);
}

#[test]
fn update_add_to_set_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "items",
        "$db": "testdb",
        "documents": [{ "_id": "i3", "tags": ["a", "b"] }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "update": "items",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "i3" },
            "u": { "$addToSet": { "tags": "c" } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_in(&fixture, "items", bson::doc! { "_id": "i3" });
    let tags = docs[0].get_array("tags").unwrap();
    assert_eq!(tags.len(), 3);
}

#[test]
fn update_add_to_set_duplicate_ignored() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "items",
        "$db": "testdb",
        "documents": [{ "_id": "i4", "tags": ["a", "b"] }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "update": "items",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "i4" },
            "u": { "$addToSet": { "tags": "a" } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_in(&fixture, "items", bson::doc! { "_id": "i4" });
    let tags = docs[0].get_array("tags").unwrap();
    assert_eq!(tags.len(), 2);
}

#[test]
fn update_pull_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "items",
        "$db": "testdb",
        "documents": [{ "_id": "i5", "tags": ["a", "b", "c"] }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "update": "items",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "i5" },
            "u": { "$pull": { "tags": "b" } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_in(&fixture, "items", bson::doc! { "_id": "i5" });
    let tags = docs[0].get_array("tags").unwrap();
    assert_eq!(tags.len(), 2);
    assert!(tags.iter().all(|t| t.as_str().unwrap() != "b"));
}

#[test]
fn update_pull_all_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "items",
        "$db": "testdb",
        "documents": [{ "_id": "i6", "tags": ["a", "b", "c", "d"] }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "update": "items",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "i6" },
            "u": { "$pullAll": { "tags": ["b", "d"] } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_in(&fixture, "items", bson::doc! { "_id": "i6" });
    let tags = docs[0].get_array("tags").unwrap();
    assert_eq!(tags.len(), 2);
}

#[test]
fn update_pop_last_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "items",
        "$db": "testdb",
        "documents": [{ "_id": "i7", "vals": [1, 2, 3] }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "update": "items",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "i7" },
            "u": { "$pop": { "vals": 1 } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_in(&fixture, "items", bson::doc! { "_id": "i7" });
    let vals = docs[0].get_array("vals").unwrap();
    assert_eq!(vals.len(), 2);
    assert_eq!(vals[1].as_i32().unwrap(), 2);
}

#[test]
fn update_pop_first_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "items",
        "$db": "testdb",
        "documents": [{ "_id": "i8", "vals": [1, 2, 3] }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "update": "items",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "i8" },
            "u": { "$pop": { "vals": -1 } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_in(&fixture, "items", bson::doc! { "_id": "i8" });
    let vals = docs[0].get_array("vals").unwrap();
    assert_eq!(vals.len(), 2);
    assert_eq!(vals[0].as_i32().unwrap(), 2);
}

#[test]
fn update_bit_and_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "items",
        "$db": "testdb",
        "documents": [{ "_id": "b1", "flags": 0b1111_i32 }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "update": "items",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "b1" },
            "u": { "$bit": { "flags": { "and": 0b1010_i32 } } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_in(&fixture, "items", bson::doc! { "_id": "b1" });
    let flags = docs[0]
        .get_i64("flags")
        .or_else(|_| docs[0].get_i32("flags").map(|n| n as i64))
        .unwrap();
    assert_eq!(flags, 0b1010);
}

#[test]
fn update_bit_or_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "items",
        "$db": "testdb",
        "documents": [{ "_id": "b2", "flags": 0b1010_i32 }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "update": "items",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "b2" },
            "u": { "$bit": { "flags": { "or": 0b0101_i32 } } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_in(&fixture, "items", bson::doc! { "_id": "b2" });
    let flags = docs[0]
        .get_i64("flags")
        .or_else(|_| docs[0].get_i32("flags").map(|n| n as i64))
        .unwrap();
    assert_eq!(flags, 0b1111);
}

#[test]
fn update_rename_operator() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "u1" },
            "u": { "$rename": { "name": "fullName" } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert_eq!(docs[0].get_str("fullName").unwrap(), "Alice");
    assert!(docs[0].get("name").is_none() || docs[0].get("name") == Some(&bson::Bson::Null));
}

#[test]
fn update_push_to_new_field() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "u1" },
            "u": { "$push": { "tags": "rust" } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    let tags = docs[0].get_array("tags").unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].as_str().unwrap(), "rust");
}

#[test]
fn update_mul_missing_field_is_zero() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "update": "users",
        "$db": "testdb",
        "updates": [{
            "q": { "_id": "u1" },
            "u": { "$mul": { "score": 5 } },
        }],
    };
    let result = update(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("nModified").unwrap(), 1);

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    let score = docs[0].get("score").unwrap();
    let score_f64 = match score {
        bson::Bson::Double(f) => *f,
        bson::Bson::Int32(n) => *n as f64,
        bson::Bson::Int64(n) => *n as f64,
        other => panic!("unexpected score type: {:?}", other),
    };
    assert!((score_f64 - 0.0).abs() < 0.01);
}

#[test]
fn delete_single_document() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "delete": "users",
        "$db": "testdb",
        "deletes": [{
            "q": { "_id": "u1" },
            "limit": 1,
        }],
    };
    let result = delete(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 1);
    assert_eq!(result.get_f64("ok").unwrap(), 1.0);

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert_eq!(docs.len(), 0);

    let all = find_doc(&fixture, bson::doc! {});
    assert_eq!(all.len(), 2);
}

#[test]
fn delete_multi_documents() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "delete": "users",
        "$db": "testdb",
        "deletes": [{
            "q": { "age": { "$gte": 30 } },
            "limit": 0,
        }],
    };
    let result = delete(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 2);

    let all = find_doc(&fixture, bson::doc! {});
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].get_str("name").unwrap(), "Bob");
}

#[test]
fn delete_no_match_returns_zero() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "delete": "users",
        "$db": "testdb",
        "deletes": [{
            "q": { "_id": "nonexistent" },
            "limit": 1,
        }],
    };
    let result = delete(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 0);
}

#[test]
fn delete_all_with_empty_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "delete": "users",
        "$db": "testdb",
        "deletes": [{
            "q": {},
            "limit": 0,
        }],
    };
    let result = delete(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 3);

    let all = find_doc(&fixture, bson::doc! {});
    assert_eq!(all.len(), 0);
}

#[test]
fn delete_limit_one_only_removes_one() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "delete": "users",
        "$db": "testdb",
        "deletes": [{
            "q": {},
            "limit": 1,
        }],
    };
    let result = delete(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 1);

    let all = find_doc(&fixture, bson::doc! {});
    assert_eq!(all.len(), 2);
}

#[test]
fn delete_missing_collection_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! { "deletes": [] };
    let err = delete(&body, &mut test_conn(), &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn delete_missing_deletes_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! { "delete": "users" };
    let err = delete(&body, &mut test_conn(), &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn delete_multiple_entries_ordered() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "delete": "users",
        "$db": "testdb",
        "deletes": [
            { "q": { "_id": "u1" }, "limit": 1 },
            { "q": { "_id": "u2" }, "limit": 1 },
        ],
    };
    let result = delete(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 2);

    let all = find_doc(&fixture, bson::doc! {});
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].get_str("name").unwrap(), "Charlie");
}

#[test]
fn find_and_modify_update_returns_old() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u1" },
        "update": { "$set": { "age": 99 } },
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_i32("age").unwrap(), 30);

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert_eq!(docs[0].get_i32("age").unwrap(), 99);
}

#[test]
fn find_and_modify_update_returns_new() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u1" },
        "update": { "$set": { "age": 99 } },
        "new": true,
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.service()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_i32("age").unwrap(), 99);
}

#[test]
fn find_and_modify_remove() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u1" },
        "remove": true,
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.service()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_str("name").unwrap(), "Alice");

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert_eq!(docs.len(), 0);
}

#[test]
fn find_and_modify_no_match_returns_null() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "nonexistent" },
        "update": { "$set": { "x": 1 } },
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.service()).unwrap();
    assert!(result.get("value").unwrap().as_null().is_some());
}

#[test]
fn find_and_modify_upsert() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u9" },
        "update": { "$set": { "name": "Upserted" } },
        "upsert": true,
        "new": true,
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.service()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_str("name").unwrap(), "Upserted");

    let leo = result.get_document("lastErrorObject").unwrap();
    assert!(!leo.get_bool("updatedExisting").unwrap());
}

#[test]
fn find_and_modify_with_fields_projection() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u1" },
        "update": { "$set": { "age": 99 } },
        "fields": { "name": 1 },
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.service()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_str("name").unwrap(), "Alice");
    assert!(value.get("_id").is_some());
    assert!(value.get("age").is_none());
}

#[test]
fn find_and_modify_replacement() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u1" },
        "update": { "name": "Replaced", "score": 42 },
        "new": true,
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.service()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_str("name").unwrap(), "Replaced");
    assert_eq!(value.get_i32("score").unwrap(), 42);
    assert!(value.get("age").is_none());
}

#[test]
fn count_all_documents() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! { "count": "users", "$db": "testdb" };
    let result = count(&body, &fixture.service()).unwrap();
    assert_eq!(result.get_i64("n").unwrap(), 3);
    assert_eq!(result.get_f64("ok").unwrap(), 1.0);
}

#[test]
fn count_with_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "count": "users",
        "$db": "testdb",
        "query": { "age": { "$gte": 30 } },
    };
    let result = count(&body, &fixture.service()).unwrap();
    assert_eq!(result.get_i64("n").unwrap(), 2);
}

#[test]
fn count_with_limit() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "count": "users",
        "$db": "testdb",
        "limit": 2,
    };
    let result = count(&body, &fixture.service()).unwrap();
    assert_eq!(result.get_i64("n").unwrap(), 2);
}

#[test]
fn count_with_skip() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "count": "users",
        "$db": "testdb",
        "skip": 2,
    };
    let result = count(&body, &fixture.service()).unwrap();
    assert_eq!(result.get_i64("n").unwrap(), 1);
}

#[test]
fn count_with_skip_and_limit() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "count": "users",
        "$db": "testdb",
        "skip": 1,
        "limit": 1,
    };
    let result = count(&body, &fixture.service()).unwrap();
    assert_eq!(result.get_i64("n").unwrap(), 1);
}

#[test]
fn count_empty_collection() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "empty",
        "$db": "testdb",
        "documents": [{ "_id": "tmp" }],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();
    let del_body = bson::doc! {
        "delete": "empty",
        "$db": "testdb",
        "deletes": [{ "q": {}, "limit": 0 }],
    };
    delete(&del_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! { "count": "empty", "$db": "testdb" };
    let result = count(&body, &fixture.service()).unwrap();
    assert_eq!(result.get_i64("n").unwrap(), 0);
}

#[test]
fn count_no_match_returns_zero() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "count": "users",
        "$db": "testdb",
        "query": { "name": "Nonexistent" },
    };
    let result = count(&body, &fixture.service()).unwrap();
    assert_eq!(result.get_i64("n").unwrap(), 0);
}

#[test]
fn count_missing_collection_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! { "$db": "testdb" };
    let err = count(&body, &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn distinct_returns_unique_values() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "distinct": "users",
        "$db": "testdb",
        "key": "name",
    };
    let result = distinct(&body, &fixture.service()).unwrap();
    assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 3);
}

#[test]
fn distinct_with_duplicates() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "scores",
        "$db": "testdb",
        "documents": [
            { "_id": "s1", "grade": "A" },
            { "_id": "s2", "grade": "B" },
            { "_id": "s3", "grade": "A" },
            { "_id": "s4", "grade": "C" },
            { "_id": "s5", "grade": "B" },
        ],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "distinct": "scores",
        "$db": "testdb",
        "key": "grade",
    };
    let result = distinct(&body, &fixture.service()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 3);
}

#[test]
fn distinct_with_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "distinct": "users",
        "$db": "testdb",
        "key": "name",
        "query": { "age": { "$gte": 30 } },
    };
    let result = distinct(&body, &fixture.service()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 2);
}

#[test]
fn distinct_nested_field() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "people",
        "$db": "testdb",
        "documents": [
            { "_id": "p1", "address": { "city": "NYC" } },
            { "_id": "p2", "address": { "city": "LA" } },
            { "_id": "p3", "address": { "city": "NYC" } },
        ],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "distinct": "people",
        "$db": "testdb",
        "key": "address.city",
    };
    let result = distinct(&body, &fixture.service()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 2);
}

#[test]
fn distinct_missing_field_excluded() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "distinct": "users",
        "$db": "testdb",
        "key": "email",
    };
    let result = distinct(&body, &fixture.service()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 0);
}

#[test]
fn distinct_null_values() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "nulls",
        "$db": "testdb",
        "documents": [
            { "_id": "n1", "val": 1 },
            { "_id": "n2", "val": bson::Bson::Null },
            { "_id": "n3", "val": 2 },
            { "_id": "n4", "val": bson::Bson::Null },
        ],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "distinct": "nulls",
        "$db": "testdb",
        "key": "val",
    };
    let result = distinct(&body, &fixture.service()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 3);
}

#[test]
fn distinct_array_field_unwinds() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "tagged",
        "$db": "testdb",
        "documents": [
            { "_id": "t1", "tags": ["a", "b"] },
            { "_id": "t2", "tags": ["b", "c"] },
        ],
    };
    insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let body = bson::doc! {
        "distinct": "tagged",
        "$db": "testdb",
        "key": "tags",
    };
    let result = distinct(&body, &fixture.service()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 3);
}

#[test]
fn distinct_missing_key_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! { "distinct": "users", "$db": "testdb" };
    let err = distinct(&body, &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn distinct_missing_collection_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! { "key": "name", "$db": "testdb" };
    let err = distinct(&body, &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}
