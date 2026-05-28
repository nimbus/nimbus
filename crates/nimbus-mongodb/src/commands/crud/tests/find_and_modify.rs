use super::*;

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
