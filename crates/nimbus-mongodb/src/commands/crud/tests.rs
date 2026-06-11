use super::super::super::connection::ConnectionState;
use std::sync::Arc;

use crate::error::{BAD_VALUE, MongoError, UNAUTHORIZED};
use nimbus_core::{
    AccessOperator, AccessPredicate, AccessRule, AccessValue, FieldSchema, FieldType,
    PrincipalClaimSource, PrincipalContext, TableAccessPolicy, TableName, TableSchema, TenantId,
};
use nimbus_engine::Engine;
use nimbus_testing::EngineFixture;
use serde_json::json;

fn test_conn() -> ConnectionState {
    ConnectionState::new(([127, 0, 0, 1], 12345).into())
}

fn test_principal() -> PrincipalContext {
    PrincipalContext::system()
}

fn principal_with_subject(subject: &str) -> PrincipalContext {
    PrincipalContext {
        authenticated: true,
        claims: serde_json::Map::from_iter([
            ("subject".to_string(), json!(subject)),
            ("sub".to_string(), json!(subject)),
        ]),
        verified_claims: serde_json::Map::new(),
    }
}

fn owner_matches_subject_rule(left: AccessValue) -> AccessRule {
    AccessRule {
        require_authenticated: true,
        predicates: vec![AccessPredicate {
            left,
            op: AccessOperator::Eq,
            right: AccessValue::PrincipalClaim {
                principal: PrincipalClaimSource::Identity,
                claim: "sub".to_string(),
            },
        }],
    }
}

fn insert(
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
) -> Result<bson::Document, MongoError> {
    super::insert(body, conn, engine, &test_principal())
}

fn find(
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
) -> Result<bson::Document, MongoError> {
    super::find(body, conn, engine, &test_principal())
}

fn update(
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
) -> Result<bson::Document, MongoError> {
    super::update(body, conn, engine, &test_principal())
}

fn delete(
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
) -> Result<bson::Document, MongoError> {
    super::delete(body, conn, engine, &test_principal())
}

fn find_and_modify(
    body: &bson::Document,
    conn: &mut ConnectionState,
    engine: &Arc<Engine>,
) -> Result<bson::Document, MongoError> {
    super::find_and_modify(body, conn, engine, &test_principal())
}

fn count(body: &bson::Document, engine: &Arc<Engine>) -> Result<bson::Document, MongoError> {
    super::count(body, engine, &test_principal())
}

fn distinct(body: &bson::Document, engine: &Arc<Engine>) -> Result<bson::Document, MongoError> {
    super::distinct(body, engine, &test_principal())
}

mod count;
mod delete;
mod distinct;
mod find;
mod find_and_modify;
mod insert;
mod update;

fn seed_users(fixture: &EngineFixture<Engine>) {
    let body = bson::doc! {
        "insert": "users",
        "$db": "testdb",
        "documents": [
            { "_id": "u1", "name": "Alice", "age": 30 },
            { "_id": "u2", "name": "Bob", "age": 25 },
            { "_id": "u3", "name": "Charlie", "age": 35 },
        ],
    };
    insert(&body, &mut test_conn(), &fixture.engine()).unwrap();
}

fn find_doc(fixture: &EngineFixture<Engine>, filter: bson::Document) -> Vec<bson::Document> {
    find_in(fixture, "users", filter)
}

fn find_in(
    fixture: &EngineFixture<Engine>,
    collection: &str,
    filter: bson::Document,
) -> Vec<bson::Document> {
    let body = bson::doc! {
        "find": collection,
        "$db": "testdb",
        "filter": filter,
    };
    let result = find(&body, &mut test_conn(), &fixture.engine()).unwrap();
    let cursor = result.get_document("cursor").unwrap();
    cursor
        .get_array("firstBatch")
        .unwrap()
        .iter()
        .filter_map(|b| b.as_document().cloned())
        .collect()
}

#[test]
fn command_principal_enforces_owner_access_policy() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let tenant_id = TenantId::new("testdb").unwrap();
    fixture.engine().create_tenant(tenant_id.clone()).unwrap();
    fixture
        .engine()
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: TableName::new("owned").unwrap(),
                fields: vec![
                    FieldSchema {
                        name: "owner".to_string(),
                        field_type: FieldType::String,
                        required: true,
                    },
                    FieldSchema {
                        name: "body".to_string(),
                        field_type: FieldType::String,
                        required: true,
                    },
                ],
                indexes: vec![],
                access_policy: Some(TableAccessPolicy {
                    read: owner_matches_subject_rule(AccessValue::DocumentField {
                        field: "owner".to_string(),
                    }),
                    create: owner_matches_subject_rule(AccessValue::DocumentField {
                        field: "owner".to_string(),
                    }),
                    ..TableAccessPolicy::default()
                }),
            },
        )
        .unwrap();
    let alice = principal_with_subject("alice");

    let allowed = bson::doc! {
        "insert": "owned",
        "$db": "testdb",
        "documents": [{ "_id": "alice-doc", "owner": "alice", "body": "visible" }],
    };
    let result = super::insert(&allowed, &mut test_conn(), &fixture.engine(), &alice).unwrap();
    assert_eq!(result.get_i32("n").unwrap(), 1);

    let denied = bson::doc! {
        "insert": "owned",
        "$db": "testdb",
        "documents": [{ "_id": "bob-doc", "owner": "bob", "body": "hidden" }],
    };
    let denied_result =
        super::insert(&denied, &mut test_conn(), &fixture.engine(), &alice).unwrap();
    assert_eq!(denied_result.get_i32("n").unwrap(), 0);
    let write_errors = denied_result.get_array("writeErrors").unwrap();
    assert_eq!(write_errors.len(), 1);
    let write_error = write_errors[0].as_document().unwrap();
    assert_eq!(write_error.get_i32("code").unwrap(), UNAUTHORIZED.code);
    assert!(
        write_error
            .get_str("errmsg")
            .unwrap()
            .contains("create access denied")
    );

    let find_body = bson::doc! {
        "find": "owned",
        "$db": "testdb",
        "filter": {},
    };
    let result = super::find(&find_body, &mut test_conn(), &fixture.engine(), &alice).unwrap();
    let batch = result
        .get_document("cursor")
        .unwrap()
        .get_array("firstBatch")
        .unwrap();
    assert_eq!(batch.len(), 1);
    let doc = batch[0].as_document().unwrap();
    assert_eq!(doc.get_str("owner").unwrap(), "alice");
}
