use std::sync::Arc;

use neovex_core::{
    AccessOperator, AccessPredicate, AccessRule, AccessValue, Error, FieldSchema, FieldType,
    IndexDefinition, OrderBy, OrderDirection, PrincipalClaimSource, Query, TableAccessPolicy,
    TableName, TableSchema,
};
use neovex_runtime::{InvocationAuth, RuntimeUserIdentity};
use serde_json::{Map, Value, json};

use super::super::execution::execute_query_result_cancellable_with_auth;
use super::super::host_bridge::{ConvexHostBridge, ConvexRuntimeResponseEnvelope};
use super::fixture::host_bridge_fixture;
use super::*;
use crate::adapters::convex::normalize_principal_context;

fn messages_table() -> TableName {
    TableName::new("messages").expect("table name should be valid")
}

fn owner_read_rule() -> AccessRule {
    AccessRule {
        require_authenticated: true,
        predicates: vec![AccessPredicate {
            left: AccessValue::DocumentField {
                field: "owner".to_string(),
            },
            op: AccessOperator::Eq,
            right: AccessValue::PrincipalClaim {
                principal: PrincipalClaimSource::Identity,
                claim: "subject".to_string(),
            },
        }],
    }
}

fn owner_create_rule() -> AccessRule {
    owner_read_rule()
}

fn read_only_policy() -> TableAccessPolicy {
    TableAccessPolicy {
        read: owner_read_rule(),
        ..TableAccessPolicy::default()
    }
}

fn schema_with_owner_policy(access_policy: TableAccessPolicy) -> TableSchema {
    TableSchema {
        table: messages_table(),
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
        indexes: vec![IndexDefinition {
            name: "by_owner".to_string(),
            field: "owner".to_string(),
        }],
        access_policy: Some(access_policy),
    }
}

fn auth_for_subject(subject: &str) -> InvocationAuth {
    InvocationAuth {
        identity: Some(RuntimeUserIdentity {
            token_identifier: format!("issuer|{subject}"),
            subject: subject.to_string(),
            issuer: "issuer".to_string(),
            name: None,
            given_name: None,
            family_name: None,
            nickname: None,
            preferred_username: None,
            profile_url: None,
            picture_url: None,
            email: None,
            email_verified: None,
            gender: None,
            birthday: None,
            timezone: None,
            language: None,
            phone_number: None,
            phone_number_verified: None,
            address: None,
            updated_at: None,
            custom_claims: Map::new(),
        }),
        verified_identity: None,
        throw_on_missing_identity: false,
    }
}

fn decode_runtime_result(value: Value) -> Result<Value, Error> {
    let envelope: ConvexRuntimeResponseEnvelope =
        serde_json::from_value(value).expect("runtime envelope should deserialize");
    envelope.into_core_result()
}

#[test]
fn convex_query_execution_matches_direct_engine_authorization_for_same_normalized_principal() {
    let (_tempdir, service, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    service
        .set_table_schema(&tenant_id, schema_with_owner_policy(read_only_policy()))
        .expect("schema should save");

    for (owner, body) in [("user-123", "Ada"), ("user-456", "Grace")] {
        service
            .insert_document(
                &tenant_id,
                table.clone(),
                Map::from_iter([
                    ("owner".to_string(), json!(owner)),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("fixture insert should succeed");
    }

    let auth = auth_for_subject("user-123");
    let principal = normalize_principal_context(Some(&auth));
    let query = Query {
        table: table.clone(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let direct_documents = service
        .query_documents_with_principal(&tenant_id, &query, &principal)
        .expect("direct query should succeed");
    let direct_json = Value::Array(
        direct_documents
            .into_iter()
            .map(|document| document.to_json())
            .collect(),
    );
    let convex_json = execute_query_result_cancellable_with_auth(
        service.as_ref(),
        &tenant_id,
        ConvexExecutableQuery::Query(query),
        Some(&auth),
        &mut || Ok(()),
    )
    .expect("convex execution should succeed");

    assert_eq!(convex_json, direct_json);
}

#[test]
fn runtime_host_bridge_query_and_insert_respect_engine_authorization() {
    let (_tempdir, service, tenant_id, _anonymous_bridge) = host_bridge_fixture();
    let table = messages_table();

    for (owner, body) in [("user-123", "Ada"), ("user-456", "Grace")] {
        service
            .insert_document(
                &tenant_id,
                table.clone(),
                Map::from_iter([
                    ("owner".to_string(), json!(owner)),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("fixture insert should succeed");
    }
    service
        .set_table_schema(
            &tenant_id,
            schema_with_owner_policy(TableAccessPolicy {
                read: owner_read_rule(),
                create: owner_create_rule(),
                ..TableAccessPolicy::default()
            }),
        )
        .expect("schema should save");

    let auth = auth_for_subject("user-123");
    let direct_json = Value::Array(
        service
            .query_documents_with_principal(
                &tenant_id,
                &Query {
                    table: table.clone(),
                    filters: Vec::new(),
                    order: Some(OrderBy {
                        field: "body".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                &normalize_principal_context(Some(&auth)),
            )
            .expect("direct query should succeed")
            .into_iter()
            .map(|document| document.to_json())
            .collect(),
    );
    let bridge = ConvexHostBridge::new(
        service.clone(),
        Arc::new(ConvexRegistry::empty()),
        tenant_id.clone(),
        Some(auth.clone()),
        normalize_principal_context(Some(&auth)),
        None,
    );

    let query_result = bridge
        .invoke_ctx_query(json!({
            "query": {
                "table": "messages",
                "filters": [],
                "order": {
                    "field": "body",
                    "direction": "asc"
                },
                "limit": null
            }
        }))
        .expect("runtime query should return an envelope");
    assert_eq!(
        decode_runtime_result(query_result).expect("query should be authorized"),
        direct_json
    );

    let sequence_before = service
        .latest_sequence(&tenant_id)
        .expect("latest sequence should load");
    let insert_result = bridge
        .invoke_ctx_mutation(json!({
            "mutation": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "owner": "user-999",
                    "body": "Blocked"
                }
            }
        }))
        .expect("runtime insert should return an envelope");
    assert!(matches!(
        decode_runtime_result(insert_result),
        Err(Error::PermissionDenied(_))
    ));
    assert_eq!(
        service
            .latest_sequence(&tenant_id)
            .expect("latest sequence should remain unchanged"),
        sequence_before
    );
}
