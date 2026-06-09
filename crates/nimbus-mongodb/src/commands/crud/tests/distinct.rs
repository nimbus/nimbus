use super::*;

#[test]
fn distinct_returns_unique_values() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "distinct": "users",
        "$db": "testdb",
        "key": "name",
    };
    let result = distinct(&body, &fixture.engine()).unwrap();
    assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 3);
}

#[test]
fn distinct_with_duplicates() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
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
    insert(&insert_body, &mut test_conn(), &fixture.engine()).unwrap();

    let body = bson::doc! {
        "distinct": "scores",
        "$db": "testdb",
        "key": "grade",
    };
    let result = distinct(&body, &fixture.engine()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 3);
}

#[test]
fn distinct_with_filter() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "distinct": "users",
        "$db": "testdb",
        "key": "name",
        "query": { "age": { "$gte": 30 } },
    };
    let result = distinct(&body, &fixture.engine()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 2);
}

#[test]
fn distinct_nested_field() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let insert_body = bson::doc! {
        "insert": "people",
        "$db": "testdb",
        "documents": [
            { "_id": "p1", "address": { "city": "NYC" } },
            { "_id": "p2", "address": { "city": "LA" } },
            { "_id": "p3", "address": { "city": "NYC" } },
        ],
    };
    insert(&insert_body, &mut test_conn(), &fixture.engine()).unwrap();

    let body = bson::doc! {
        "distinct": "people",
        "$db": "testdb",
        "key": "address.city",
    };
    let result = distinct(&body, &fixture.engine()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 2);
}

#[test]
fn distinct_missing_field_excluded() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "distinct": "users",
        "$db": "testdb",
        "key": "email",
    };
    let result = distinct(&body, &fixture.engine()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 0);
}

#[test]
fn distinct_null_values() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
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
    insert(&insert_body, &mut test_conn(), &fixture.engine()).unwrap();

    let body = bson::doc! {
        "distinct": "nulls",
        "$db": "testdb",
        "key": "val",
    };
    let result = distinct(&body, &fixture.engine()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 3);
}

#[test]
fn distinct_array_field_unwinds() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let insert_body = bson::doc! {
        "insert": "tagged",
        "$db": "testdb",
        "documents": [
            { "_id": "t1", "tags": ["a", "b"] },
            { "_id": "t2", "tags": ["b", "c"] },
        ],
    };
    insert(&insert_body, &mut test_conn(), &fixture.engine()).unwrap();

    let body = bson::doc! {
        "distinct": "tagged",
        "$db": "testdb",
        "key": "tags",
    };
    let result = distinct(&body, &fixture.engine()).unwrap();
    let values = result.get_array("values").unwrap();
    assert_eq!(values.len(), 3);
}

#[test]
fn distinct_missing_key_returns_error() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let body = bson::doc! { "distinct": "users", "$db": "testdb" };
    let err = distinct(&body, &fixture.engine()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn distinct_missing_collection_returns_error() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let body = bson::doc! { "key": "name", "$db": "testdb" };
    let err = distinct(&body, &fixture.engine()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}
