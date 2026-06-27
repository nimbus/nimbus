use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::SystemTime;

use axum::http::header;
use axum::{Json, Router, extract::State, routing::get};
use base64::Engine as Base64Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures::channel::mpsc;
use nimbus_core::{
    AccessOperator, AccessPredicate, AccessRule, AccessValue, CollectionName, CollectionPath,
    DocumentPath, FieldSchema, FieldType, IndexDefinition, PrincipalClaimSource, PrincipalContext,
    SpecialDouble, TableAccessPolicy, TableName, TableSchema, TenantId, TransactionSessionMode,
    TypedScalarValue,
};
use nimbus_engine::{Engine, run_scheduler};
use nimbus_runtime::{
    RuntimeBundle, RuntimeMetricsSnapshot, RuntimeRequestCorrelationSnapshot,
    RuntimeTenantMetricsSnapshot,
};
pub(crate) use nimbus_testing::{
    DeterministicHarness, DeterministicTestCase, EngineFixture, GeneratedTaskHistory,
    GeneratedTaskHistorySeedCase, GeneratedTaskPageExpectation, GeneratedTaskRecord,
    HttpApiFixture, ScenarioMetadata, ServerFixture, VerificationHarnessMode, WebSocketFixture,
    replay_generated_task_history_async, run_to_completion_snapshot_runtime_test_limits,
    selected_generated_task_history_seed_corpus, wait_for_condition, wait_for_value,
};
use prost::Message as ProstMessage;
use prost_types::Timestamp as ProstTimestamp;
use reqwest::StatusCode;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair};
use serde_json::json;
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use tokio::sync::watch;
use tokio::time::{Duration, Instant, timeout};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode as WsCloseCode;
use tonic::Code;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;

use crate::{
    ConvexRegistry, FirebaseConfig, LicenseDocument, LicenseEntitlements, LicenseKind,
    LicenseSourceInfo, LicenseSourceKind, LicenseState, RouterOptions, ServeOptions, build_router,
    serve,
};
use crate::router::RouterBuildConfig;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::document_transform::FieldTransform as GrpcFieldTransform;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::document_transform::field_transform::{
    ServerValue as GrpcServerValue, TransformType as GrpcTransformType,
};
use crate::adapters::firebase::grpc::generated::google::firestore::v1::firestore_client::FirestoreClient;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::batch_get_documents_request::ConsistencySelector as GrpcBatchGetConsistencySelector;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::batch_get_documents_response::Result as GrpcBatchGetResult;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::ExistenceFilter as GrpcExistenceFilter;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::listen_request::TargetChange as GrpcListenTargetChange;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::listen_response::ResponseType as GrpcListenResponseType;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::list_collection_ids_request::ConsistencySelector as GrpcListCollectionIdsConsistencySelector;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::list_documents_request::ConsistencySelector as GrpcListDocumentsConsistencySelector;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::precondition::ConditionType as GrpcConditionType;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::run_aggregation_query_request::ConsistencySelector as GrpcRunAggregationConsistencySelector;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::run_aggregation_query_request::QueryType as GrpcRunAggregationQueryType;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::run_query_request::ConsistencySelector as GrpcRunQueryConsistencySelector;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_aggregation_query::Aggregation as GrpcAggregation;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_aggregation_query::aggregation::Count as GrpcCountAggregation;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_aggregation_query::aggregation::Operator as GrpcAggregationOperator;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_query::CollectionSelector as GrpcCollectionSelector;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_query::Direction as GrpcListenDirection;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_query::FieldFilter as GrpcListenFieldFilter;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_query::FieldReference as GrpcListenFieldReference;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_query::Filter as GrpcListenFilter;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_query::Order as GrpcListenOrder;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_query::field_filter::Operator as GrpcListenFieldFilterOperator;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_query::filter::FilterType as GrpcListenFilterType;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::target::ResumeType as GrpcListenResumeType;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::target::query_target::QueryType as GrpcListenQueryType;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::target::TargetType as GrpcTargetType;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::target_change::TargetChangeType as GrpcTargetChangeType;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::transaction_options::Mode as GrpcTransactionMode;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::transaction_options::ReadOnly as GrpcReadOnlyTransactionOptions;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::transaction_options::read_only::ConsistencySelector as GrpcReadOnlyConsistencySelector;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::value::ValueType as GrpcValueType;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::write::Operation as GrpcWriteOperation;
use crate::adapters::firebase::grpc::generated::google::firestore::v1::{
    ArrayValue as GrpcArrayValue,
    BatchGetDocumentsRequest as GrpcBatchGetDocumentsRequest,
    BatchWriteRequest as GrpcBatchWriteRequest,
    BeginTransactionRequest as GrpcBeginTransactionRequest,
    CommitRequest as GrpcCommitRequest, CreateDocumentRequest as GrpcCreateDocumentRequest,
    Cursor as GrpcCursor,
    DeleteDocumentRequest as GrpcDeleteDocumentRequest, Document as GrpcDocument,
    DocumentChange as GrpcDocumentChange, DocumentMask as GrpcDocumentMask,
    DocumentTransform as GrpcDocumentTransform, ListenRequest as GrpcListenRequest,
    ListenResponse as GrpcListenResponse, Precondition as GrpcPrecondition,
    RollbackRequest as GrpcRollbackRequest,
    RunAggregationQueryRequest as GrpcRunAggregationQueryRequest,
    RunQueryRequest as GrpcRunQueryRequest, StructuredAggregationQuery as GrpcStructuredAggregationQuery,
    StructuredQuery as GrpcStructuredQuery, Target as GrpcTarget, TargetChange as GrpcTargetChange,
    ListCollectionIdsRequest as GrpcListCollectionIdsRequest,
    ListDocumentsRequest as GrpcListDocumentsRequest, TransactionOptions as GrpcTransactionOptions,
    Value as GrpcValue, Write as GrpcWrite, WriteRequest as GrpcWriteRequest,
    GetDocumentRequest as GrpcGetDocumentRequest, UpdateDocumentRequest as GrpcUpdateDocumentRequest,
};

fn router_for_engine(engine: Arc<Engine>) -> Router {
    build_router(RouterOptions::new(engine))
}

fn router_for_convex(engine: Arc<Engine>, convex_registry: ConvexRegistry) -> Router {
    build_router(RouterOptions::new(engine).with_convex_registry(convex_registry))
}

fn router_for_firebase(engine: Arc<Engine>, firebase_config: FirebaseConfig) -> Router {
    build_router(RouterOptions::new(engine).with_firebase_config(firebase_config))
}

fn router_for_license(engine: Arc<Engine>, license_state: LicenseState) -> Router {
    build_router(RouterOptions::new(engine).with_license(license_state))
}

#[tokio::test]
async fn serve_loads_embedded_system_convex_registry_by_default() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("listener should have addr");
    let server = tokio::spawn(serve(listener, ServeOptions::new(fixture.engine())));
    tokio::task::yield_now().await;
    if server.is_finished() {
        let result = server.await;
        panic!("server exited before embedded system query: {result:?}");
    }
    let client = reqwest::Client::new();
    let url = format!("http://{addr}/convex/_nimbus/query");

    let response = wait_for_value(
        "embedded system Convex registry should answer _nimbus queries",
        Duration::from_secs(5),
        Duration::from_millis(25),
        || {
            let client = client.clone();
            let url = url.clone();
            async move {
                client
                    .post(url)
                    .json(&json!({
                        "name": "routes:list",
                        "args": {
                            "adapter": null,
                            "limit": null,
                        },
                    }))
                    .send()
                    .await
            }
        },
        |result| result.is_ok(),
    )
    .await
    .expect("system query should send");
    let status = response.status();
    let body = response
        .text()
        .await
        .expect("system query body should read");
    assert_eq!(status, StatusCode::OK, "system query body: {body}");

    let routes =
        serde_json::from_str::<serde_json::Value>(&body).expect("system query body should parse");
    assert!(
        routes.as_array().is_some_and(|routes| routes
            .iter()
            .any(|route| route["path"] == "/health" && route["adapter"] == "native")),
        "embedded system bundle should return seeded route inventory: {routes}"
    );

    let response = client
        .post(&url)
        .json(&json!({
            "name": "system:status",
            "args": {},
        }))
        .send()
        .await
        .expect("system status query should send");
    let status = response.status();
    let body = response
        .text()
        .await
        .expect("system status query body should read");
    assert_eq!(status, StatusCode::OK, "system status query body: {body}");
    let system_status =
        serde_json::from_str::<serde_json::Value>(&body).expect("status body should parse");
    assert_eq!(system_status["name"], json!("server"));
    assert_eq!(system_status["health"], json!("ok"));
    assert_eq!(system_status["version"], json!(env!("CARGO_PKG_VERSION")));
    assert!(
        system_status["startedAt"].is_number(),
        "embedded system bundle should return server start time: {system_status}"
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn router_prepare_system_tenant_records_enabled_adapter_listeners() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let listen_addr = "127.0.0.1:45678".parse().expect("listen addr should parse");
    RouterBuildConfig::core(fixture.engine())
        .with_system_convex_registry(convex_registry(json!([])))
        .with_firebase(FirebaseConfig::new())
        .with_listen_addr(listen_addr)
        .prepare_system_tenant()
        .await
        .expect("router config should prepare system tenant");

    let listeners = fixture
        .engine()
        .list_documents_async(
            crate::system_tenant::system_tenant_id().expect("system id should parse"),
            TableName::new("listeners").expect("table should parse"),
        )
        .await
        .expect("listeners should list");
    let has_listener = |adapter: &str, protocol: &str| {
        listeners.iter().any(|listener| {
            listener.fields.get("adapter") == Some(&json!(adapter))
                && listener.fields.get("protocol") == Some(&json!(protocol))
                && listener.fields.get("state") == Some(&json!("listening"))
                && listener.fields.get("address") == Some(&json!(listen_addr.to_string()))
        })
    };
    assert!(has_listener("native", "http"));
    assert!(has_listener("convex", "websocket"));
    assert!(has_listener("firebase", "http+websocket"));
}

fn header_csv_values(response: &reqwest::Response, header_name: &str) -> BTreeSet<String> {
    response
        .headers()
        .get_all(header_name)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect()
}

async fn response_json_lines(response: reqwest::Response) -> Vec<serde_json::Value> {
    let body = response
        .text()
        .await
        .expect("streaming JSON response body should deserialize to text");
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|error| {
                panic!("streaming JSON line should parse ({error}): {line}")
            })
        })
        .collect()
}

fn empty_grpc_frame() -> Vec<u8> {
    vec![0, 0, 0, 0, 0]
}

async fn firestore_grpc_client(server: &ServerFixture) -> FirestoreClient<Channel> {
    FirestoreClient::connect(server.http_url(""))
        .await
        .expect("Firestore gRPC client should connect")
}

fn grpc_string_value(value: &str) -> GrpcValue {
    GrpcValue {
        value_type: Some(GrpcValueType::StringValue(value.to_string())),
    }
}

fn grpc_integer_value(value: i64) -> GrpcValue {
    GrpcValue {
        value_type: Some(GrpcValueType::IntegerValue(value)),
    }
}

fn grpc_double_value(value: f64) -> GrpcValue {
    GrpcValue {
        value_type: Some(GrpcValueType::DoubleValue(value)),
    }
}

fn grpc_reference_value(value: &str) -> GrpcValue {
    GrpcValue {
        value_type: Some(GrpcValueType::ReferenceValue(value.to_string())),
    }
}

fn grpc_array_value(values: impl IntoIterator<Item = GrpcValue>) -> GrpcValue {
    GrpcValue {
        value_type: Some(GrpcValueType::ArrayValue(GrpcArrayValue {
            values: values.into_iter().collect(),
        })),
    }
}

fn grpc_update_write(
    document_name: &str,
    fields: impl IntoIterator<Item = (&'static str, GrpcValue)>,
) -> GrpcWrite {
    GrpcWrite {
        operation: Some(GrpcWriteOperation::Update(GrpcDocument {
            name: document_name.to_string(),
            fields: HashMap::from_iter(
                fields
                    .into_iter()
                    .map(|(field, value)| (field.to_string(), value)),
            ),
            create_time: None,
            update_time: None,
        })),
        update_mask: None,
        update_transforms: Vec::new(),
        current_document: None,
    }
}

fn grpc_delete_write(document_name: &str) -> GrpcWrite {
    GrpcWrite {
        operation: Some(GrpcWriteOperation::Delete(document_name.to_string())),
        update_mask: None,
        update_transforms: Vec::new(),
        current_document: None,
    }
}

fn grpc_transform_write(
    document_name: &str,
    field_transforms: impl IntoIterator<Item = GrpcFieldTransform>,
) -> GrpcWrite {
    GrpcWrite {
        operation: Some(GrpcWriteOperation::Transform(GrpcDocumentTransform {
            document: document_name.to_string(),
            field_transforms: field_transforms.into_iter().collect(),
        })),
        update_mask: None,
        update_transforms: Vec::new(),
        current_document: None,
    }
}

fn grpc_server_timestamp_transform(field_path: &str) -> GrpcFieldTransform {
    GrpcFieldTransform {
        field_path: field_path.to_string(),
        transform_type: Some(GrpcTransformType::SetToServerValue(
            GrpcServerValue::RequestTime as i32,
        )),
    }
}

fn grpc_increment_transform(field_path: &str, operand: GrpcValue) -> GrpcFieldTransform {
    GrpcFieldTransform {
        field_path: field_path.to_string(),
        transform_type: Some(GrpcTransformType::Increment(operand)),
    }
}

fn grpc_maximum_transform(field_path: &str, operand: GrpcValue) -> GrpcFieldTransform {
    GrpcFieldTransform {
        field_path: field_path.to_string(),
        transform_type: Some(GrpcTransformType::Maximum(operand)),
    }
}

fn grpc_append_missing_elements_transform(
    field_path: &str,
    values: impl IntoIterator<Item = GrpcValue>,
) -> GrpcFieldTransform {
    GrpcFieldTransform {
        field_path: field_path.to_string(),
        transform_type: Some(GrpcTransformType::AppendMissingElements(GrpcArrayValue {
            values: values.into_iter().collect(),
        })),
    }
}

fn grpc_document_mask(fields: impl IntoIterator<Item = &'static str>) -> GrpcDocumentMask {
    GrpcDocumentMask {
        field_paths: fields.into_iter().map(str::to_string).collect(),
    }
}

fn grpc_batch_get_request(
    documents: impl IntoIterator<Item = &'static str>,
) -> GrpcBatchGetDocumentsRequest {
    GrpcBatchGetDocumentsRequest {
        database: "projects/demo/databases/(default)".to_string(),
        documents: documents.into_iter().map(str::to_string).collect(),
        mask: None,
        consistency_selector: None,
    }
}

fn grpc_run_query_request(
    parent: &str,
    structured_query: GrpcStructuredQuery,
) -> GrpcRunQueryRequest {
    GrpcRunQueryRequest {
        parent: parent.to_string(),
        explain_options: None,
        query_type: Some(
            crate::adapters::firebase::grpc::generated::google::firestore::v1::run_query_request::QueryType::StructuredQuery(
                structured_query,
            ),
        ),
        consistency_selector: None,
    }
}

fn grpc_count_aggregation(alias: &str, up_to: Option<i64>) -> GrpcAggregation {
    GrpcAggregation {
        alias: alias.to_string(),
        operator: Some(GrpcAggregationOperator::Count(GrpcCountAggregation {
            up_to,
        })),
    }
}

fn grpc_run_aggregation_query_request(
    parent: &str,
    structured_query: GrpcStructuredQuery,
    aggregations: Vec<GrpcAggregation>,
) -> GrpcRunAggregationQueryRequest {
    GrpcRunAggregationQueryRequest {
        parent: parent.to_string(),
        explain_options: None,
        query_type: Some(GrpcRunAggregationQueryType::StructuredAggregationQuery(
            GrpcStructuredAggregationQuery {
                query_type: Some(
                    crate::adapters::firebase::grpc::generated::google::firestore::v1::structured_aggregation_query::QueryType::StructuredQuery(
                        structured_query,
                    ),
                ),
                aggregations,
            },
        )),
        consistency_selector: None,
    }
}

fn grpc_listen_query_request(
    target_id: i32,
    parent: &str,
    collection_id: &str,
) -> GrpcListenRequest {
    GrpcListenRequest {
        database: "projects/demo/databases/(default)".to_string(),
        target_change: Some(GrpcListenTargetChange::AddTarget(GrpcTarget {
            target_id,
            once: false,
            expected_count: None,
            target_type: Some(GrpcTargetType::Query(
                crate::adapters::firebase::grpc::generated::google::firestore::v1::target::QueryTarget {
                    parent: parent.to_string(),
                    query_type: Some(GrpcListenQueryType::StructuredQuery(GrpcStructuredQuery {
                        from: vec![GrpcCollectionSelector {
                            collection_id: collection_id.to_string(),
                            all_descendants: false,
                        }],
                        ..Default::default()
                    })),
                },
            )),
            resume_type: None,
        })),
        labels: HashMap::new(),
    }
}

fn grpc_listen_filtered_query_request(
    target_id: i32,
    parent: &str,
    collection_id: &str,
    field_path: &str,
    value: GrpcValue,
) -> GrpcListenRequest {
    GrpcListenRequest {
        database: "projects/demo/databases/(default)".to_string(),
        target_change: Some(GrpcListenTargetChange::AddTarget(GrpcTarget {
            target_id,
            once: false,
            expected_count: None,
            target_type: Some(GrpcTargetType::Query(
                crate::adapters::firebase::grpc::generated::google::firestore::v1::target::QueryTarget {
                    parent: parent.to_string(),
                    query_type: Some(GrpcListenQueryType::StructuredQuery(GrpcStructuredQuery {
                        from: vec![GrpcCollectionSelector {
                            collection_id: collection_id.to_string(),
                            all_descendants: false,
                        }],
                        r#where: Some(GrpcListenFilter {
                            filter_type: Some(GrpcListenFilterType::FieldFilter(
                                GrpcListenFieldFilter {
                                    field: Some(GrpcListenFieldReference {
                                        field_path: field_path.to_string(),
                                    }),
                                    op: GrpcListenFieldFilterOperator::Equal as i32,
                                    value: Some(value),
                                },
                            )),
                        }),
                        ..Default::default()
                    })),
                },
            )),
            resume_type: None,
        })),
        labels: HashMap::new(),
    }
}

fn grpc_listen_query_request_with_resume_token(
    target_id: i32,
    parent: &str,
    collection_id: &str,
    resume_token: Vec<u8>,
) -> GrpcListenRequest {
    let mut request = grpc_listen_query_request(target_id, parent, collection_id);
    let Some(GrpcListenTargetChange::AddTarget(target)) = request.target_change.as_mut() else {
        panic!("Listen add_target request should include a target");
    };
    target.resume_type = Some(GrpcListenResumeType::ResumeToken(resume_token));
    request
}

fn grpc_listen_query_request_with_resume_token_and_expected_count(
    target_id: i32,
    parent: &str,
    collection_id: &str,
    resume_token: Vec<u8>,
    expected_count: i32,
) -> GrpcListenRequest {
    let mut request =
        grpc_listen_query_request_with_resume_token(target_id, parent, collection_id, resume_token);
    let Some(GrpcListenTargetChange::AddTarget(target)) = request.target_change.as_mut() else {
        panic!("Listen add_target request should include a target");
    };
    target.expected_count = Some(expected_count);
    request
}

fn grpc_listen_once_query_request(
    target_id: i32,
    parent: &str,
    collection_id: &str,
) -> GrpcListenRequest {
    let mut request = grpc_listen_query_request(target_id, parent, collection_id);
    let Some(GrpcListenTargetChange::AddTarget(target)) = request.target_change.as_mut() else {
        panic!("Listen add_target request should include a target");
    };
    target.once = true;
    request
}

async fn collect_listen_bootstrap(
    responses: &mut tonic::codec::Streaming<GrpcListenResponse>,
) -> (Vec<GrpcTargetChange>, Vec<GrpcDocumentChange>) {
    let mut target_changes = Vec::new();
    let mut document_changes = Vec::new();
    loop {
        let response = responses
            .message()
            .await
            .expect("Listen response should stream")
            .expect("Listen response should be present");
        match response
            .response_type
            .expect("Listen response should set a response_type")
        {
            GrpcListenResponseType::TargetChange(change) => {
                let change_type = GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode");
                let is_current = change_type == GrpcTargetChangeType::Current;
                target_changes.push(change);
                if is_current {
                    return (target_changes, document_changes);
                }
            }
            GrpcListenResponseType::DocumentChange(change) => document_changes.push(change),
            other => panic!("unexpected bootstrap Listen response: {other:?}"),
        }
    }
}

async fn collect_listen_until_target_change(
    responses: &mut tonic::codec::Streaming<GrpcListenResponse>,
    expected: GrpcTargetChangeType,
) -> (Vec<GrpcTargetChange>, Vec<GrpcDocumentChange>) {
    let mut target_changes = Vec::new();
    let mut document_changes = Vec::new();
    loop {
        let response = timeout(Duration::from_secs(2), responses.message())
            .await
            .expect("Listen response should arrive before the timeout")
            .expect("Listen response should stream")
            .expect("Listen response should be present");
        match response
            .response_type
            .expect("Listen response should set a response_type")
        {
            GrpcListenResponseType::TargetChange(change) => {
                let change_type = GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode");
                let matched = change_type == expected;
                target_changes.push(change);
                if matched {
                    return (target_changes, document_changes);
                }
            }
            GrpcListenResponseType::DocumentChange(change) => document_changes.push(change),
            other => panic!("unexpected Listen response while awaiting {expected:?}: {other:?}"),
        }
    }
}

async fn collect_listen_until_target_change_with_filters(
    responses: &mut tonic::codec::Streaming<GrpcListenResponse>,
    expected: GrpcTargetChangeType,
) -> (
    Vec<GrpcTargetChange>,
    Vec<GrpcDocumentChange>,
    Vec<GrpcExistenceFilter>,
) {
    let mut target_changes = Vec::new();
    let mut document_changes = Vec::new();
    let mut filters = Vec::new();
    loop {
        let response = timeout(Duration::from_secs(2), responses.message())
            .await
            .expect("Listen response should arrive before the timeout")
            .expect("Listen response should stream")
            .expect("Listen response should be present");
        match response
            .response_type
            .expect("Listen response should set a response_type")
        {
            GrpcListenResponseType::TargetChange(change) => {
                let change_type = GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode");
                let matched = change_type == expected;
                target_changes.push(change);
                if matched {
                    return (target_changes, document_changes, filters);
                }
            }
            GrpcListenResponseType::DocumentChange(change) => document_changes.push(change),
            GrpcListenResponseType::Filter(filter) => filters.push(filter),
            other => panic!("unexpected Listen response while awaiting {expected:?}: {other:?}"),
        }
    }
}

async fn collect_listen_until_no_change_for_targets(
    responses: &mut tonic::codec::Streaming<GrpcListenResponse>,
    expected_target_ids: &[i32],
) -> (Vec<GrpcTargetChange>, Vec<GrpcDocumentChange>) {
    let expected_target_ids = BTreeSet::from_iter(expected_target_ids.iter().copied());
    let mut observed_no_change = BTreeSet::new();
    let mut target_changes = Vec::new();
    let mut document_changes = Vec::new();
    loop {
        let response = timeout(Duration::from_secs(2), responses.message())
            .await
            .expect("Listen response should arrive before the timeout")
            .expect("Listen response should stream")
            .expect("Listen response should be present");
        match response
            .response_type
            .expect("Listen response should set a response_type")
        {
            GrpcListenResponseType::TargetChange(change) => {
                let change_type = GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode");
                if change_type == GrpcTargetChangeType::NoChange {
                    for target_id in &change.target_ids {
                        if expected_target_ids.contains(target_id) {
                            observed_no_change.insert(*target_id);
                        }
                    }
                }
                target_changes.push(change);
                if observed_no_change == expected_target_ids {
                    return (target_changes, document_changes);
                }
            }
            GrpcListenResponseType::DocumentChange(change) => document_changes.push(change),
            other => panic!(
                "unexpected Listen response while awaiting multi-target NO_CHANGE: {other:?}"
            ),
        }
    }
}

fn decode_grpc_resume_token(token: &[u8]) -> u64 {
    let bytes: [u8; 8] = token
        .try_into()
        .expect("Listen resume tokens should encode as eight bytes");
    u64::from_be_bytes(bytes)
}

fn encode_grpc_resume_token(sequence: u64) -> Vec<u8> {
    sequence.to_be_bytes().to_vec()
}

fn grpc_timestamp_millis(timestamp: &prost_types::Timestamp) -> i128 {
    i128::from(timestamp.seconds) * 1_000 + i128::from(timestamp.nanos) / 1_000_000
}

async fn next_listen_websocket_response(socket: &mut WebSocketFixture) -> GrpcListenResponse {
    GrpcListenResponse::decode(socket.next_binary().await.as_slice())
        .expect("Listen websocket frame should decode as a protobuf ListenResponse")
}

fn websocket_close_code(message: WsMessage) -> WsCloseCode {
    let WsMessage::Close(Some(frame)) = message else {
        panic!("expected websocket close frame, got {message:?}");
    };
    frame.code
}

async fn collect_listen_websocket_bootstrap(
    socket: &mut WebSocketFixture,
) -> (Vec<GrpcTargetChange>, Vec<GrpcDocumentChange>) {
    let mut target_changes = Vec::new();
    let mut document_changes = Vec::new();
    loop {
        let response = next_listen_websocket_response(socket).await;
        match response
            .response_type
            .expect("Listen websocket response should set a response_type")
        {
            GrpcListenResponseType::TargetChange(change) => {
                let change_type = GrpcTargetChangeType::try_from(change.target_change_type)
                    .expect("target change type should decode");
                let is_current = change_type == GrpcTargetChangeType::Current;
                target_changes.push(change);
                if is_current {
                    return (target_changes, document_changes);
                }
            }
            GrpcListenResponseType::DocumentChange(change) => document_changes.push(change),
            other => panic!("unexpected websocket bootstrap Listen response: {other:?}"),
        }
    }
}

fn seed_firebase_document(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    document_path: &[&str],
    fields: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) {
    seed_firebase_document_with_principal(
        engine,
        tenant_id,
        document_path,
        fields,
        PrincipalContext::anonymous(),
    );
}

fn firebase_owner_access_rule() -> AccessRule {
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

fn firebase_existing_owner_access_rule() -> AccessRule {
    AccessRule {
        require_authenticated: true,
        predicates: vec![AccessPredicate {
            left: AccessValue::ExistingDocumentField {
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

fn firebase_owner_read_write_policy() -> TableAccessPolicy {
    TableAccessPolicy {
        read: firebase_owner_access_rule(),
        create: firebase_owner_access_rule(),
        update: firebase_existing_owner_access_rule(),
        delete: firebase_existing_owner_access_rule(),
    }
}

fn firebase_owner_read_only_policy() -> TableAccessPolicy {
    TableAccessPolicy {
        read: firebase_owner_access_rule(),
        ..TableAccessPolicy::default()
    }
}

fn firebase_owner_schema_for_collection(
    collection_id: &str,
    access_policy: TableAccessPolicy,
) -> TableSchema {
    let collection_path = CollectionPath::root(
        CollectionName::new(collection_id).expect("collection id should parse"),
    );
    let table = crate::adapters::firebase::storage_table_for_collection_path(&collection_path)
        .expect("firebase collection table should derive");
    TableSchema {
        table,
        fields: vec![
            FieldSchema {
                name: "owner".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "name".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "body".to_string(),
                field_type: FieldType::String,
                required: false,
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

fn firebase_test_auth_config(
    issuer: &str,
    application_id: &str,
    jwks_data_url: &str,
) -> serde_json::Value {
    json!({
        "providers": [
            {
                "type": "customJwt",
                "issuer": issuer,
                "jwks": jwks_data_url,
                "algorithm": "ES256",
                "applicationID": application_id
            }
        ]
    })
}

fn seed_firebase_document_with_principal(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    document_path: &[&str],
    fields: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
    principal: PrincipalContext,
) {
    use nimbus_core::{
        AtomicWrite, AtomicWriteBatch, ResourcePathBinding, WriteKey, WritePrecondition,
        WriteSetMode,
    };

    let document_path = DocumentPath::from_segments(document_path.iter().copied())
        .expect("document path should parse");
    let locator = crate::adapters::firebase::locator_for_document_path(&document_path)
        .expect("firebase locator should derive");
    let batch = AtomicWriteBatch::new(vec![AtomicWrite::Set {
        key: WriteKey::from(ResourcePathBinding::new(locator, document_path)),
        document: serde_json::Map::from_iter(
            fields
                .into_iter()
                .map(|(field, value)| (field.to_string(), value)),
        ),
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    }])
    .expect("seed write batch should build");
    engine
        .begin_mutation_execution_unit(tenant_id.clone(), principal)
        .expect("seed execution unit should begin")
        .execute_atomic_write_batch(batch)
        .expect("seed write batch should commit");
}

fn delete_firebase_document(engine: &Arc<Engine>, tenant_id: &TenantId, document_path: &[&str]) {
    use nimbus_core::{
        AtomicWrite, AtomicWriteBatch, ResourcePathBinding, WriteKey, WritePrecondition,
    };

    let document_path = DocumentPath::from_segments(document_path.iter().copied())
        .expect("document path should parse");
    let locator = crate::adapters::firebase::locator_for_document_path(&document_path)
        .expect("firebase locator should derive");
    let batch = AtomicWriteBatch::new(vec![AtomicWrite::Delete {
        key: WriteKey::from(ResourcePathBinding::new(locator, document_path)),
        precondition: WritePrecondition::default(),
        missing_ok: false,
    }])
    .expect("delete write batch should build");
    engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("delete execution unit should begin")
        .execute_atomic_write_batch(batch)
        .expect("delete write batch should commit");
}

#[test]
fn async_runtime_integration_removes_hot_path_blocking_adapters() {
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let engine_mod = fs::read_to_string(workspace_root.join("../nimbus-engine/src/engine/mod.rs"))
        .expect("engine module should be readable");
    assert!(
        !engine_mod.contains("call_blocking("),
        "engine should not retain the call_blocking adapter"
    );

    let async_host_calls =
        fs::read_to_string(workspace_root.join("../nimbus-bridge/src/host_calls/async_calls.rs"))
            .expect("runtime async host call module should be readable");
    assert!(
        !async_host_calls.contains("spawn_blocking("),
        "runtime async host calls should await real futures instead of spawn_blocking wrappers"
    );
    assert!(
        !async_host_calls.contains("execute_async_blocking_host_call"),
        "runtime async host calls should not retain the blocking adapter helper"
    );

    let runtime_capabilities =
        fs::read_to_string(workspace_root.join("../nimbus-bridge/src/capabilities.rs"))
            .expect("runtime host bridge capabilities module should be readable");
    assert!(
        !runtime_capabilities.contains("spawn_blocking("),
        "runtime host capabilities should not hide async write execution behind spawn_blocking"
    );
}

#[tokio::test]
async fn cors_preflight_only_allows_loopback_browser_origins() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;

    let allowed = server
        .client()
        .request(reqwest::Method::OPTIONS, server.http_url("/api/tenants"))
        .header("origin", "http://localhost:5173")
        .header("access-control-request-method", "POST")
        .send()
        .await
        .expect("allowed preflight should send");
    assert_eq!(
        allowed
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("http://localhost:5173")
    );

    let denied = server
        .client()
        .request(reqwest::Method::OPTIONS, server.http_url("/api/tenants"))
        .header("origin", "http://example.com")
        .header("access-control-request-method", "POST")
        .send()
        .await
        .expect("denied preflight should send");
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert!(
        denied
            .headers()
            .get("access-control-allow-origin")
            .is_none(),
        "non-loopback origins must not receive a CORS allow-origin header"
    );
}

fn convex_registry(functions: serde_json::Value) -> ConvexRegistry {
    convex_registry_with_routes(functions, json!([]))
}

fn convex_registry_with_routes(
    functions: serde_json::Value,
    routes: serde_json::Value,
) -> ConvexRegistry {
    convex_registry_with_routes_and_bundle_and_auth_and_schema(functions, routes, None, None, None)
}

fn convex_registry_with_routes_and_bundle(
    functions: serde_json::Value,
    routes: serde_json::Value,
    bundle: Option<&str>,
) -> ConvexRegistry {
    convex_registry_with_routes_and_bundle_and_auth_and_schema(
        functions, routes, bundle, None, None,
    )
}

fn convex_registry_with_routes_and_bundle_and_auth(
    functions: serde_json::Value,
    routes: serde_json::Value,
    bundle: Option<&str>,
    auth_config: Option<serde_json::Value>,
) -> ConvexRegistry {
    convex_registry_with_routes_and_bundle_and_auth_and_schema(
        functions,
        routes,
        bundle,
        auth_config,
        None,
    )
}

fn convex_registry_with_routes_and_bundle_and_auth_and_schema(
    functions: serde_json::Value,
    routes: serde_json::Value,
    bundle: Option<&str>,
    auth_config: Option<serde_json::Value>,
    schema: Option<serde_json::Value>,
) -> ConvexRegistry {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({ "functions": functions }))
            .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": routes }))
            .expect("convex http route json should serialize"),
    )
    .expect("convex http route manifest should write");
    if let Some(auth_config) = auth_config {
        fs::write(
            convex_dir.join("auth.config.json"),
            serde_json::to_vec_pretty(&auth_config).expect("convex auth json should serialize"),
        )
        .expect("convex auth config should write");
    }
    if let Some(schema) = schema {
        fs::write(
            convex_dir.join("schema.json"),
            serde_json::to_vec_pretty(&schema).expect("convex schema json should serialize"),
        )
        .expect("convex schema manifest should write");
    }
    if let Some(bundle) = bundle {
        let bundle_path = convex_dir.join("bundle.mjs");
        fs::write(&bundle_path, bundle).expect("convex runtime bundle should write");
        let bundle_sha256 =
            RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
        fs::write(
            bundle_path.with_extension("sha256"),
            format!("{bundle_sha256}\n"),
        )
        .expect("convex runtime bundle hash should write");
    }
    let registry = ConvexRegistry::from_app_dir(tempdir.path())
        .expect("convex registry should load")
        .with_runtime_limits(run_to_completion_snapshot_runtime_test_limits());
    std::mem::forget(tempdir);
    registry
}

struct PendingHttpRequest {
    _stream: TcpStream,
}

async fn open_json_post_stream(
    server: &ServerFixture,
    path: &str,
    body: &serde_json::Value,
) -> PendingHttpRequest {
    let addr = server
        .http_url("")
        .trim_start_matches("http://")
        .to_string();
    let body = serde_json::to_string(body).expect("request body should serialize");
    let mut stream = TcpStream::connect(&addr)
        .await
        .expect("raw HTTP client should connect");
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("raw HTTP request should write");
    stream.flush().await.expect("raw HTTP request should flush");
    PendingHttpRequest { _stream: stream }
}

async fn wait_for_server_runtime_metrics(
    server: &ServerFixture,
    description: &str,
    predicate: impl Fn(&RuntimeMetricsSnapshot) -> bool,
) -> RuntimeMetricsSnapshot {
    wait_for_server_runtime_metrics_case_impl(server, description.to_string(), predicate).await
}

async fn wait_for_server_runtime_metrics_case(
    server: &ServerFixture,
    case: DeterministicTestCase,
    description: &str,
    predicate: impl Fn(&RuntimeMetricsSnapshot) -> bool,
) -> RuntimeMetricsSnapshot {
    wait_for_server_runtime_metrics_case_impl(server, case.failure_context(description), predicate)
        .await
}

async fn wait_for_server_runtime_metrics_case_impl(
    server: &ServerFixture,
    description: String,
    predicate: impl Fn(&RuntimeMetricsSnapshot) -> bool,
) -> RuntimeMetricsSnapshot {
    // Synchronization budget for a runtime-metrics condition (e.g. "the
    // blocking query has dispatched onto a worker"), NOT an assertion. The
    // first dispatch pays a cold-start cost -- worker-thread spawn + per-job
    // tokio runtime build + first V8 isolate warm-up -- that has crossed 20s
    // on a resource-contended CI shard. This loop only ever awaits a positive
    // condition and panics on timeout, so a wider budget buys slack on a loaded
    // host, costs nothing on a healthy one, and never weakens the
    // post-conditions these tests assert once the condition is met.
    let timeout = Duration::from_secs(45);
    let poll_interval = Duration::from_millis(25);
    let started_at = Instant::now();
    let mut attempts = 0_u64;
    loop {
        attempts += 1;
        let snapshots = server_runtime_metric_snapshots(server).await;
        for snapshot in &snapshots {
            if predicate(snapshot) {
                return snapshot.clone();
            }
        }
        let elapsed = started_at.elapsed();
        if elapsed >= timeout {
            panic!(
                "timed out waiting for {description} after {elapsed:?} (budget {timeout:?}, \
                 poll interval {poll_interval:?}, attempts {attempts}); last runtime metrics \
                 snapshots by lane: {snapshots:#?}"
            );
        }
        tokio::time::sleep(poll_interval).await;
    }
}

async fn server_runtime_metric_snapshots(server: &ServerFixture) -> Vec<RuntimeMetricsSnapshot> {
    let response = server
        .client()
        .get(server.http_url("/debug/runtime/metrics"))
        .send()
        .await
        .expect("runtime diagnostics request should send");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime diagnostics body should parse");
    let lanes = body["lanes"]
        .as_array()
        .expect("runtime diagnostics should include lane snapshots");
    lanes
        .iter()
        .map(|lane| runtime_metrics_snapshot_from_json(&lane["metrics"]))
        .collect()
}

fn runtime_metrics_snapshot_from_json(metrics: &serde_json::Value) -> RuntimeMetricsSnapshot {
    RuntimeMetricsSnapshot {
        active_runtime_instances: json_usize(metrics, "active_runtime_instances"),
        queued_invocations: json_usize(metrics, "queued_invocations"),
        worker_dispatched_invocations: json_u64(metrics, "worker_dispatched_invocations"),
        runtime_pool_hits: json_u64(metrics, "runtime_pool_hits"),
        runtime_pool_misses: json_u64(metrics, "runtime_pool_misses"),
        runtime_pool_replacements: json_u64(metrics, "runtime_pool_replacements"),
        started_invocations: json_u64(metrics, "started_invocations"),
        completed_invocations: json_u64(metrics, "completed_invocations"),
        timed_out_invocations: json_u64(metrics, "timed_out_invocations"),
        canceled_invocations: json_u64(metrics, "canceled_invocations"),
        rejected_invocations: json_u64(metrics, "rejected_invocations"),
        queued_canceled_invocations: json_u64(metrics, "queued_canceled_invocations"),
        in_flight_canceled_invocations: json_u64(metrics, "in_flight_canceled_invocations"),
        disconnect_canceled_invocations: json_u64(metrics, "disconnect_canceled_invocations"),
        explicit_canceled_invocations: json_u64(metrics, "explicit_canceled_invocations"),
        tenants: runtime_tenant_metrics_from_json(metrics),
        recent_request_correlations: runtime_request_correlations_from_json(metrics),
        ..Default::default()
    }
}

fn runtime_tenant_metrics_from_json(
    metrics: &serde_json::Value,
) -> BTreeMap<String, RuntimeTenantMetricsSnapshot> {
    let Some(tenants) = metrics["tenants"].as_object() else {
        return BTreeMap::new();
    };
    tenants
        .iter()
        .map(|(tenant_id, metrics)| {
            let snapshot = RuntimeTenantMetricsSnapshot {
                active_runtime_instances: json_usize(metrics, "active_runtime_instances"),
                started_invocations: json_u64(metrics, "started_invocations"),
                completed_invocations: json_u64(metrics, "completed_invocations"),
                rejected_invocations: json_u64(metrics, "rejected_invocations"),
                queued_canceled_invocations: json_u64(metrics, "queued_canceled_invocations"),
                in_flight_canceled_invocations: json_u64(metrics, "in_flight_canceled_invocations"),
                disconnect_canceled_invocations: json_u64(
                    metrics,
                    "disconnect_canceled_invocations",
                ),
                explicit_canceled_invocations: json_u64(metrics, "explicit_canceled_invocations"),
                ..Default::default()
            };
            (tenant_id.clone(), snapshot)
        })
        .collect()
}

fn runtime_request_correlations_from_json(
    metrics: &serde_json::Value,
) -> Vec<RuntimeRequestCorrelationSnapshot> {
    let Some(correlations) = metrics["recent_request_correlations"].as_array() else {
        return Vec::new();
    };
    correlations
        .iter()
        .map(|correlation| RuntimeRequestCorrelationSnapshot {
            invocation_id: json_u64(correlation, "invocation_id"),
            server_request_id: json_string(correlation, "server_request_id"),
            tenant_label: correlation["tenant_label"].as_str().map(ToOwned::to_owned),
            function_name: json_string(correlation, "function_name"),
            kind: json_string(correlation, "kind"),
            is_top_level: correlation["is_top_level"].as_bool().unwrap_or(false),
            bypasses_concurrency_limit: correlation["bypasses_concurrency_limit"]
                .as_bool()
                .unwrap_or(false),
        })
        .collect()
}

fn json_u64(value: &serde_json::Value, field: &str) -> u64 {
    value[field].as_u64().unwrap_or(0)
}

fn json_usize(value: &serde_json::Value, field: &str) -> usize {
    json_u64(value, field) as usize
}

fn json_string(value: &serde_json::Value, field: &str) -> String {
    value[field].as_str().unwrap_or_default().to_owned()
}

#[path = "tests/auth_fixtures/mod.rs"]
mod auth_fixtures;

#[path = "tests/auth.rs"]
mod auth;
#[path = "tests/convex_functions.rs"]
mod convex_functions;
#[path = "tests/convex_runtime.rs"]
mod convex_runtime;
#[path = "tests/core_http.rs"]
mod core_http;
#[path = "tests/cors.rs"]
mod cors;
#[path = "tests/deploy.rs"]
mod deploy;
#[path = "tests/dynamodb_wire.rs"]
mod dynamodb_wire;
#[path = "tests/firebase/auth_and_availability.rs"]
mod firebase_auth_and_availability;
#[path = "tests/firebase/grpc_unary.rs"]
mod firebase_grpc_unary;
#[path = "tests/firebase/listen.rs"]
mod firebase_listen;
#[path = "tests/firebase/rest_and_cors.rs"]
mod firebase_rest_and_cors;
#[path = "tests/firebase/rest_crud.rs"]
mod firebase_rest_crud;
#[path = "tests/firebase/rest_query.rs"]
mod firebase_rest_query;
#[path = "tests/firebase/write_stream.rs"]
mod firebase_write_stream;
#[path = "tests/local_admin.rs"]
mod local_admin;
#[path = "tests/local_audit.rs"]
mod local_audit;
#[path = "tests/local_server_security.rs"]
mod local_server_security;
#[path = "tests/local_ui.rs"]
mod local_ui;
#[path = "tests/machine_lifecycle.rs"]
mod machine_lifecycle;
#[path = "tests/mongodb_wire.rs"]
mod mongodb_wire;
#[path = "tests/registry_and_license/mod.rs"]
mod registry_and_license;
#[path = "tests/rest_route_parity.rs"]
mod rest_route_parity;
#[path = "tests/scheduling.rs"]
mod scheduling;
#[path = "tests/tenant_isolation_harness.rs"]
mod tenant_isolation_harness;
#[path = "tests/tls_serve.rs"]
mod tls_serve;
#[path = "tests/verification_harness.rs"]
mod verification_harness;
#[path = "tests/websocket_protocol.rs"]
mod websocket_protocol;
