use super::*;

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
