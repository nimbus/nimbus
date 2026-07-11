use serde_json::{Map, Value, json};

use super::super::host_bridge::ConvexRuntimeResponseEnvelope;
use super::fixture::host_bridge_fixture;
use super::*;

fn decoded_runtime_value(value: Value) -> Value {
    let envelope: ConvexRuntimeResponseEnvelope =
        serde_json::from_value(value).expect("runtime envelope should deserialize");
    envelope
        .into_core_result()
        .expect("runtime result should be ok")
}

fn recorded_document_reads(bridge: &ConvexHostBridge) -> Vec<(String, String)> {
    let dependencies = bridge.snapshot_read_set().dependency_set();
    let mut documents = dependencies
        .documents
        .iter()
        .map(|dependency| {
            (
                dependency.table.to_string(),
                dependency.document_id.to_string(),
            )
        })
        .collect::<Vec<_>>();
    documents.sort();
    documents
}

#[test]
fn executable_get_plan_null_read_records_raw_document_id() {
    let (_tempdir, engine, tenant_id, bridge) = host_bridge_fixture();
    // Materialize the table so the absent read tracks a document dependency
    // rather than a missing-table dependency.
    engine
        .insert_document(
            &tenant_id,
            TableName::new("messages").expect("table should build"),
            Map::from_iter([("body".to_string(), json!("existing"))]),
        )
        .expect("document insert should succeed");

    let result = bridge
        .invoke_ctx_query(json!({
            "query": {
                "type": "get",
                "table": "messages",
                "id": "messages:01ARZ3NDEKTSV4RRFFQ69G5FAV"
            }
        }))
        .expect("runtime get should return an envelope");
    assert_eq!(decoded_runtime_value(result), Value::Null);

    assert_eq!(
        recorded_document_reads(&bridge),
        vec![(
            "messages".to_string(),
            "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string()
        )],
        "a null get must record the raw storage id so a later write to that document invalidates the read set",
    );
}

#[test]
fn executable_get_plan_hit_records_only_raw_document_id() {
    let (_tempdir, engine, tenant_id, bridge) = host_bridge_fixture();
    let table = TableName::new("messages").expect("table should build");
    let document_id = engine
        .insert_document(
            &tenant_id,
            table.clone(),
            Map::from_iter([("body".to_string(), json!("hello"))]),
        )
        .expect("document insert should succeed");
    let convex_id =
        encode_convex_document_id(&table, &document_id).expect("Convex document id should encode");

    let result = bridge
        .invoke_ctx_query(json!({
            "query": {
                "type": "get",
                "table": "messages",
                "id": convex_id.to_string()
            }
        }))
        .expect("runtime get should return an envelope");
    let value = decoded_runtime_value(result);
    assert_eq!(value["body"], json!("hello"));
    assert_eq!(value["_id"], json!(convex_id.to_string()));

    assert_eq!(
        recorded_document_reads(&bridge),
        vec![("messages".to_string(), document_id.to_string())],
        "the read set must hold exactly the raw storage id with no table-scoped duplicate",
    );
}
