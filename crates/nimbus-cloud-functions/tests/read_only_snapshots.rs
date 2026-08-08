use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_cloud_functions::http::{CloudFunctionsHttpInvocation, execute_http_target};
use nimbus_cloud_functions::{
    CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE, CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR,
    CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE, CloudFunctionsArtifactManifest,
    CloudFunctionsAuthoringSurface, CloudFunctionsExecutionPrincipal, CloudFunctionsHttpExposure,
    CloudFunctionsHttpTenantBinding, CloudFunctionsRegistry, CloudFunctionsRuntimeContext,
    CloudFunctionsRuntimeInvocation, CloudFunctionsRuntimeInvoker, CloudFunctionsSignatureType,
    CloudFunctionsTargetBinding, CloudFunctionsTargetDefinition, CloudFunctionsTargetsManifest,
    CloudFunctionsTriggerExecutor, build_callable_request_args,
};
use nimbus_core::{
    Document, DocumentEventData, DocumentEventDocument, DocumentPath, FirestoreCloudEventType,
    FirestoreTriggerMetadata, PrincipalContext, Result, SequenceNumber, TableName, TenantId,
    Timestamp, TriggerCloudEvent, TriggerCommitMetadata, TriggerEvent, TriggerExecutionPrincipal,
    TriggerInvocationKey, TriggerInvocationRecord,
};
use nimbus_engine::{Engine, TriggerInvocationExecution, TriggerInvocationExecutor};
use nimbus_runtime::{
    InvocationServiceBinding, InvocationServiceProtocol, InvocationServices, RuntimeBundle,
    RuntimeLimits, RuntimePolicy,
};
use nimbus_services::RuntimeServiceRegistry;
use nimbus_tenant::TenantIsolationMode;
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

struct ReadOnlyCloudFunctionsRegistry {
    expected_tenant: TenantId,
    snapshot_calls: AtomicUsize,
    forbidden_non_snapshot_calls: AtomicUsize,
}

impl RuntimeServiceRegistry for ReadOnlyCloudFunctionsRegistry {
    fn snapshot_for_tenant(&self, tenant_id: &TenantId) -> InvocationServices {
        assert_eq!(tenant_id, &self.expected_tenant);
        self.snapshot_calls.fetch_add(1, Ordering::SeqCst);
        InvocationServices::from([(
            "db".to_owned(),
            InvocationServiceBinding {
                host: "127.0.0.1".to_owned(),
                port: 15432,
                protocol: InvocationServiceProtocol::Tcp,
                endpoints: BTreeMap::new(),
            },
        )])
    }

    fn resolve_service_binding(
        &self,
        _tenant_id: &TenantId,
        _service_name: &str,
    ) -> Result<Option<InvocationServiceBinding>> {
        self.forbidden_non_snapshot_calls
            .fetch_add(1, Ordering::SeqCst);
        panic!("Cloud Functions invocation snapshots must not perform a service lookup")
    }
}

#[derive(Debug)]
struct CapturedInvocation {
    tenant_id: TenantId,
    deployment_generation: u64,
    function_name: String,
    args: Value,
    services: InvocationServices,
}

#[derive(Default)]
struct RecordingRuntimeInvoker {
    invocations: Mutex<Vec<CapturedInvocation>>,
}

impl CloudFunctionsRuntimeInvoker for RecordingRuntimeInvoker {
    fn runtime_policy(&self, limits: &RuntimeLimits) -> Arc<RuntimePolicy> {
        Arc::new(RuntimePolicy::new(limits.clone()))
    }

    fn invoke_runtime_bundle(&self, invocation: CloudFunctionsRuntimeInvocation) -> Result<Value> {
        let function_name = invocation.request.function_name.clone();
        self.invocations
            .lock()
            .expect("invocation capture lock should not be poisoned")
            .push(CapturedInvocation {
                tenant_id: invocation.tenant_id,
                deployment_generation: invocation.deployment_generation,
                function_name: function_name.clone(),
                args: invocation.request.args,
                services: invocation.request.services,
            });
        Ok(json!({
            "status": 200,
            "body_kind": "json",
            "body": { "function": function_name },
        }))
    }
}

#[test]
fn cloud_functions_snapshots_have_zero_activation_store_or_provider_calls_for_http_and_callable() {
    let fixture = CloudFunctionsFixture::new(http_and_callable_targets());
    let http_response = fixture.execute_http(
        "exports.http",
        json!({ "transport": "http" }),
        "request-http",
    );
    assert_eq!(http_response.status.as_u16(), 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&http_response.body)
            .expect("HTTP response body should decode"),
        json!({ "function": "exports.http" })
    );

    let mut callable_headers = http::HeaderMap::new();
    callable_headers.insert(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    let callable_args = build_callable_request_args(
        &callable_headers,
        None,
        "/callable",
        HashMap::new(),
        br#"{"data":{"name":"Nimbus"}}"#,
        None,
    )
    .expect("callable request should build");
    let callable_response =
        fixture.execute_http("exports.callable", callable_args, "request-callable");
    assert_eq!(callable_response.status.as_u16(), 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&callable_response.body)
            .expect("callable response body should decode"),
        json!({ "function": "exports.callable" })
    );

    let invocations = fixture
        .runtime_invoker
        .invocations
        .lock()
        .expect("invocation capture lock should not be poisoned");
    assert_eq!(invocations.len(), 2);
    for invocation in invocations.iter() {
        assert_eq!(invocation.tenant_id, fixture.tenant_id);
        assert_eq!(invocation.deployment_generation, 7);
        let db = invocation
            .services
            .get("db")
            .expect("read-only snapshot should be passed to the runtime");
        assert_eq!(db.port, 15432);
    }
    assert_eq!(invocations[0].function_name, "exports.http");
    assert_eq!(invocations[0].args, json!({ "transport": "http" }));
    assert_eq!(invocations[1].function_name, "exports.callable");
    assert_eq!(
        invocations[1].args["callable"]["data"],
        json!({ "name": "Nimbus" })
    );
    drop(invocations);

    assert_eq!(
        fixture
            .service_registry
            .snapshot_calls
            .load(Ordering::SeqCst),
        2,
        "HTTP and callable invocations should each take one read-only snapshot"
    );
    assert_eq!(
        fixture
            .service_registry
            .forbidden_non_snapshot_calls
            .load(Ordering::SeqCst),
        0,
        "HTTP and callable invocations must stay on the read-only snapshot projection; the registry exposes no lifecycle capability"
    );
}

#[test]
fn cloud_functions_snapshots_have_zero_activation_store_or_provider_calls_for_trigger_and_unknown_target()
 {
    let fixture = CloudFunctionsFixture::new(trigger_targets());
    let executor = CloudFunctionsTriggerExecutor::new(
        fixture.engine.clone(),
        fixture.registry.clone(),
        7,
        fixture.service_registry.clone(),
        TenantIsolationMode::LocalDevelopment,
        fixture.runtime_invoker.clone(),
    );
    let users = TableName::new("users").expect("users table should parse");
    let user_id = nimbus_core::DocumentId::from_key("alice").expect("user id should parse");

    let completed = executor.execute_invocation(
        &fixture.tenant_id,
        &sample_trigger_record("syncUser", &users, &user_id),
    );
    assert_eq!(completed, TriggerInvocationExecution::completed());
    assert_eq!(
        fixture
            .service_registry
            .snapshot_calls
            .load(Ordering::SeqCst),
        1
    );
    assert_eq!(
        fixture
            .runtime_invoker
            .invocations
            .lock()
            .expect("invocation capture lock should not be poisoned")
            .len(),
        1
    );

    let missing = executor.execute_invocation(
        &fixture.tenant_id,
        &sample_trigger_record("missing", &users, &user_id),
    );
    assert!(
        matches!(
            missing,
            TriggerInvocationExecution::TerminalFailure { ref error }
                if error.contains("trigger target `missing` is not present")
        ),
        "unknown target should fail before any snapshot or runtime dispatch: {missing:?}"
    );
    assert_eq!(
        fixture
            .service_registry
            .snapshot_calls
            .load(Ordering::SeqCst),
        1,
        "unknown target must fail before taking a snapshot"
    );
    assert_eq!(
        fixture
            .runtime_invoker
            .invocations
            .lock()
            .expect("invocation capture lock should not be poisoned")
            .len(),
        1,
        "unknown target must fail before runtime dispatch"
    );
    assert_eq!(
        fixture
            .service_registry
            .forbidden_non_snapshot_calls
            .load(Ordering::SeqCst),
        0,
        "trigger success and refusal must stay on the read-only snapshot projection; the registry exposes no lifecycle capability"
    );
}

struct CloudFunctionsFixture {
    engine: Arc<Engine>,
    registry: Arc<CloudFunctionsRegistry>,
    tenant_id: TenantId,
    service_registry: Arc<ReadOnlyCloudFunctionsRegistry>,
    runtime_invoker: Arc<RecordingRuntimeInvoker>,
    _engine_dir: TempDir,
    _app_dir: TempDir,
}

impl CloudFunctionsFixture {
    fn new(targets: Vec<CloudFunctionsTargetDefinition>) -> Self {
        let engine_dir = tempdir().expect("engine tempdir should build");
        let engine = Arc::new(Engine::new(engine_dir.path()).expect("engine should build"));
        let tenant_id = TenantId::new("tenant-read-only").expect("tenant id should parse");
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(app_dir.path(), &targets);
        let registry = Arc::new(
            CloudFunctionsRegistry::from_app_dir(app_dir.path())
                .expect("Cloud Functions registry should load"),
        );
        let service_registry = Arc::new(ReadOnlyCloudFunctionsRegistry {
            expected_tenant: tenant_id.clone(),
            snapshot_calls: AtomicUsize::new(0),
            forbidden_non_snapshot_calls: AtomicUsize::new(0),
        });
        Self {
            engine,
            registry,
            tenant_id,
            service_registry,
            runtime_invoker: Arc::new(RecordingRuntimeInvoker::default()),
            _engine_dir: engine_dir,
            _app_dir: app_dir,
        }
    }

    fn execute_http(
        &self,
        function_name: &str,
        args: Value,
        server_request_id: &str,
    ) -> nimbus_cloud_functions::CloudFunctionsHttpResponseParts {
        execute_http_target(
            CloudFunctionsRuntimeContext::new(
                self.engine.clone(),
                self.service_registry.clone(),
                TenantIsolationMode::LocalDevelopment,
                self.runtime_invoker.clone(),
            ),
            CloudFunctionsHttpInvocation {
                registry: self.registry.clone(),
                deployment_generation: 7,
                tenant_binding: CloudFunctionsHttpTenantBinding::new(self.tenant_id.clone())
                    .expect("tenant binding should build"),
                function_name: function_name.to_owned(),
                args,
                auth: None,
                server_request_id: server_request_id.to_owned(),
            },
        )
        .expect("Cloud Functions HTTP invocation should succeed")
    }
}

fn http_and_callable_targets() -> Vec<CloudFunctionsTargetDefinition> {
    vec![
        CloudFunctionsTargetDefinition {
            name: "http".to_owned(),
            entrypoint: "exports.http".to_owned(),
            authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
            signature_type: CloudFunctionsSignatureType::Http,
            binding: CloudFunctionsTargetBinding::Https {
                exposure: CloudFunctionsHttpExposure::Http,
                path: "/http".to_owned(),
                execution: CloudFunctionsExecutionPrincipal::RequestPrincipal,
            },
        },
        CloudFunctionsTargetDefinition {
            name: "callable".to_owned(),
            entrypoint: "exports.callable".to_owned(),
            authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
            signature_type: CloudFunctionsSignatureType::Http,
            binding: CloudFunctionsTargetBinding::Https {
                exposure: CloudFunctionsHttpExposure::Callable,
                path: "/callable".to_owned(),
                execution: CloudFunctionsExecutionPrincipal::RequestPrincipal,
            },
        },
    ]
}

fn trigger_targets() -> Vec<CloudFunctionsTargetDefinition> {
    vec![CloudFunctionsTargetDefinition {
        name: "syncUser".to_owned(),
        entrypoint: "exports.syncUser".to_owned(),
        authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
        signature_type: CloudFunctionsSignatureType::CloudEvent,
        binding: CloudFunctionsTargetBinding::FirestoreDocument {
            event_type: FirestoreCloudEventType::Written,
            database: "(default)".to_owned(),
            document: "users/{userId}".to_owned(),
            namespace: None,
            execution: CloudFunctionsExecutionPrincipal::ServiceAccount,
        },
    }]
}

fn sample_trigger_record(
    registration_id: &str,
    table: &TableName,
    document_id: &nimbus_core::DocumentId,
) -> TriggerInvocationRecord {
    let document_path = DocumentPath::from_segments([table.as_str(), document_id.as_str()])
        .expect("document path should parse");
    let after = Document::with_id(
        document_id.clone(),
        table.clone(),
        serde_json::Map::from_iter([("name".to_owned(), json!("after"))]),
    );
    TriggerInvocationRecord::pending(
        TriggerInvocationKey::new(registration_id, "event-1")
            .expect("trigger invocation key should parse"),
        SequenceNumber(1),
        TriggerEvent::new(
            TriggerCloudEvent::new(
                "event-1",
                "//firestore.googleapis.com/projects/demo/databases/(default)",
                FirestoreCloudEventType::Written,
                Timestamp(1),
                format!("documents/{document_path}"),
            ),
            FirestoreTriggerMetadata::new(
                "demo",
                "(default)",
                document_path.clone(),
                BTreeMap::from([("userId".to_owned(), document_id.to_string())]),
            ),
            DocumentEventData::new(
                Some(DocumentEventDocument::new(document_path, after)),
                None,
                None,
            ),
            TriggerCommitMetadata::new(SequenceNumber(1), Timestamp(1)),
            TriggerExecutionPrincipal::service_account(PrincipalContext::system()),
        ),
    )
}

fn write_cloud_functions_artifact(app_dir: &Path, targets: &[CloudFunctionsTargetDefinition]) {
    let artifact_dir = app_dir.join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR);
    fs::create_dir_all(&artifact_dir).expect("artifact directory should create");
    fs::write(
        artifact_dir.join(CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE),
        serde_json::to_vec_pretty(&CloudFunctionsArtifactManifest::v1())
            .expect("artifact manifest should encode"),
    )
    .expect("artifact manifest should write");
    fs::write(
        artifact_dir.join(CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE),
        serde_json::to_vec_pretty(
            &CloudFunctionsTargetsManifest::v1(targets.to_vec()).expect("targets should validate"),
        )
        .expect("targets should encode"),
    )
    .expect("targets should write");
    let bundle_path = artifact_dir.join("bundle.mjs");
    fs::write(&bundle_path, "export {};").expect("bundle should write");
    let digest =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle digest should compute");
    fs::write(bundle_path.with_extension("sha256"), format!("{digest}\n"))
        .expect("bundle digest should write");
}
