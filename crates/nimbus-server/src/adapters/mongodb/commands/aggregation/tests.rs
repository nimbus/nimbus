use super::super::super::connection::ConnectionState;
use super::*;
use crate::adapters::mongodb::commands::crud;
use nimbus_testing::ServiceFixture;

fn test_conn() -> ConnectionState {
    ConnectionState::new(([127, 0, 0, 1], 12345).into())
}

fn seed_users(fixture: &ServiceFixture<Service>) {
    let body = bson::doc! {
        "insert": "users",
        "$db": "testdb",
        "documents": [
            { "_id": "u1", "name": "Alice", "age": 30, "dept": "eng" },
            { "_id": "u2", "name": "Bob", "age": 25, "dept": "eng" },
            { "_id": "u3", "name": "Charlie", "age": 35, "dept": "sales" },
        ],
    };
    crud::insert(&body, &mut test_conn(), &fixture.service()).unwrap();
}

fn agg_result(
    fixture: &ServiceFixture<Service>,
    collection: &str,
    pipeline: Vec<bson::Bson>,
) -> Vec<bson::Document> {
    let body = bson::doc! {
        "aggregate": collection,
        "$db": "testdb",
        "pipeline": pipeline,
        "cursor": {},
    };
    let result = aggregate(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    cursor
        .get_array("firstBatch")
        .unwrap()
        .iter()
        .filter_map(|b| b.as_document().cloned())
        .collect()
}

#[test]
fn aggregate_empty_pipeline() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(&fixture, "users", vec![]);
    assert_eq!(docs.len(), 3);
}

#[test]
fn aggregate_match_stage() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![bson::Bson::Document(
            bson::doc! { "$match": { "dept": "eng" } },
        )],
    );
    assert_eq!(docs.len(), 2);
}

#[test]
fn aggregate_match_with_comparison() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![bson::Bson::Document(
            bson::doc! { "$match": { "age": { "$gte": 30 } } },
        )],
    );
    assert_eq!(docs.len(), 2);
}

#[test]
fn aggregate_sort_stage() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![bson::Bson::Document(bson::doc! { "$sort": { "age": 1 } })],
    );
    assert_eq!(docs[0].get_str("name").unwrap(), "Bob");
    assert_eq!(docs[2].get_str("name").unwrap(), "Charlie");
}

#[test]
fn aggregate_sort_descending() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![bson::Bson::Document(bson::doc! { "$sort": { "age": -1 } })],
    );
    assert_eq!(docs[0].get_str("name").unwrap(), "Charlie");
    assert_eq!(docs[2].get_str("name").unwrap(), "Bob");
}

#[test]
fn aggregate_limit_stage() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![bson::Bson::Document(bson::doc! { "$limit": 2 })],
    );
    assert_eq!(docs.len(), 2);
}

#[test]
fn aggregate_skip_stage() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![bson::Bson::Document(bson::doc! { "$skip": 2 })],
    );
    assert_eq!(docs.len(), 1);
}

#[test]
fn aggregate_match_sort_limit_chain() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![
            bson::Bson::Document(bson::doc! { "$match": { "dept": "eng" } }),
            bson::Bson::Document(bson::doc! { "$sort": { "age": -1 } }),
            bson::Bson::Document(bson::doc! { "$limit": 1 }),
        ],
    );
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].get_str("name").unwrap(), "Alice");
}

#[test]
fn aggregate_project_inclusion() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![
            bson::Bson::Document(bson::doc! { "$match": { "_id": "u1" } }),
            bson::Bson::Document(bson::doc! { "$project": { "name": 1 } }),
        ],
    );
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].get_str("name").unwrap(), "Alice");
    assert!(docs[0].get("age").is_none());
}

#[test]
fn aggregate_add_fields() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![
            bson::Bson::Document(bson::doc! { "$match": { "_id": "u1" } }),
            bson::Bson::Document(bson::doc! { "$addFields": { "status": "active" } }),
        ],
    );
    assert_eq!(docs[0].get_str("status").unwrap(), "active");
    assert_eq!(docs[0].get_str("name").unwrap(), "Alice");
}

#[test]
fn aggregate_count_stage() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![
            bson::Bson::Document(bson::doc! { "$match": { "dept": "eng" } }),
            bson::Bson::Document(bson::doc! { "$count": "total" }),
        ],
    );
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].get_i64("total").unwrap(), 2);
}

#[test]
fn aggregate_group_sum() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![bson::Bson::Document(bson::doc! {
            "$group": {
                "_id": "$dept",
                "totalAge": { "$sum": "$age" },
            }
        })],
    );
    assert_eq!(docs.len(), 2);

    let eng = docs
        .iter()
        .find(|d| d.get("_id") == Some(&bson::Bson::String("eng".into())));
    assert!(eng.is_some());
    assert_eq!(eng.unwrap().get_i64("totalAge").unwrap(), 55);
}

#[test]
fn aggregate_group_avg() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![bson::Bson::Document(bson::doc! {
            "$group": {
                "_id": "$dept",
                "avgAge": { "$avg": "$age" },
            }
        })],
    );

    let eng = docs
        .iter()
        .find(|d| d.get("_id") == Some(&bson::Bson::String("eng".into())))
        .unwrap();
    let avg = eng.get_f64("avgAge").unwrap();
    assert!((avg - 27.5).abs() < 0.01);
}

#[test]
fn aggregate_group_min_max() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![bson::Bson::Document(bson::doc! {
            "$group": {
                "_id": bson::Bson::Null,
                "minAge": { "$min": "$age" },
                "maxAge": { "$max": "$age" },
            }
        })],
    );
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].get_f64("minAge").unwrap(), 25.0);
    assert_eq!(docs[0].get_f64("maxAge").unwrap(), 35.0);
}

#[test]
fn aggregate_group_first_last() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![
            bson::Bson::Document(bson::doc! { "$sort": { "age": 1 } }),
            bson::Bson::Document(bson::doc! {
                "$group": {
                    "_id": bson::Bson::Null,
                    "youngest": { "$first": "$name" },
                    "oldest": { "$last": "$name" },
                }
            }),
        ],
    );
    assert_eq!(docs[0].get_str("youngest").unwrap(), "Bob");
    assert_eq!(docs[0].get_str("oldest").unwrap(), "Charlie");
}

#[test]
fn aggregate_group_push() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![bson::Bson::Document(bson::doc! {
            "$group": {
                "_id": "$dept",
                "names": { "$push": "$name" },
            }
        })],
    );

    let eng = docs
        .iter()
        .find(|d| d.get("_id") == Some(&bson::Bson::String("eng".into())))
        .unwrap();
    let names = eng.get_array("names").unwrap();
    assert_eq!(names.len(), 2);
}

#[test]
fn aggregate_group_add_to_set() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let docs = agg_result(
        &fixture,
        "users",
        vec![bson::Bson::Document(bson::doc! {
            "$group": {
                "_id": bson::Bson::Null,
                "depts": { "$addToSet": "$dept" },
            }
        })],
    );
    let depts = docs[0].get_array("depts").unwrap();
    assert_eq!(depts.len(), 2);
}

#[test]
fn aggregate_unwind() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "tagged",
        "$db": "testdb",
        "documents": [
            { "_id": "t1", "tags": ["a", "b"] },
            { "_id": "t2", "tags": ["c"] },
        ],
    };
    crud::insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let docs = agg_result(
        &fixture,
        "tagged",
        vec![bson::Bson::Document(bson::doc! { "$unwind": "$tags" })],
    );
    assert_eq!(docs.len(), 3);
}

#[test]
fn aggregate_unwind_preserve_null() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "mixed",
        "$db": "testdb",
        "documents": [
            { "_id": "m1", "tags": ["a"] },
            { "_id": "m2", "val": 1 },
        ],
    };
    crud::insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let docs = agg_result(
        &fixture,
        "mixed",
        vec![bson::Bson::Document(bson::doc! {
            "$unwind": {
                "path": "$tags",
                "preserveNullAndEmptyArrays": true,
            }
        })],
    );
    assert_eq!(docs.len(), 2);
}

#[test]
fn aggregate_unwind_include_array_index() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let insert_body = bson::doc! {
        "insert": "indexed",
        "$db": "testdb",
        "documents": [{ "_id": "x1", "items": ["a", "b", "c"] }],
    };
    crud::insert(&insert_body, &mut test_conn(), &fixture.service()).unwrap();

    let docs = agg_result(
        &fixture,
        "indexed",
        vec![bson::Bson::Document(bson::doc! {
            "$unwind": {
                "path": "$items",
                "includeArrayIndex": "idx",
            }
        })],
    );
    assert_eq!(docs.len(), 3);
    assert_eq!(docs[0].get_i64("idx").unwrap(), 0);
    assert_eq!(docs[2].get_i64("idx").unwrap(), 2);
}

#[test]
fn aggregate_unsupported_stage_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "aggregate": "users",
        "$db": "testdb",
        "pipeline": [{ "$lookup": {} }],
        "cursor": {},
    };
    let err = aggregate(&body, &mut test_conn(), &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { message, .. } => assert!(message.contains("$lookup")),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn aggregate_missing_pipeline_returns_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let body = bson::doc! { "aggregate": "users", "$db": "testdb" };
    let err = aggregate(&body, &mut test_conn(), &fixture.service()).unwrap_err();
    match err {
        MongoError::Command { code, .. } => assert_eq!(code, BAD_VALUE.code),
        other => panic!("expected Command, got {:?}", other),
    }
}

#[test]
fn aggregate_with_cursor_batch_size() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    seed_users(&fixture);

    let body = bson::doc! {
        "aggregate": "users",
        "$db": "testdb",
        "pipeline": [],
        "cursor": { "batchSize": 1 },
    };
    let result = aggregate(&body, &mut test_conn(), &fixture.service()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 1);
    assert!(cursor.get_i64("id").unwrap() > 0);
}
