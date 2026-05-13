use super::*;

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
