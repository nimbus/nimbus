use super::*;

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
