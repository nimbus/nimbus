use std::collections::{BTreeSet, HashMap};
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
use nimbus_runtime::{RuntimeBundle, RuntimeMetricsSnapshot};
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
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::sync::watch;
use tokio::time::{Duration, timeout};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode as WsCloseCode;
use tonic::Code;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;

use crate::{
    CloudflareConfig, ConvexRegistry, FirebaseConfig, LicenseDocument, LicenseEntitlements,
    LicenseKind, LicenseSourceInfo, LicenseSourceKind, LicenseState, ProjectTenantRegistry,
    RouterOptions, ServeOptions, build_router, serve,
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
    let test_host_parallelism =
        std::num::NonZeroUsize::new(64).expect("test host parallelism is nonzero");
    build_router(
        RouterOptions::new(engine)
            .with_convex_registry(convex_registry)
            .with_runtime_host_resource_budget(
                nimbus_runtime::RuntimeHostResourceBudget::conservative_for_logical_cpus(
                    test_host_parallelism,
                ),
            ),
    )
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
        .with_cloudflare(CloudflareConfig::default())
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
    assert!(has_listener("cloudflare", "http"));
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

/// A genuine Firebase ID-token issuer for `project`
/// (`securetoken.google.com/<project>`). The #24 gate derives the verified
/// project from this issuer.
fn firebase_securetoken_issuer(project: &str) -> String {
    format!("https://securetoken.google.com/{project}")
}

/// A dev-mode Firebase Emulator bearer token: a JSON object (not a signed JWT)
/// carrying the subject and the Firebase project issuer. The token-verification
/// bypass fabricates a *verified* principal from it, so `iss` selects the
/// project the #24 registry binds to a tenant. `sub` becomes the principal's
/// `subject`/`sub` claim (owner-based access policies match on it).
fn firebase_verified_token(subject: &str, project: &str) -> String {
    json!({
        "sub": subject,
        "iss": firebase_securetoken_issuer(project),
    })
    .to_string()
}

/// The `Authorization: Bearer` value for a verified-path Firestore request to
/// `project` as `subject`.
fn firebase_verified_bearer(subject: &str, project: &str) -> String {
    format!("Bearer {}", firebase_verified_token(subject, project))
}

/// `FirebaseConfig` for the verified-path tests: the dev-mode
/// token-verification bypass on (so a JSON emulator token fabricates a verified
/// project) plus the identity registry (project -> same-named tenant), so a
/// token for project `demo` reaches tenant `demo`. The `ServerFixture` binds
/// loopback, so the bypass is allowed (the boot guard only refuses it on a
/// public bind).
fn firebase_verified_config() -> FirebaseConfig {
    FirebaseConfig::new()
        .with_emulator_token_verification_bypass()
        .with_project_registry(ProjectTenantRegistry::identity())
}

/// The exact [`PrincipalContext`] the dev-mode bypass fabricates for the
/// verified-path token. Use it when a test seeds engine state under the same
/// principal a verified-path request carries (e.g. a transaction session, which
/// the engine binds to its creating principal), so the engine's principal-bound
/// checks match the later verified-path request.
fn firebase_verified_principal(subject: &str, project: &str) -> PrincipalContext {
    nimbus_auth::firebase_emulator_verification_bypass_principal_from_bearer(
        &firebase_verified_token(subject, project),
    )
    .expect("verified-path bypass principal should build")
}

/// The browser WebSocket subprotocol header value that offers the dev-mode
/// verified bearer for `project` alongside the Firestore Listen subprotocol.
fn firebase_listen_ws_auth_protocol(subject: &str, project: &str) -> String {
    let encoded = URL_SAFE_NO_PAD.encode(firebase_verified_token(subject, project).as_bytes());
    format!("nimbus.firebase.listen.v1,nimbus.firebase.auth.{encoded}")
}

/// A reqwest client that sends the dev-mode verified-path bearer for `project`
/// on every request, for the verified-path Firestore REST tests. Pair it with
/// [`assert_firebase_rest_anonymous_refused`] (which uses the fixture's
/// unauthenticated client) to keep each test non-vacuous.
fn firebase_rest_client(subject: &str, project: &str) -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::AUTHORIZATION,
        firebase_verified_bearer(subject, project)
            .parse()
            .expect("verified-path bearer should be a valid header value"),
    );
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("verified-path reqwest client should build")
}

/// Wrap a unary or client-streaming gRPC payload in a request carrying the
/// dev-mode verified bearer for `project`, so the #24 gate resolves the project
/// to its tenant.
fn firebase_grpc_request<T>(message: T, subject: &str, project: &str) -> tonic::Request<T> {
    let mut request = tonic::Request::new(message);
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(firebase_verified_bearer(subject, project))
            .expect("grpc authorization metadata should build"),
    );
    request
}

/// A tonic interceptor that attaches the dev-mode verified bearer to every
/// outgoing Firestore gRPC request, so the #24 gate resolves the project to its
/// tenant without wrapping each call site.
#[derive(Clone)]
struct FirebaseBearerInterceptor {
    bearer: MetadataValue<tonic::metadata::Ascii>,
}

impl tonic::service::Interceptor for FirebaseBearerInterceptor {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> std::result::Result<tonic::Request<()>, tonic::Status> {
        request
            .metadata_mut()
            .insert("authorization", self.bearer.clone());
        Ok(request)
    }
}

/// A Firestore gRPC client whose every request carries the dev-mode verified
/// bearer for `project` as `subject` (the #24 verified-path client). Pair it
/// with the matching `assert_firebase_grpc_*_anonymous_refused` helper to keep
/// each test non-vacuous.
async fn firestore_grpc_authed_client(
    server: &ServerFixture,
    subject: &str,
    project: &str,
) -> FirestoreClient<
    tonic::service::interceptor::InterceptedService<Channel, FirebaseBearerInterceptor>,
> {
    let channel = Channel::from_shared(server.http_url(""))
        .expect("Firestore gRPC channel URI should parse")
        .connect()
        .await
        .expect("Firestore gRPC channel should connect");
    let bearer = MetadataValue::try_from(firebase_verified_bearer(subject, project))
        .expect("grpc authorization metadata should build");
    FirestoreClient::with_interceptor(channel, FirebaseBearerInterceptor { bearer })
}

/// Assert an anonymous (no-metadata) gRPC unary request is refused at the #24
/// gate with `PermissionDenied`. The non-vacuous half of every migrated gRPC
/// unary test (the gate runs before any document access, so a fixed read path
/// proves the refusal).
async fn assert_firebase_grpc_unary_anonymous_refused(server: &ServerFixture) {
    let mut client = firestore_grpc_client(server).await;
    let error = client
        .get_document(GrpcGetDocumentRequest {
            name: "projects/demo/databases/(default)/documents/cities/SF".to_string(),
            mask: None,
            consistency_selector: None,
        })
        .await
        .expect_err("anonymous gRPC unary request must be refused by the #24 gate");
    assert_eq!(error.code(), Code::PermissionDenied);
}

/// Assert the Firestore REST endpoint refuses an anonymous (no-bearer) request
/// with HTTP 403 — the #24 verified-project gate refuses a caller with no
/// verified Firebase project. The non-vacuous half of every migrated REST test.
async fn assert_firebase_rest_anonymous_refused(server: &ServerFixture, path: &str, body: &str) {
    let response = server
        .client()
        .post(server.http_url(path))
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body(body.to_string())
        .send()
        .await
        .expect("anonymous firebase request should send");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "anonymous Firestore REST request must be refused by the #24 verified-project gate: {path}"
    );
}

/// Assert an anonymous (no-metadata) gRPC write-stream handshake is refused at
/// the #24 gate (the gate runs on the handshake, so the first response is the
/// refusal). The non-vacuous half of every migrated write-stream test.
async fn assert_firebase_grpc_write_stream_anonymous_refused(
    server: &ServerFixture,
    database: &str,
) {
    let mut client = firestore_grpc_client(server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .write(receiver)
        .await
        .expect("anonymous write stream should open")
        .into_inner();
    sender
        .unbounded_send(GrpcWriteRequest {
            database: database.to_string(),
            ..Default::default()
        })
        .expect("anonymous write handshake should send");
    let error = responses
        .message()
        .await
        .expect_err("anonymous write-stream handshake must be refused by the #24 gate");
    assert_eq!(error.code(), Code::PermissionDenied);
}

/// Assert an anonymous (no-metadata) gRPC Listen add-target is refused at the
/// #24 gate. The non-vacuous half of every migrated gRPC Listen test.
async fn assert_firebase_grpc_listen_anonymous_refused(
    server: &ServerFixture,
    parent: &str,
    collection_id: &str,
) {
    let mut client = firestore_grpc_client(server).await;
    let (sender, receiver) = mpsc::unbounded();
    let mut responses = client
        .listen(receiver)
        .await
        .expect("anonymous Listen stream should open")
        .into_inner();
    sender
        .unbounded_send(grpc_listen_query_request(0, parent, collection_id))
        .expect("anonymous Listen add_target should send");
    let error = responses
        .message()
        .await
        .expect_err("anonymous Listen add_target must be refused by the #24 gate");
    assert_eq!(error.code(), Code::PermissionDenied);
}

/// Connect a browser WebSocket Listen socket that offers the dev-mode verified
/// bearer for `project` as `subject` (the #24 verified-path WebSocket client).
async fn firebase_listen_ws_connect(
    server: &ServerFixture,
    subject: &str,
    project: &str,
) -> WebSocketFixture {
    let mut request = server
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("listen websocket request should build");
    request.headers_mut().insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_static("http://localhost:5173"),
    );
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        axum::http::HeaderValue::from_str(&firebase_listen_ws_auth_protocol(subject, project))
            .expect("listen auth subprotocol header should build"),
    );
    WebSocketFixture::connect_request(request)
        .await
        .expect("authenticated listen websocket should connect")
}

/// Assert an anonymous browser WebSocket Listen (no auth offer) is refused at
/// the #24 gate: the connection opens but the add-target closes with a policy
/// frame. The non-vacuous half of every migrated WebSocket Listen test.
async fn assert_firebase_listen_ws_anonymous_refused(
    server: &ServerFixture,
    parent: &str,
    collection_id: &str,
) {
    let mut request = server
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("anonymous browser websocket request should build");
    request.headers_mut().insert(
        header::ORIGIN,
        axum::http::HeaderValue::from_static("http://localhost:5173"),
    );
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        axum::http::HeaderValue::from_static("nimbus.firebase.listen.v1"),
    );
    let mut socket = WebSocketFixture::connect_request(request)
        .await
        .expect("anonymous browser websocket should connect before target admission");
    socket
        .send_binary(grpc_listen_query_request(91, parent, collection_id).encode_to_vec())
        .await;
    let close = socket.next_message().await;
    let WsMessage::Close(Some(frame)) = close else {
        panic!("anonymous WebSocket Listen must close with a policy frame, got {close:?}");
    };
    assert_eq!(frame.code, WsCloseCode::Policy);
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

struct HeldJsonPostRequest {
    handle: tokio::task::JoinHandle<Result<StatusCode, reqwest::Error>>,
}

impl Drop for HeldJsonPostRequest {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn open_json_post_stream(
    server: &ServerFixture,
    path: &str,
    body: &serde_json::Value,
) -> HeldJsonPostRequest {
    let client = server.client().clone();
    let url = server.http_url(path);
    let body = body.clone();
    let handle = tokio::spawn(async move {
        client
            .post(url)
            .json(&body)
            .send()
            .await
            .map(|response| response.status())
    });
    tokio::task::yield_now().await;
    if handle.is_finished() {
        let outcome = handle
            .await
            .expect("held JSON POST task should not panic before returning");
        panic!("JSON POST request completed before test could hold it open: {outcome:?}");
    }
    HeldJsonPostRequest { handle }
}

async fn wait_for_runtime_metrics(
    registry: &ConvexRegistry,
    description: &str,
    predicate: impl Fn(&nimbus_runtime::RuntimeMetricsSnapshot) -> bool,
) -> RuntimeMetricsSnapshot {
    wait_for_runtime_metrics_case_impl(registry, description.to_string(), predicate).await
}

async fn wait_for_runtime_metrics_case(
    registry: &ConvexRegistry,
    case: DeterministicTestCase,
    description: &str,
    predicate: impl Fn(&nimbus_runtime::RuntimeMetricsSnapshot) -> bool,
) -> RuntimeMetricsSnapshot {
    wait_for_runtime_metrics_case_impl(registry, case.failure_context(description), predicate).await
}

async fn wait_for_runtime_metrics_case_impl(
    registry: &ConvexRegistry,
    description: String,
    predicate: impl Fn(&RuntimeMetricsSnapshot) -> bool,
) -> RuntimeMetricsSnapshot {
    // Synchronization budget for a runtime-metrics condition, not a behavioral
    // assertion. The first dispatch can pay multi-shard CI cold-start costs
    // (worker-thread spawn, per-job tokio runtime build, and first V8 isolate
    // warm-up). Timeout diagnostics include the last snapshot so failures show
    // whether work was never routed, merely queued, or rejected early.
    let timeout = Duration::from_secs(60);
    let poll_interval = Duration::from_millis(25);
    let started_at = tokio::time::Instant::now();
    let mut attempts = 0_u64;
    loop {
        attempts += 1;
        let metrics = registry.runtime_metrics_snapshot();
        if predicate(&metrics) {
            return metrics;
        }
        let elapsed = started_at.elapsed();
        if elapsed >= timeout {
            let lanes = registry
                .runtime_lane_diagnostics()
                .into_iter()
                .map(|lane| {
                    let metrics = lane.metrics;
                    format!(
                        "{}: default={}, linked={:?}, started={}, active={}, queued={}, dispatched={}, completed={}, canceled={}, rejected={}, correlations={}",
                        lane.lane_name,
                        lane.default_lane,
                        lane.execution_adapter_state,
                        lane.executor_started,
                        metrics.active_runtime_instances,
                        metrics.queued_invocations,
                        metrics.worker_dispatched_invocations,
                        metrics.completed_invocations,
                        metrics.canceled_invocations,
                        metrics.rejected_invocations,
                        metrics.request_correlation_records
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            panic!(
                "timed out waiting for {description} after {elapsed:?} \
                 (budget {timeout:?}, poll interval {poll_interval:?}, attempts {attempts}, \
                 last runtime metrics: {}, runtime lanes: {lanes})",
                runtime_metrics_test_summary(&metrics)
            );
        }
        tokio::time::sleep(poll_interval).await;
    }
}

fn runtime_metrics_test_summary(metrics: &RuntimeMetricsSnapshot) -> String {
    format!(
        "active={}, queued={}, dispatched={}, router_dispatches={}, started={}, completed={}, canceled={}, rejected={}, pool_hits={}, pool_misses={}, pool_replacements={}, correlations={}, tenants={:?}",
        metrics.active_runtime_instances,
        metrics.queued_invocations,
        metrics.worker_dispatched_invocations,
        metrics.worker_router_dispatches,
        metrics.started_invocations,
        metrics.completed_invocations,
        metrics.canceled_invocations,
        metrics.rejected_invocations,
        metrics.runtime_pool_hits,
        metrics.runtime_pool_misses,
        metrics.runtime_pool_replacements,
        metrics.request_correlation_records,
        metrics.tenants.keys().collect::<Vec<_>>()
    )
}

#[path = "tests/auth_fixtures/mod.rs"]
mod auth_fixtures;

#[path = "tests/auth.rs"]
mod auth;
#[path = "tests/convex_functions.rs"]
mod convex_functions;
#[path = "tests/convex_runtime.rs"]
mod convex_runtime;
#[path = "tests/convex_tenant_exposure.rs"]
mod convex_tenant_exposure;
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
