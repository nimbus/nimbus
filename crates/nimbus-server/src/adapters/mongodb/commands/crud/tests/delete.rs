use super::*;

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
