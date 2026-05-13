use super::super::super::connection::ConnectionState;
use super::*;
use nimbus_testing::ServiceFixture;

fn test_conn() -> ConnectionState {
    ConnectionState::new(([127, 0, 0, 1], 12345).into())
}

mod count;
mod delete;
mod distinct;
mod find;
mod find_and_modify;
mod insert;
mod update;

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
