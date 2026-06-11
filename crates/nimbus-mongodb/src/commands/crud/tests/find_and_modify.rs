use super::*;

#[test]
fn find_and_modify_update_returns_old() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u1" },
        "update": { "$set": { "age": 99 } },
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.engine()).unwrap();
    assert_eq!(result.get_f64("ok").unwrap(), 1.0);
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_i32("age").unwrap(), 30);

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert_eq!(docs[0].get_i32("age").unwrap(), 99);
}

#[test]
fn find_and_modify_update_returns_new() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u1" },
        "update": { "$set": { "age": 99 } },
        "new": true,
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.engine()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_i32("age").unwrap(), 99);
}

#[test]
fn find_and_modify_return_new_reads_transaction_overlay() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);
    let mut conn = test_conn();
    let lsid = start_transaction(&mut conn, &fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "lsid": lsid_field(&lsid),
        "query": { "_id": "u1" },
        "update": { "$set": { "age": 77 } },
        "new": true,
    };
    let result = find_and_modify(&body, &mut conn, &fixture.engine()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(
        value.get_i32("age").unwrap(),
        77,
        "return-new must read from the staged transaction overlay"
    );

    let outside_before_commit = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert_eq!(
        outside_before_commit[0].get_i32("age").unwrap(),
        30,
        "outside reads must not see the staged transaction update"
    );

    commit_transaction(&mut conn, &fixture, &lsid);
    let outside_after_commit = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert_eq!(outside_after_commit[0].get_i32("age").unwrap(), 77);
}

#[test]
fn find_and_modify_remove() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u1" },
        "remove": true,
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.engine()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_str("name").unwrap(), "Alice");

    let docs = find_doc(&fixture, bson::doc! { "_id": "u1" });
    assert_eq!(docs.len(), 0);
}

#[test]
fn find_and_modify_no_match_returns_null() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "nonexistent" },
        "update": { "$set": { "x": 1 } },
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.engine()).unwrap();
    assert!(result.get("value").unwrap().as_null().is_some());
}

#[test]
fn find_and_modify_upsert() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u9" },
        "update": { "$set": { "name": "Upserted" } },
        "upsert": true,
        "new": true,
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.engine()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_str("name").unwrap(), "Upserted");

    let leo = result.get_document("lastErrorObject").unwrap();
    assert!(!leo.get_bool("updatedExisting").unwrap());
}

#[test]
fn find_and_modify_upsert_return_new_reads_transaction_overlay() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);
    let mut conn = test_conn();
    let lsid = start_transaction(&mut conn, &fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "lsid": lsid_field(&lsid),
        "query": { "_id": "u9" },
        "update": { "$set": { "name": "Upserted" } },
        "upsert": true,
        "new": true,
    };
    let result = find_and_modify(&body, &mut conn, &fixture.engine()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(
        value.get_str("name").unwrap(),
        "Upserted",
        "upsert return-new must include the staged created document"
    );

    let outside_before_commit = find_doc(&fixture, bson::doc! { "_id": "u9" });
    assert!(
        outside_before_commit.is_empty(),
        "outside reads must not see staged transaction upsert"
    );

    commit_transaction(&mut conn, &fixture, &lsid);
    let outside_after_commit = find_doc(&fixture, bson::doc! { "_id": "u9" });
    assert_eq!(outside_after_commit.len(), 1);
    assert_eq!(outside_after_commit[0].get_str("name").unwrap(), "Upserted");
}

#[test]
fn find_and_modify_with_fields_projection() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u1" },
        "update": { "$set": { "age": 99 } },
        "fields": { "name": 1 },
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.engine()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_str("name").unwrap(), "Alice");
    assert!(value.get("_id").is_some());
    assert!(value.get("age").is_none());
}

#[test]
fn find_and_modify_replacement() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "findAndModify": "users",
        "$db": "testdb",
        "query": { "_id": "u1" },
        "update": { "name": "Replaced", "score": 42 },
        "new": true,
    };
    let result = find_and_modify(&body, &mut test_conn(), &fixture.engine()).unwrap();
    let value = result.get_document("value").unwrap();
    assert_eq!(value.get_str("name").unwrap(), "Replaced");
    assert_eq!(value.get_i32("score").unwrap(), 42);
    assert!(value.get("age").is_none());
}

fn start_transaction(
    conn: &mut ConnectionState,
    fixture: &EngineFixture<Engine>,
) -> bson::Document {
    let session_body = bson::doc! { "startSession": 1, "$db": "admin" };
    let session =
        crate::commands::session::start_session(&session_body, conn).expect("session should start");
    let lsid = session
        .get_document("id")
        .expect("session response should contain id")
        .clone();
    let start_body = bson::doc! {
        "insert": "users",
        "$db": "testdb",
        "startTransaction": true,
        "lsid": lsid_field(&lsid),
        "documents": [],
    };
    crate::commands::session::handle_start_transaction(
        &start_body,
        conn,
        &fixture.engine(),
        &test_principal(),
    )
    .expect("transaction should start");
    lsid
}

fn commit_transaction(
    conn: &mut ConnectionState,
    fixture: &EngineFixture<Engine>,
    lsid: &bson::Document,
) {
    let commit_body = bson::doc! {
        "commitTransaction": 1,
        "$db": "admin",
        "lsid": lsid_field(lsid),
    };
    crate::commands::session::commit_transaction(
        &commit_body,
        conn,
        &fixture.engine(),
        &test_principal(),
    )
    .expect("transaction should commit");
}

fn lsid_field(lsid: &bson::Document) -> bson::Bson {
    bson::Bson::Document(lsid.clone())
}
