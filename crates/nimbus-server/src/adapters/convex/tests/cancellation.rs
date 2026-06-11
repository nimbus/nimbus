use serde_json::json;

use super::fixture::host_bridge_fixture;
use super::*;

#[test]
fn execute_convex_action_cancellable_short_circuits_before_mutation_dispatch() {
    let tempdir = tempdir().expect("runtime action tempdir should build");
    let engine = Engine::new(tempdir.path()).expect("engine should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should be created");
    let registry = ConvexRegistry::empty();
    let cancellation = HostCallCancellation::default();
    cancellation.cancel();

    let result = execute_convex_action_cancellable(
        &engine,
        &registry,
        &tenant_id,
        ConvexExecutableAction::Action(ConvexAction::Mutation {
            mutation: Mutation::Insert {
                table: TableName::new("messages").expect("table should build"),
                id: None,
                fields: serde_json::Map::from_iter([("body".to_string(), json!("hello"))]),
            },
        }),
        &cancellation,
    );

    assert!(matches!(result, Err(Error::Cancelled)));
    let documents = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: TableName::new("messages").expect("table should build"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
        )
        .expect("document query should succeed");
    assert!(documents.is_empty());
}

#[test]
fn runtime_cancellable_db_get_short_circuits_before_dispatch() {
    let (_tempdir, engine, tenant_id, bridge) = host_bridge_fixture();
    let document_id = engine
        .insert_document(
            &tenant_id,
            TableName::new("messages").expect("table should build"),
            serde_json::Map::from_iter([("body".to_string(), json!("hello"))]),
        )
        .expect("document insert should succeed");
    let cancellation = HostCallCancellation::default();
    cancellation.cancel();

    let result = bridge.dispatch_host_call_cancellable(
        HostCallRequest::new(
            HostCallOperation::DocumentGet,
            json!({
                "table": "messages",
                "id": document_id.to_string(),
            }),
        ),
        &cancellation,
    );

    assert!(matches!(result, Err(NimbusRuntimeError::Cancelled)));
    let dependencies = bridge.snapshot_read_set().dependency_set();
    assert!(dependencies.tables.is_empty());
    assert!(dependencies.missing_tables.is_empty());
    assert!(dependencies.missing_predicates.is_empty());
    assert!(dependencies.documents.is_empty());
    assert!(dependencies.index_ranges.is_empty());
    assert!(dependencies.predicates.is_empty());
    assert!(dependencies.paginated_windows.is_empty());
}

#[test]
fn runtime_cancellable_http_route_short_circuits_before_mutation_dispatch() {
    let (_tempdir, engine, tenant_id, bridge) = host_bridge_fixture();
    let cancellation = HostCallCancellation::default();
    cancellation.cancel();

    let result = bridge.dispatch_host_call_cancellable(
        HostCallRequest::new(
            HostCallOperation::HttpRoute,
            json!({
                "request": {
                    "kind": "action",
                    "function_name": "messages:send",
                    "args": {
                        "method": "POST",
                        "url": "http://localhost/messages",
                        "pathname": "/messages",
                        "query": {},
                        "headers": {},
                        "body_bytes": [],
                        "body_text": ""
                    }
                },
                "route": {
                    "name": "messages:send",
                    "method": "POST",
                    "path": "/messages",
                    "plan": {
                        "operation": {
                            "type": "mutation",
                            "mutation": {
                                "type": "insert",
                                "table": "messages",
                                "fields": {
                                    "body": "hello"
                                }
                            }
                        },
                        "response": {
                            "kind": "json",
                            "body": {
                                "ok": true
                            }
                        }
                    }
                }
            }),
        ),
        &cancellation,
    );

    assert!(matches!(result, Err(NimbusRuntimeError::Cancelled)));
    let documents = engine
        .query_documents(
            &tenant_id,
            &Query {
                table: TableName::new("messages").expect("table should build"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
        )
        .expect("document query should succeed");
    assert!(documents.is_empty());
}
