use std::fs;
use std::sync::Arc;

use nimbus_core::{
    AccessOperator, AccessPredicate, AccessRule, AccessValue, DocumentId, Error, FieldSchema,
    FieldType, IndexDefinition, OrderBy, OrderDirection, PrincipalClaimSource, Query,
    TableAccessPolicy, TableName, TableSchema,
};
use nimbus_runtime::{InvocationAuth, InvocationKind, NimbusRuntimeError, RuntimeUserIdentity};
use serde_json::{Map, Value, json};

use super::super::execution::execute_query_result_cancellable_with_auth;
use super::super::host_bridge::{ConvexHostBridge, ConvexRuntimeResponseEnvelope};
use super::fixture::host_bridge_fixture;
use super::*;
use crate::application_auth::normalize_principal_context;
use crate::service_registry::SandboxCatalogRuntimeServiceRegistry;

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
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Enabled,
            name: "by_owner".to_string(),
            fields: vec!["owner".to_string()],
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

fn convex_document_id(table: &TableName, document_id: &DocumentId) -> DocumentId {
    encode_convex_document_id(table, document_id).expect("Convex document id should encode")
}

fn raw_document_id_from_convex_value(table: &TableName, value: &Value) -> DocumentId {
    let convex_id = value
        .as_str()
        .expect("value should contain a Convex document id")
        .parse::<DocumentId>()
        .expect("Convex document id should parse as a document key");
    resolve_convex_document_id(table, convex_id)
        .expect("Convex document id should resolve for the expected table")
        .into_document_id()
}

fn assert_unknown_tenant_payload_rejected(error: NimbusRuntimeError) {
    assert!(
        matches!(error, NimbusRuntimeError::Json(_)),
        "tenant payload smuggling should fail during host-call payload decoding: {error}"
    );
    let message = error.to_string();
    assert!(
        message.contains("unknown field `tenant_id`"),
        "tenant payload smuggling error should name the rejected field: {message}"
    );
}

#[test]
fn host_bridge_service_lookup_rejects_service_missing_from_decision_grants() {
    let (_tempdir, _service, _tenant_id, bridge) = host_bridge_fixture();

    let value = bridge
        .invoke_ctx_service_lookup(json!({
            "service_name": "db",
            "session_id": bridge.session_id(),
        }))
        .expect("service lookup should return a runtime envelope");
    let error = decode_runtime_result(value).expect_err(
        "HostBridge service lookup must reject a service missing from the decision grant set",
    );

    assert!(
        matches!(error, Error::PermissionDenied(_)),
        "missing service decision grant should be permission denied: {error}"
    );
    assert!(
        error.to_string().contains("did not authorize service `db`"),
        "error should name the rejected service: {error}"
    );
}

fn mutation_bridge(
    service: Arc<Service>,
    registry: Arc<ConvexRegistry>,
    tenant_id: TenantId,
    principal: nimbus_core::PrincipalContext,
) -> ConvexHostBridge {
    let isolation = crate::tenant::TenantIsolationContext::application(
        tenant_id,
        principal.clone(),
        "convex_authorization_test",
    );
    let decision = crate::tenant::admit_runtime_invocation_decision(
        &isolation,
        "convex_authorization_test",
        None,
        &registry.runtime_policy(),
        crate::tenant::RuntimeIsolationTier::InProcessUntrusted,
        crate::tenant::TenantIsolationMode::LocalDevelopment,
        std::iter::empty::<String>(),
    )
    .expect("authorization test tenant isolation decision should build");
    ConvexHostBridge::build(
        ConvexHostBridgeScope::new(
            service,
            registry,
            decision,
            Arc::new(SandboxCatalogRuntimeServiceRegistry::new(Arc::new(
                crate::EmptySandboxCatalog,
            ))),
        ),
        ConvexHostBridgeInvocation::new(
            None,
            Default::default(),
            principal,
            None,
            InvocationKind::Mutation,
        ),
    )
    .expect("mutation bridge should build")
}

fn registry_with_scheduled_mutation() -> Arc<ConvexRegistry> {
    let tempdir = tempfile::tempdir().expect("convex registry tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [
                {
                    "name": "messages:sendInternal",
                    "kind": "mutation",
                    "visibility": "internal",
                    "schedulable": true,
                    "plan": {
                        "type": "insert",
                        "table": "messages",
                        "fields": {
                            "owner": "system",
                            "body": { "$arg": "body" }
                        }
                    }
                }
            ]
        }))
        .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route manifest should serialize"),
    )
    .expect("convex http route manifest should write");
    let registry =
        ConvexRegistry::from_app_dir(tempdir.path()).expect("convex registry should load");
    std::mem::forget(tempdir);
    Arc::new(registry)
}

#[test]
fn runtime_host_bridge_rejects_payload_tenant_for_db_insert() {
    let (_tempdir, service, tenant_id, _bridge) = host_bridge_fixture();
    let tenant_b = TenantId::new("tenant-b").expect("tenant-b id should be valid");
    service
        .create_tenant(tenant_b.clone())
        .expect("tenant-b should be created");
    let table = messages_table();
    let bridge = mutation_bridge(
        service.clone(),
        Arc::new(ConvexRegistry::empty()),
        tenant_id.clone(),
        nimbus_core::PrincipalContext::anonymous(),
    );

    let error = bridge
        .invoke_ctx_db_insert(json!({
            "tenant_id": "tenant-b",
            "table": table,
            "fields": {
                "owner": "user-123",
                "body": "tenant payload smuggling"
            }
        }))
        .expect_err("host-call db insert payload must reject tenant_id");
    assert_unknown_tenant_payload_rejected(error);

    for target_tenant in [&tenant_id, &tenant_b] {
        let documents = service
            .query_documents(
                target_tenant,
                &Query {
                    table: table.clone(),
                    filters: Vec::new(),
                    order: None,
                    limit: None,
                },
            )
            .expect("tenant table query should succeed");
        assert!(
            documents.is_empty(),
            "rejected host-call payload should not write to tenant {target_tenant}"
        );
    }
}

#[test]
fn runtime_host_bridge_rejects_payload_tenant_for_scheduler() {
    let (_tempdir, service, tenant_id, _bridge) = host_bridge_fixture();
    let tenant_b = TenantId::new("tenant-b").expect("tenant-b id should be valid");
    service
        .create_tenant(tenant_b.clone())
        .expect("tenant-b should be created");
    let bridge = mutation_bridge(
        service.clone(),
        registry_with_scheduled_mutation(),
        tenant_id.clone(),
        nimbus_core::PrincipalContext::anonymous(),
    );

    let error = bridge
        .invoke_ctx_scheduler_run_after(json!({
            "tenant_id": "tenant-b",
            "delay_ms": 0,
            "name": "messages:sendInternal",
            "visibility": "internal",
            "args": {
                "body": "tenant payload smuggling"
            }
        }))
        .expect_err("host-call scheduler payload must reject tenant_id");
    assert_unknown_tenant_payload_rejected(error);

    assert!(
        service
            .list_scheduled_jobs(&tenant_id)
            .expect("tenant-a scheduled jobs should load")
            .is_empty()
    );
    assert!(
        service
            .list_scheduled_jobs(&tenant_b)
            .expect("tenant-b scheduled jobs should load")
            .is_empty()
    );
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

    let direct_json = documents_to_convex_json(
        service
            .query_documents_with_principal(&tenant_id, &query, &principal)
            .expect("direct query should succeed"),
    )
    .expect("direct documents should encode as Convex JSON");
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
    let direct_json = documents_to_convex_json(
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
            .expect("direct query should succeed"),
    )
    .expect("direct documents should encode as Convex JSON");
    let registry = Arc::new(ConvexRegistry::empty());
    let isolation = crate::tenant::TenantIsolationContext::application(
        tenant_id.clone(),
        normalize_principal_context(Some(&auth)),
        "convex_authorization_test",
    );
    let decision = crate::tenant::admit_runtime_invocation_decision(
        &isolation,
        "convex_authorization_test",
        None,
        &registry.runtime_policy(),
        crate::tenant::RuntimeIsolationTier::InProcessUntrusted,
        crate::tenant::TenantIsolationMode::LocalDevelopment,
        std::iter::empty::<String>(),
    )
    .expect("authorization query tenant isolation decision should build");
    let bridge = ConvexHostBridge::new(
        ConvexHostBridgeScope::new(
            service.clone(),
            registry,
            decision,
            Arc::new(SandboxCatalogRuntimeServiceRegistry::new(Arc::new(
                crate::EmptySandboxCatalog,
            ))),
        ),
        ConvexHostBridgeInvocation::new(
            Some(auth.clone()),
            Default::default(),
            normalize_principal_context(Some(&auth)),
            None,
            InvocationKind::Query,
        ),
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

#[test]
fn runtime_mutation_bridge_stages_writes_until_commit_and_reads_its_own_writes() {
    let (_tempdir, service, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    let bridge = mutation_bridge(
        service.clone(),
        Arc::new(ConvexRegistry::empty()),
        tenant_id.clone(),
        nimbus_core::PrincipalContext::anonymous(),
    );

    let inserted = decode_runtime_result(
        bridge
            .invoke_ctx_db_insert(json!({
                "table": table,
                "fields": {
                    "owner": "user-123",
                    "body": "Hello from tx"
                }
            }))
            .expect("staged insert should encode"),
    )
    .expect("staged insert should succeed");
    let convex_id = inserted
        .as_str()
        .expect("insert should return a Convex document id")
        .parse::<DocumentId>()
        .expect("Convex document id should parse");
    let document_id = raw_document_id_from_convex_value(&table, &inserted);

    let read_back = decode_runtime_result(
        bridge
            .invoke_ctx_db_get(json!({
                "table": table,
                "id": convex_id
            }))
            .expect("staged get should encode"),
    )
    .expect("staged get should succeed");
    assert_eq!(read_back["body"], json!("Hello from tx"));
    assert_eq!(read_back["_id"], json!(convex_id.to_string()));
    let dependencies = bridge.snapshot_read_set().dependency_set();
    assert!(
        dependencies.missing_tables.contains(&table),
        "staged-only table reads should record a table-creation dependency until a durable TableId exists"
    );
    assert!(
        !dependencies
            .documents
            .iter()
            .any(|dependency| dependency.table == table && dependency.document_id == convex_id),
        "Convex read tracking must not record the protocol-scoped id"
    );
    assert!(matches!(
        service.get_document(&tenant_id, &table, document_id.clone()),
        Err(Error::DocumentNotFound(_))
    ));

    bridge
        .commit_mutation_execution_unit()
        .expect("commit should persist staged writes");
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("committed document should exist")
            .get_field("body"),
        Some(&json!("Hello from tx"))
    );
}

#[test]
fn runtime_mutation_bridge_reads_own_writes_even_when_materialized_serving_snapshot_is_warmed() {
    let (_tempdir, service, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    service
        .insert_document(
            &tenant_id,
            table.clone(),
            Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Already committed")),
            ]),
        )
        .expect("fixture insert should succeed");
    let warmed = service
        .query_documents(
            &tenant_id,
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
        )
        .expect("warm query should succeed");
    assert_eq!(warmed.len(), 1);
    let surface_stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(surface_stats.loaded_table_count, 1);

    let bridge = mutation_bridge(
        service.clone(),
        Arc::new(ConvexRegistry::empty()),
        tenant_id.clone(),
        nimbus_core::PrincipalContext::anonymous(),
    );
    let inserted = decode_runtime_result(
        bridge
            .invoke_ctx_db_insert(json!({
                "table": table,
                "fields": {
                    "owner": "user-123",
                    "body": "Hello from staged tx"
                }
            }))
            .expect("staged insert should encode"),
    )
    .expect("staged insert should succeed");
    let convex_id = inserted
        .as_str()
        .expect("insert should return a Convex document id")
        .parse::<DocumentId>()
        .expect("Convex document id should parse");
    let document_id = raw_document_id_from_convex_value(&table, &inserted);

    let read_back = decode_runtime_result(
        bridge
            .invoke_ctx_db_get(json!({
                "table": table,
                "id": convex_id
            }))
            .expect("staged get should encode"),
    )
    .expect("staged get should succeed");
    assert_eq!(read_back["body"], json!("Hello from staged tx"));
    assert!(matches!(
        service.get_document(&tenant_id, &table, document_id),
        Err(Error::DocumentNotFound(_))
    ));
}

#[test]
fn runtime_host_bridge_rejects_wrong_table_convex_document_ids() {
    let (_tempdir, service, tenant_id, _bridge) = host_bridge_fixture();
    let messages = messages_table();
    let users = TableName::new("users").expect("users table should be valid");
    let bridge = mutation_bridge(
        service,
        Arc::new(ConvexRegistry::empty()),
        tenant_id,
        nimbus_core::PrincipalContext::anonymous(),
    );

    let inserted = decode_runtime_result(
        bridge
            .invoke_ctx_db_insert(json!({
                "table": messages,
                "fields": {
                    "owner": "user-123",
                    "body": "Wrong table probe"
                }
            }))
            .expect("insert should encode"),
    )
    .expect("insert should succeed");
    let convex_id = inserted
        .as_str()
        .expect("insert should return a Convex document id");
    assert!(
        convex_id.starts_with("messages:"),
        "Convex ids should carry their developer table: {convex_id}"
    );

    for (operation, value) in [
        (
            "get",
            bridge
                .invoke_ctx_db_get(json!({
                    "table": users,
                    "id": convex_id,
                }))
                .expect("wrong-table get should encode"),
        ),
        (
            "patch",
            bridge
                .invoke_ctx_db_patch(json!({
                    "table": users,
                    "id": convex_id,
                    "patch": {
                        "body": "should not apply"
                    }
                }))
                .expect("wrong-table patch should encode"),
        ),
        (
            "delete",
            bridge
                .invoke_ctx_db_delete(json!({
                    "table": users,
                    "id": convex_id,
                }))
                .expect("wrong-table delete should encode"),
        ),
    ] {
        let error = decode_runtime_result(value)
            .expect_err("wrong-table Convex document id should be rejected");
        assert!(
            matches!(error, Error::InvalidInput(_)),
            "wrong-table {operation} should be InvalidInput: {error}"
        );
        assert!(
            error.to_string().contains("belongs to table messages"),
            "wrong-table {operation} should name the encoded table: {error}"
        );
    }
}

#[test]
fn convex_read_get_round_trips_custom_table_scoped_ids() {
    let (_tempdir, service, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    let raw_id = DocumentId::from_key("custom:id".to_string())
        .expect("custom id with colon should be a valid storage id");
    service
        .insert_document_with_id(
            &tenant_id,
            table.clone(),
            raw_id.clone(),
            Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Custom identity")),
            ]),
        )
        .expect("custom id insert should succeed");
    let convex_id = convex_document_id(&table, &raw_id);

    let value = execute_query_result_cancellable_with_auth(
        service.as_ref(),
        &tenant_id,
        ConvexExecutableQuery::Read(ConvexReadCommand::Get {
            table: table.clone(),
            id: convex_id.clone(),
        }),
        None,
        &mut || Ok(()),
    )
    .expect("Convex get should read a custom id");

    assert_eq!(value["_id"], json!(convex_id.to_string()));
    assert_eq!(value["body"], json!("Custom identity"));
    assert_eq!(
        raw_document_id_from_convex_value(&table, &value["_id"]),
        raw_id
    );
}

#[test]
fn runtime_mutation_bridge_commit_detects_occ_conflicts() {
    let (_tempdir, service, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    let document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("fixture insert should succeed");
    let bridge = mutation_bridge(
        service.clone(),
        Arc::new(ConvexRegistry::empty()),
        tenant_id.clone(),
        nimbus_core::PrincipalContext::anonymous(),
    );
    let convex_id = convex_document_id(&table, &document_id);

    let _ = decode_runtime_result(
        bridge
            .invoke_ctx_db_get(json!({
                "table": table,
                "id": convex_id
            }))
            .expect("point read should encode"),
    )
    .expect("point read should succeed");
    let committed_table_id = service
        .table_id(&tenant_id, &table)
        .expect("table id lookup should succeed")
        .expect("committed document should have a table id");
    let dependencies = bridge.snapshot_read_set().dependency_set();
    assert!(
        dependencies.documents.iter().any(|dependency| {
            dependency.table == table
                && dependency.table_id == committed_table_id
                && dependency.document_id == document_id
        }),
        "Convex read tracking should record the raw storage document id under the stable TableId"
    );
    assert!(
        !dependencies.documents.iter().any(|dependency| {
            dependency.table == table
                && dependency.table_id == committed_table_id
                && dependency.document_id == convex_id
        }),
        "Convex read tracking must not record the protocol-scoped id"
    );
    let _ = decode_runtime_result(
        bridge
            .invoke_ctx_db_patch(json!({
                "table": table,
                "id": convex_id,
                "patch": {
                    "body": "Bridge update"
                }
            }))
            .expect("staged patch should encode"),
    )
    .expect("staged patch should succeed");

    service
        .update_document(
            &tenant_id,
            table.clone(),
            document_id.clone(),
            Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("outside update should commit");

    let error = bridge
        .commit_mutation_execution_unit()
        .expect_err("commit should detect the conflict");
    assert!(matches!(error, Error::Conflict(_)));
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("document should remain committed")
            .get_field("body"),
        Some(&json!("Outside update"))
    );
}

#[test]
fn runtime_mutation_bridge_conflict_discards_staged_scheduler_side_effects() {
    let (_tempdir, service, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    let document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("fixture insert should succeed");
    let bridge = mutation_bridge(
        service.clone(),
        registry_with_scheduled_mutation(),
        tenant_id.clone(),
        nimbus_core::PrincipalContext::anonymous(),
    );
    let convex_id = convex_document_id(&table, &document_id);

    let scheduled_job_id = decode_runtime_result(
        bridge
            .invoke_ctx_scheduler_run_after(json!({
                "delay_ms": 0,
                "name": "messages:sendInternal",
                "visibility": "internal",
                "args": {
                    "body": "Scheduled from tx"
                }
            }))
            .expect("staged scheduler call should encode"),
    )
    .expect("staged scheduler call should succeed");
    assert!(scheduled_job_id.as_str().is_some());
    assert!(
        service
            .list_scheduled_jobs(&tenant_id)
            .expect("scheduled jobs should load")
            .is_empty()
    );

    let _ = decode_runtime_result(
        bridge
            .invoke_ctx_db_patch(json!({
                "table": table,
                "id": convex_id,
                "patch": {
                    "body": "Bridge update"
                }
            }))
            .expect("staged patch should encode"),
    )
    .expect("staged patch should succeed");

    service
        .update_document(
            &tenant_id,
            table.clone(),
            document_id,
            Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("outside update should commit");

    let error = bridge
        .commit_mutation_execution_unit()
        .expect_err("commit should detect the conflict");
    assert!(matches!(error, Error::Conflict(_)));
    assert!(
        service
            .list_scheduled_jobs(&tenant_id)
            .expect("scheduled jobs should load")
            .is_empty()
    );
}
