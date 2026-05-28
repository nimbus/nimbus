use super::*;

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
