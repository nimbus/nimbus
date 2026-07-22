use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;

use nimbus_core::{
    AccessOperator, AccessPredicate, AccessRule, AccessValue, DocumentId, Error, FieldSchema,
    FieldType, IndexDefinition, InvocationAuth, OrderBy, OrderDirection, PrincipalClaimSource,
    Query, RuntimeUserIdentity, TableAccessPolicy, TableName, TableSchema, TenantId,
};
use nimbus_runtime::{
    InvocationKind, InvocationServiceBinding, InvocationServiceProtocol, InvocationServices,
    NimbusRuntimeError,
};
use serde_json::{Map, Value, json};

use super::super::execution::execute_query_result_cancellable_with_auth;
use super::super::host_bridge::{ConvexHostBridge, ConvexRuntimeResponseEnvelope};
use super::fixture::host_bridge_fixture;
use super::*;
use nimbus_auth::normalize_principal_context;
use nimbus_bridge::capabilities::RuntimeServiceCapabilityHost;
use nimbus_services::{RuntimeServiceRegistry, ServiceInstanceBindingRegistry};

struct StaticRuntimeServiceRegistry {
    service_name: String,
    binding: InvocationServiceBinding,
}

impl RuntimeServiceRegistry for StaticRuntimeServiceRegistry {
    fn snapshot_for_tenant(&self, _tenant_id: &TenantId) -> InvocationServices {
        InvocationServices::new()
    }

    fn resolve_service_binding(
        &self,
        _tenant_id: &TenantId,
        service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>, Error> {
        Ok((service_name == self.service_name).then(|| self.binding.clone()))
    }
}

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

    assert!(
        bridge.service_capabilities().is_none(),
        "ungranted bridge must not expose runtime service capabilities"
    );

    let value = bridge
        .invoke_ctx_service_lookup(json!({
            "service_name": "db",
            "host_call_session_id": bridge.host_call_session_id(),
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
        error
            .to_string()
            .contains("runtime service capability was not granted"),
        "error should name the missing service capability: {error}"
    );
}

#[test]
fn host_bridge_service_capabilities_are_exact_grant_only() {
    let (_tempdir, engine, tenant_id, _bridge) = host_bridge_fixture();
    let registry = Arc::new(ConvexRegistry::empty());
    let bridge = service_capable_bridge(
        engine,
        registry,
        tenant_id,
        Arc::new(StaticRuntimeServiceRegistry {
            service_name: "db".to_string(),
            binding: InvocationServiceBinding {
                host: "127.0.0.1".to_string(),
                port: 15432,
                protocol: InvocationServiceProtocol::Tcp,
                endpoints: BTreeMap::new(),
            },
        }),
        ["db".to_string()],
    );

    let service_capabilities = bridge
        .service_capabilities()
        .expect("db grant should expose runtime service capabilities");
    let service_access = service_capabilities
        .service_access("db")
        .expect("exact service grant should authorize db");
    assert_eq!(service_access.service_name(), "db");
    assert_eq!(service_access.tenant_id(), bridge.tenant_id());

    let denied = service_capabilities
        .service_access("cache")
        .expect_err("service capability must reject non-exact service names");
    assert!(
        denied
            .to_string()
            .contains("did not authorize service `cache`"),
        "exact service denial should name the rejected service: {denied}"
    );

    let value = bridge
        .invoke_ctx_service_lookup(json!({
            "service_name": "db",
            "host_call_session_id": bridge.host_call_session_id(),
        }))
        .expect("service lookup should return a runtime envelope");
    let binding = decode_runtime_result(value).expect("db service lookup should succeed");
    assert_eq!(binding["port"], json!(15432));

    let denied_value = bridge
        .invoke_ctx_service_lookup(json!({
            "service_name": "cache",
            "host_call_session_id": bridge.host_call_session_id(),
        }))
        .expect("denied service lookup should still return a runtime envelope");
    let denied_error =
        decode_runtime_result(denied_value).expect_err("cache lookup must be denied");
    assert!(
        denied_error
            .to_string()
            .contains("did not authorize service `cache`"),
        "service lookup denial should use exact grant wording: {denied_error}"
    );
}

fn service_capable_bridge(
    engine: Arc<Engine>,
    registry: Arc<ConvexRegistry>,
    tenant_id: TenantId,
    runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    services: impl IntoIterator<Item = String>,
) -> ConvexHostBridge {
    let isolation = nimbus_tenant::TenantIsolationContext::application(
        tenant_id,
        nimbus_core::PrincipalContext::anonymous(),
        "convex_service_capability_test",
    );
    let runtime_policy = Arc::new(nimbus_runtime::RuntimePolicy::new(
        registry.runtime_limits(),
    ));
    let decision = nimbus_tenant::admit_runtime_invocation_decision(
        &isolation,
        "convex_service_capability_test",
        None,
        &runtime_policy,
        nimbus_tenant::RuntimeIsolationTier::InProcessUntrusted,
        nimbus_tenant::TenantIsolationMode::LocalDevelopment,
        services,
    )
    .expect("service capability tenant isolation decision should build");
    ConvexHostBridge::build(
        ConvexHostBridgeScope::new_for_test(engine, registry, decision, runtime_service_registry),
        ConvexHostBridgeInvocation::new(
            None,
            Default::default(),
            nimbus_core::PrincipalContext::anonymous(),
            None,
            InvocationKind::Query,
            "convex_service_capability_test",
        ),
    )
    .expect("service capable bridge should build")
}

fn mutation_bridge(
    engine: Arc<Engine>,
    registry: Arc<ConvexRegistry>,
    tenant_id: TenantId,
    principal: nimbus_core::PrincipalContext,
) -> ConvexHostBridge {
    let isolation = nimbus_tenant::TenantIsolationContext::application(
        tenant_id,
        principal.clone(),
        "convex_authorization_test",
    );
    let runtime_policy = Arc::new(nimbus_runtime::RuntimePolicy::new(
        registry.runtime_limits(),
    ));
    let decision = nimbus_tenant::admit_runtime_invocation_decision(
        &isolation,
        "convex_authorization_test",
        None,
        &runtime_policy,
        nimbus_tenant::RuntimeIsolationTier::InProcessUntrusted,
        nimbus_tenant::TenantIsolationMode::LocalDevelopment,
        std::iter::empty::<String>(),
    )
    .expect("authorization test tenant isolation decision should build");
    ConvexHostBridge::build(
        ConvexHostBridgeScope::new_for_test(
            engine,
            registry,
            decision,
            Arc::new(ServiceInstanceBindingRegistry::new(Arc::new(
                nimbus_services::EmptyServiceInstanceCatalog,
            ))),
        ),
        ConvexHostBridgeInvocation::new(
            None,
            Default::default(),
            principal,
            None,
            InvocationKind::Mutation,
            "convex_authorization_test",
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
    let (_tempdir, engine, tenant_id, _bridge) = host_bridge_fixture();
    let tenant_b = TenantId::new("tenant-b").expect("tenant-b id should be valid");
    engine
        .create_tenant(tenant_b.clone())
        .expect("tenant-b should be created");
    let table = messages_table();
    let bridge = mutation_bridge(
        engine.clone(),
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
        let documents = engine
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
    let (_tempdir, engine, tenant_id, _bridge) = host_bridge_fixture();
    let tenant_b = TenantId::new("tenant-b").expect("tenant-b id should be valid");
    engine
        .create_tenant(tenant_b.clone())
        .expect("tenant-b should be created");
    let bridge = mutation_bridge(
        engine.clone(),
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
        engine
            .list_scheduled_jobs(&tenant_id)
            .expect("tenant-a scheduled jobs should load")
            .is_empty()
    );
    assert!(
        engine
            .list_scheduled_jobs(&tenant_b)
            .expect("tenant-b scheduled jobs should load")
            .is_empty()
    );
}

#[test]
fn convex_query_execution_matches_direct_engine_authorization_for_same_normalized_principal() {
    let (_tempdir, engine, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    engine
        .set_table_schema(&tenant_id, schema_with_owner_policy(read_only_policy()))
        .expect("schema should save");

    for (owner, body) in [("user-123", "Ada"), ("user-456", "Grace")] {
        engine
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
        engine
            .query_documents_with_principal(&tenant_id, &query, &principal)
            .expect("direct query should succeed"),
    )
    .expect("direct documents should encode as Convex JSON");
    let convex_json = execute_query_result_cancellable_with_auth(
        engine.as_ref(),
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
    let (_tempdir, engine, tenant_id, _anonymous_bridge) = host_bridge_fixture();
    let table = messages_table();

    for (owner, body) in [("user-123", "Ada"), ("user-456", "Grace")] {
        engine
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
    engine
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
        engine
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
    let isolation = nimbus_tenant::TenantIsolationContext::application(
        tenant_id.clone(),
        normalize_principal_context(Some(&auth)),
        "convex_authorization_test",
    );
    let runtime_policy = Arc::new(nimbus_runtime::RuntimePolicy::new(
        registry.runtime_limits(),
    ));
    let decision = nimbus_tenant::admit_runtime_invocation_decision(
        &isolation,
        "convex_authorization_test",
        None,
        &runtime_policy,
        nimbus_tenant::RuntimeIsolationTier::InProcessUntrusted,
        nimbus_tenant::TenantIsolationMode::LocalDevelopment,
        std::iter::empty::<String>(),
    )
    .expect("authorization query tenant isolation decision should build");
    let bridge = ConvexHostBridge::new(
        ConvexHostBridgeScope::new_for_test(
            engine.clone(),
            registry,
            decision,
            Arc::new(ServiceInstanceBindingRegistry::new(Arc::new(
                nimbus_services::EmptyServiceInstanceCatalog,
            ))),
        ),
        ConvexHostBridgeInvocation::new(
            Some(auth.clone()),
            Default::default(),
            normalize_principal_context(Some(&auth)),
            None,
            InvocationKind::Query,
            "convex_authorization_query_test",
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

    // Count document-bearing commits rather than comparing raw
    // `latest_sequence`: the tenant's background trigger-candidate feed
    // appends zero-write cursor-advance commits to the same sequence space
    // at its own pace, so a raw-sequence equality races it (observed
    // flake). A denied insert must add no DOCUMENT commit; cursor commits
    // carry no writes and cannot mask one.
    let document_commits = |engine: &nimbus_engine::Engine| {
        engine
            .read_durable_journal(&tenant_id, nimbus_core::SequenceNumber(0))
            .expect("durable journal should read")
            .into_iter()
            .filter(|record| !record.as_commit_entry().writes.is_empty())
            .count()
    };
    let document_commits_before = document_commits(&engine);
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
        document_commits(&engine),
        document_commits_before,
        "denied insert must not add a document commit"
    );
}

#[test]
fn runtime_mutation_bridge_stages_writes_until_commit_and_reads_its_own_writes() {
    let (_tempdir, engine, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    let bridge = mutation_bridge(
        engine.clone(),
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
        engine.get_document(&tenant_id, &table, document_id.clone()),
        Err(Error::DocumentNotFound(_))
    ));

    bridge
        .commit_mutation_execution_unit()
        .expect("commit should persist staged writes");
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("committed document should exist")
            .get_field("body"),
        Some(&json!("Hello from tx"))
    );
}

#[test]
fn runtime_mutation_bridge_reads_own_writes_even_when_materialized_serving_snapshot_is_warmed() {
    let (_tempdir, engine, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    engine
        .insert_document(
            &tenant_id,
            table.clone(),
            Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Already committed")),
            ]),
        )
        .expect("fixture insert should succeed");
    let warmed = engine
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
    let surface_stats = engine
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(surface_stats.loaded_table_count, 1);

    let bridge = mutation_bridge(
        engine.clone(),
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
        engine.get_document(&tenant_id, &table, document_id),
        Err(Error::DocumentNotFound(_))
    ));
}

#[test]
fn runtime_host_bridge_rejects_wrong_table_convex_document_ids() {
    let (_tempdir, engine, tenant_id, _bridge) = host_bridge_fixture();
    let messages = messages_table();
    let users = TableName::new("users").expect("users table should be valid");
    let bridge = mutation_bridge(
        engine,
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
    let (_tempdir, engine, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    let raw_id = DocumentId::from_key("custom:id".to_string())
        .expect("custom id with colon should be a valid storage id");
    engine
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
        engine.as_ref(),
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
fn runtime_host_bridge_get_records_missing_durable_document_reads() {
    let (_tempdir, engine, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    let seed_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Seed")),
            ]),
        )
        .expect("seed document should create the durable table");
    engine
        .delete_document(&tenant_id, table.clone(), seed_id)
        .expect("seed document should be deleted while preserving table identity");
    let missing_id = DocumentId::from_key("missing-doc".to_string())
        .expect("missing document id should be valid");
    let convex_id = convex_document_id(&table, &missing_id);
    let bridge = mutation_bridge(
        engine.clone(),
        Arc::new(ConvexRegistry::empty()),
        tenant_id.clone(),
        nimbus_core::PrincipalContext::anonymous(),
    );

    let read_back = decode_runtime_result(
        bridge
            .invoke_ctx_db_get(json!({
                "table": table,
                "id": convex_id
            }))
            .expect("missing get should encode"),
    )
    .expect("missing get should succeed");

    assert_eq!(read_back, Value::Null);
    let committed_table_id = engine
        .table_id(&tenant_id, &table)
        .expect("table id lookup should succeed")
        .expect("deleted seed should leave a durable table id");
    let dependencies = bridge.snapshot_read_set().dependency_set();
    assert!(
        dependencies.documents.iter().any(|dependency| {
            dependency.table == table
                && dependency.table_id == committed_table_id
                && dependency.document_id == missing_id
        }),
        "Convex ctx.db.get should record absent document reads through the shared bridge helper"
    );
}

#[test]
fn runtime_mutation_bridge_commit_detects_occ_conflicts() {
    let (_tempdir, engine, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    let document_id = engine
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
        engine.clone(),
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
    let committed_table_id = engine
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

    engine
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
    assert!(matches!(error, Error::Conflict { .. }));
    assert_eq!(
        engine
            .get_document(&tenant_id, &table, document_id.clone())
            .expect("document should remain committed")
            .get_field("body"),
        Some(&json!("Outside update"))
    );
}

#[test]
fn runtime_mutation_bridge_conflict_discards_staged_scheduler_side_effects() {
    let (_tempdir, engine, tenant_id, _bridge) = host_bridge_fixture();
    let table = messages_table();
    let document_id = engine
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
        engine.clone(),
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
        engine
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

    engine
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
    assert!(matches!(error, Error::Conflict { .. }));
    assert!(
        engine
            .list_scheduled_jobs(&tenant_id)
            .expect("scheduled jobs should load")
            .is_empty()
    );
}
