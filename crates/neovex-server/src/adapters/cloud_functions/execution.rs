use std::sync::Arc;

use neovex_core::{Error, Result, StorageErrorKind, TenantId, TriggerInvocationRecord};
use neovex_engine::{Service, TriggerInvocationExecution, TriggerInvocationExecutor};
use neovex_runtime::{InvocationKind, InvocationRequest};

use super::host_bridge::CloudFunctionsHostBridge;
use super::registry::CloudFunctionsRegistry;
use crate::execution::errors::runtime_error_to_core;
use crate::execution::invocations::{
    RuntimeBundleInvocationOptions, invoke_runtime_bundle_blocking_with_host,
    next_runtime_server_request_id,
};
use crate::runtime_host::{RuntimeHostInvocation, RuntimeHostScope};
use crate::service_registry::RuntimeServiceRegistry;

pub(crate) struct CloudFunctionsTriggerExecutor {
    service: Arc<Service>,
    registry: Arc<CloudFunctionsRegistry>,
    runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
}

impl CloudFunctionsTriggerExecutor {
    pub(crate) fn new(
        service: Arc<Service>,
        registry: Arc<CloudFunctionsRegistry>,
        runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    ) -> Self {
        Self {
            service,
            registry,
            runtime_service_registry,
        }
    }
}

impl TriggerInvocationExecutor for CloudFunctionsTriggerExecutor {
    fn execute_invocation(
        &self,
        tenant_id: &TenantId,
        record: &TriggerInvocationRecord,
    ) -> TriggerInvocationExecution {
        match self.execute_invocation_once(tenant_id, record) {
            Ok(()) => TriggerInvocationExecution::completed(),
            Err(error) => classify_cloud_functions_trigger_error(error),
        }
    }
}

impl CloudFunctionsTriggerExecutor {
    fn execute_invocation_once(
        &self,
        tenant_id: &TenantId,
        record: &TriggerInvocationRecord,
    ) -> Result<()> {
        let target = self
            .registry
            .required_firestore_trigger_target(&record.key.registration_id)?;
        let args = serde_json::to_value(&record.event)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let server_request_id = next_runtime_server_request_id("cloud-functions-trigger");
        let services = self.runtime_service_registry.snapshot_for_tenant(tenant_id);
        let request = InvocationRequest {
            kind: InvocationKind::Mutation,
            function_name: target.entrypoint.clone(),
            args,
            page_size: None,
            cursor: None,
            auth: None,
            services: services.clone(),
        };
        let bridge = Arc::new(CloudFunctionsHostBridge::build(
            RuntimeHostScope::new(
                self.service.clone(),
                self.registry.runtime_policy(),
                tenant_id.clone(),
            ),
            RuntimeHostInvocation::new(
                record.event.execution.principal().clone(),
                Some(server_request_id.clone()),
                InvocationKind::Mutation,
            )
            .with_trigger_write_origin(neovex_core::TriggerWriteOrigin::new(
                record.key.clone(),
                record.depth(),
            )),
        )?);

        invoke_runtime_bundle_blocking_with_host(
            &self.registry.runtime_executor(),
            self.registry.runtime_policy(),
            bridge.clone(),
            self.registry.runtime_bundle(),
            request,
            RuntimeBundleInvocationOptions::enforcing_policy_limit(
                tenant_id,
                Some(server_request_id.as_str()),
                None,
            ),
        )
        .map_err(runtime_error_to_core)?;
        bridge.commit_mutation_execution_unit()?;
        Ok(())
    }
}

fn classify_cloud_functions_trigger_error(error: Error) -> TriggerInvocationExecution {
    let message = error.to_string();
    match error {
        Error::Cancelled | Error::ResourceExhausted(_) => {
            TriggerInvocationExecution::retryable(message)
        }
        Error::Storage {
            kind:
                StorageErrorKind::Busy
                | StorageErrorKind::Io
                | StorageErrorKind::Transient
                | StorageErrorKind::Unavailable,
            ..
        } => TriggerInvocationExecution::retryable(message),
        _ => TriggerInvocationExecution::terminal(message),
    }
}

#[cfg(test)]
mod tests {
    use crate::provider_family::firestore::locator_for_document_path;

    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;

    use neovex_core::{
        AtomicWrite, AtomicWriteBatch, Document, DocumentEventData, DocumentEventDocument,
        DocumentPath, FirestoreCloudEventType, FirestoreTriggerMetadata, PrincipalContext,
        ResourcePathBinding, SequenceNumber, TableName, Timestamp, TriggerCloudEvent,
        TriggerCommitMetadata, TriggerEvent, TriggerExecutionPrincipal, TriggerInvocationKey,
        WriteKey, WritePrecondition, WriteSetMode,
    };
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;
    use crate::EmptySandboxCatalog;
    use crate::adapters::cloud_functions::{
        CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE, CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR,
        CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE, CloudFunctionsArtifactManifest,
        CloudFunctionsAuthoringSurface, CloudFunctionsExecutionBinding,
        CloudFunctionsSignatureType, CloudFunctionsTargetBinding, CloudFunctionsTargetDefinition,
        CloudFunctionsTargetsManifest,
    };
    use crate::router::RouterBuildConfig;
    use crate::service_registry::SandboxCatalogRuntimeServiceRegistry;
    use neovex_testing::{ServerFixture, wait_for_value};

    #[test]
    fn cloud_functions_trigger_executor_reads_and_writes_via_runtime_bundle() {
        let service_dir = tempdir().expect("service tempdir should build");
        let service = Arc::new(Service::new(service_dir.path()).expect("service should build"));
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");

        let users = TableName::new("users").expect("users table should parse");
        let audit = TableName::new("audit").expect("audit table should parse");
        let user_id = neovex_core::DocumentId::from_key("alice").expect("user id should parse");
        service
            .insert_document_with_id(
                &tenant_id,
                users.clone(),
                user_id.clone(),
                serde_json::Map::from_iter([("name".to_string(), json!("before"))]),
            )
            .expect("seed user should insert");

        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(
            app_dir.path(),
            &[CloudFunctionsTargetDefinition {
                name: "syncUser".to_string(),
                entrypoint: "exports.syncUser".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
                signature_type: CloudFunctionsSignatureType::CloudEvent,
                binding: CloudFunctionsTargetBinding::FirestoreDocument {
                    event_type: FirestoreCloudEventType::Written,
                    database: "(default)".to_string(),
                    document: "users/{userId}".to_string(),
                    namespace: None,
                    execution: CloudFunctionsExecutionBinding::Service,
                },
            }],
            r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `trigger:${request.function_name}`,
  });
  if (request.function_name !== "exports.syncUser") {
    throw new Error(`unknown handler ${request.function_name}`);
  }
  const event = request.args;
  const userId = event.firestore.params.userId;
  const current = await ctx.db.get("users", userId);
  await ctx.db.insert("audit", {
    userId,
    previousName: current ? current.name : null,
    nextName: event.data.value ? event.data.value.document.fields.name : null,
  });
  return { ok: true };
};

export {};
"#,
        );

        let registry = Arc::new(
            CloudFunctionsRegistry::from_app_dir(app_dir.path()).expect("registry should load"),
        );
        let runtime_service_registry: Arc<dyn RuntimeServiceRegistry> = Arc::new(
            SandboxCatalogRuntimeServiceRegistry::new(Arc::new(EmptySandboxCatalog)),
        );
        let executor =
            CloudFunctionsTriggerExecutor::new(service.clone(), registry, runtime_service_registry);

        assert_eq!(
            executor.execute_invocation(
                &tenant_id,
                &sample_trigger_record("syncUser", &users, &user_id),
            ),
            TriggerInvocationExecution::completed()
        );

        let audit_documents = service
            .query_documents(
                &tenant_id,
                &neovex_core::Query {
                    table: audit,
                    filters: Vec::new(),
                    order: None,
                    limit: None,
                },
            )
            .expect("audit documents should load");
        assert_eq!(audit_documents.len(), 1);
        let audit_document = &audit_documents[0];
        assert_eq!(audit_document.get_field("userId"), Some(&json!("alice")));
        assert_eq!(
            audit_document.get_field("previousName"),
            Some(&json!("before"))
        );
        assert_eq!(audit_document.get_field("nextName"), Some(&json!("after")));
    }

    #[test]
    fn cloud_functions_trigger_executor_runs_generated_firebase_bundle_with_import_aliases() {
        let repo_root = repo_root();
        if !workspace_codegen_dependencies_available(&repo_root) {
            eprintln!(
                "skipping generated firebase cloud functions executor smoke; workspace JS dependencies are unavailable"
            );
            return;
        }

        let service_dir = tempdir().expect("service tempdir should build");
        let service = Arc::new(Service::new(service_dir.path()).expect("service should build"));
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");

        let users = TableName::new("users").expect("users table should parse");
        let audit = TableName::new("audit").expect("audit table should parse");
        let user_id = neovex_core::DocumentId::from_key("alice").expect("user id should parse");
        service
            .insert_document_with_id(
                &tenant_id,
                users.clone(),
                user_id.clone(),
                serde_json::Map::from_iter([("name".to_string(), json!("before"))]),
            )
            .expect("seed user should insert");

        let app_dir = tempdir().expect("app tempdir should build");
        write_firebase_cloud_functions_fixture(app_dir.path());
        let output = Command::new("node")
            .current_dir(&repo_root)
            .arg("./packages/codegen/src/cli.mjs")
            .arg("--app")
            .arg(app_dir.path())
            .output()
            .expect("cloud functions codegen should run");
        assert!(
            output.status.success(),
            "cloud functions codegen should pass\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let registry = Arc::new(
            CloudFunctionsRegistry::from_app_dir(app_dir.path()).expect("registry should load"),
        );
        let runtime_service_registry: Arc<dyn RuntimeServiceRegistry> = Arc::new(
            SandboxCatalogRuntimeServiceRegistry::new(Arc::new(EmptySandboxCatalog)),
        );
        let executor =
            CloudFunctionsTriggerExecutor::new(service.clone(), registry, runtime_service_registry);

        assert_eq!(
            executor.execute_invocation(
                &tenant_id,
                &sample_trigger_record("syncUser", &users, &user_id),
            ),
            TriggerInvocationExecution::completed()
        );

        let audit_documents = service
            .query_documents(
                &tenant_id,
                &neovex_core::Query {
                    table: audit,
                    filters: Vec::new(),
                    order: None,
                    limit: None,
                },
            )
            .expect("audit documents should load");
        assert_eq!(audit_documents.len(), 1);
        let audit_document = &audit_documents[0];
        assert_eq!(audit_document.get_field("userId"), Some(&json!("alice")));
        assert_eq!(audit_document.get_field("retry"), Some(&json!(true)));
        assert_eq!(audit_document.get_field("nextName"), Some(&json!("after")));
    }

    #[test]
    fn cloud_functions_trigger_executor_runs_generated_firebase_bundle_with_admin_firestore_operations()
     {
        let repo_root = repo_root();
        if !workspace_codegen_dependencies_available(&repo_root) {
            eprintln!(
                "skipping generated firebase admin cloud functions executor smoke; workspace JS dependencies are unavailable"
            );
            return;
        }

        let service_dir = tempdir().expect("service tempdir should build");
        let service = Arc::new(Service::new(service_dir.path()).expect("service should build"));
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");

        seed_firebase_document(
            &service,
            &tenant_id,
            "users/alice",
            serde_json::Map::from_iter([(
                "profile".to_string(),
                json!({
                    "name": "before",
                }),
            )]),
        );
        seed_firebase_document(
            &service,
            &tenant_id,
            "trash/alice",
            serde_json::Map::from_iter([("stale".to_string(), json!(true))]),
        );

        let app_dir = tempdir().expect("app tempdir should build");
        write_firebase_admin_cloud_functions_fixture(app_dir.path());
        let output = Command::new("node")
            .current_dir(&repo_root)
            .arg("./packages/codegen/src/cli.mjs")
            .arg("--app")
            .arg(app_dir.path())
            .output()
            .expect("cloud functions codegen should run");
        assert!(
            output.status.success(),
            "cloud functions codegen should pass\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let registry = Arc::new(
            CloudFunctionsRegistry::from_app_dir(app_dir.path()).expect("registry should load"),
        );
        let runtime_service_registry: Arc<dyn RuntimeServiceRegistry> = Arc::new(
            SandboxCatalogRuntimeServiceRegistry::new(Arc::new(EmptySandboxCatalog)),
        );
        let executor =
            CloudFunctionsTriggerExecutor::new(service.clone(), registry, runtime_service_registry);

        let users = TableName::new("users").expect("users table should parse");
        let user_id = neovex_core::DocumentId::from_key("alice").expect("user id should parse");
        assert_eq!(
            executor.execute_invocation(
                &tenant_id,
                &sample_trigger_record("syncUser", &users, &user_id),
            ),
            TriggerInvocationExecution::completed()
        );

        let audit_document = firebase_document(&service, &tenant_id, "audit/alice")
            .expect("audit read should succeed")
            .expect("audit document should exist");
        assert_eq!(
            audit_document.get_field("beforeName"),
            Some(&json!("before"))
        );
        assert_eq!(audit_document.get_field("recordedAt"), Some(&json!(42)));

        let updated_user = firebase_document(&service, &tenant_id, "users/alice")
            .expect("user read should succeed")
            .expect("updated user should exist");
        assert_eq!(updated_user.get_field("processed"), Some(&json!(true)));
        assert_eq!(
            updated_user.get_field("profile"),
            Some(&json!({
                "name": "before",
            }))
        );

        assert_eq!(
            firebase_document(&service, &tenant_id, "trash/alice")
                .expect("trash read should succeed"),
            None
        );
    }

    #[tokio::test]
    async fn cloud_functions_trigger_lifecycle_runs_generated_firebase_bundle_after_committed_write()
     {
        let repo_root = repo_root();
        if !workspace_codegen_dependencies_available(&repo_root) {
            eprintln!(
                "skipping generated firebase cloud functions lifecycle smoke; workspace JS dependencies are unavailable"
            );
            return;
        }

        let service_dir = tempdir().expect("service tempdir should build");
        let service = Arc::new(Service::new(service_dir.path()).expect("service should build"));
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");

        let app_dir = tempdir().expect("app tempdir should build");
        write_firebase_admin_trigger_lifecycle_fixture(app_dir.path());
        let output = run_cloud_functions_codegen(app_dir.path());
        assert!(
            output.status.success(),
            "cloud functions codegen should pass\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let _server = ServerFixture::start(
            RouterBuildConfig::core(service.clone())
                .with_cloud_functions(registry)
                .build(),
        )
        .await;

        seed_firebase_document(
            &service,
            &tenant_id,
            "users/alice",
            serde_json::Map::from_iter([(
                "profile".to_string(),
                json!({
                    "name": "before",
                }),
            )]),
        );

        let audit_document = wait_for_value(
            "firebase cloud functions trigger should write audit document",
            Duration::from_secs(5),
            Duration::from_millis(20),
            || async {
                firebase_document(&service, &tenant_id, "audit/alice")
                    .expect("audit read should succeed")
            },
            |document| document.is_some(),
        )
        .await
        .expect("audit document should exist");

        assert_eq!(audit_document.get_field("retry"), Some(&json!(true)));
        assert_eq!(
            audit_document.get_field("type"),
            Some(&json!("google.cloud.firestore.document.v1.written"))
        );
        assert_eq!(audit_document.get_field("name"), Some(&json!("before")));
    }

    #[tokio::test]
    async fn cloud_functions_trigger_lifecycle_runs_generated_framework_bundle_after_committed_write()
     {
        let repo_root = repo_root();
        if !workspace_codegen_dependencies_available(&repo_root) {
            eprintln!(
                "skipping generated functions-framework lifecycle smoke; workspace JS dependencies are unavailable"
            );
            return;
        }

        let service_dir = tempdir().expect("service tempdir should build");
        let service = Arc::new(Service::new(service_dir.path()).expect("service should build"));
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");

        let app_dir = tempdir().expect("app tempdir should build");
        write_framework_admin_trigger_lifecycle_fixture(app_dir.path());
        let output = run_cloud_functions_codegen(app_dir.path());
        assert!(
            output.status.success(),
            "cloud functions codegen should pass\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let _server = ServerFixture::start(
            RouterBuildConfig::core(service.clone())
                .with_cloud_functions(registry)
                .build(),
        )
        .await;

        seed_firebase_document(
            &service,
            &tenant_id,
            "users/alice",
            serde_json::Map::from_iter([(
                "profile".to_string(),
                json!({
                    "name": "before",
                }),
            )]),
        );

        let audit_document = wait_for_value(
            "functions-framework trigger should write audit document",
            Duration::from_secs(5),
            Duration::from_millis(20),
            || async {
                firebase_document(&service, &tenant_id, "audit/alice")
                    .expect("audit read should succeed")
            },
            |document| document.is_some(),
        )
        .await
        .expect("audit document should exist");

        assert_eq!(
            audit_document.get_field("type"),
            Some(&json!("google.cloud.firestore.document.v1.written"))
        );
        assert_eq!(audit_document.get_field("name"), Some(&json!("before")));
        assert_eq!(
            audit_document.get_field("subject"),
            Some(&json!("documents/users/alice"))
        );
    }

    #[tokio::test]
    async fn cloud_functions_trigger_lifecycle_suppresses_noop_updates_before_real_updated_events()
    {
        let repo_root = repo_root();
        if !workspace_codegen_dependencies_available(&repo_root) {
            eprintln!(
                "skipping generated firebase noop-update lifecycle smoke; workspace JS dependencies are unavailable"
            );
            return;
        }

        let service_dir = tempdir().expect("service tempdir should build");
        let service = Arc::new(Service::new(service_dir.path()).expect("service should build"));
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");

        let app_dir = tempdir().expect("app tempdir should build");
        write_firebase_updated_noop_lifecycle_fixture(app_dir.path());
        let output = run_cloud_functions_codegen(app_dir.path());
        assert!(
            output.status.success(),
            "cloud functions codegen should pass\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let _server = ServerFixture::start(
            RouterBuildConfig::core(service.clone())
                .with_cloud_functions(registry)
                .build(),
        )
        .await;

        seed_firebase_document(
            &service,
            &tenant_id,
            "users/alice",
            serde_json::Map::from_iter([(
                "profile".to_string(),
                json!({
                    "name": "before",
                }),
            )]),
        );
        seed_firebase_document(
            &service,
            &tenant_id,
            "users/alice",
            serde_json::Map::from_iter([(
                "profile".to_string(),
                json!({
                    "name": "before",
                }),
            )]),
        );

        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(
            table_documents(&service, &tenant_id, "audit")
                .expect("audit query should succeed")
                .len(),
            0,
            "no-op overwrite should not emit an updated trigger"
        );

        seed_firebase_document(
            &service,
            &tenant_id,
            "users/alice",
            serde_json::Map::from_iter([(
                "profile".to_string(),
                json!({
                    "name": "after",
                }),
            )]),
        );

        wait_for_value(
            "real update should emit exactly one audit document",
            Duration::from_secs(5),
            Duration::from_millis(20),
            || async {
                table_documents(&service, &tenant_id, "audit")
                    .expect("audit query should succeed")
                    .len()
            },
            |count| *count >= 1,
        )
        .await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        let audit_documents =
            table_documents(&service, &tenant_id, "audit").expect("audit query should succeed");
        assert_eq!(audit_documents.len(), 1);
        assert_eq!(
            audit_documents[0].get_field("userId"),
            Some(&json!("alice"))
        );
        assert_eq!(audit_documents[0].get_field("name"), Some(&json!("after")));
    }

    #[tokio::test]
    async fn cloud_functions_trigger_lifecycle_processes_concurrent_writes() {
        let repo_root = repo_root();
        if !workspace_codegen_dependencies_available(&repo_root) {
            eprintln!(
                "skipping generated firebase concurrent-trigger lifecycle smoke; workspace JS dependencies are unavailable"
            );
            return;
        }

        let service_dir = tempdir().expect("service tempdir should build");
        let service = Arc::new(Service::new(service_dir.path()).expect("service should build"));
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");

        let app_dir = tempdir().expect("app tempdir should build");
        write_firebase_concurrent_trigger_lifecycle_fixture(app_dir.path());
        let output = run_cloud_functions_codegen(app_dir.path());
        assert!(
            output.status.success(),
            "cloud functions codegen should pass\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let _server = ServerFixture::start(
            RouterBuildConfig::core(service.clone())
                .with_cloud_functions(registry)
                .build(),
        )
        .await;

        let tenant_for_alice = tenant_id.clone();
        let service_for_alice = service.clone();
        let alice = tokio::task::spawn_blocking(move || {
            seed_firebase_document(
                &service_for_alice,
                &tenant_for_alice,
                "users/alice",
                serde_json::Map::from_iter([("name".to_string(), json!("alice"))]),
            );
        });
        let tenant_for_bob = tenant_id.clone();
        let service_for_bob = service.clone();
        let bob = tokio::task::spawn_blocking(move || {
            seed_firebase_document(
                &service_for_bob,
                &tenant_for_bob,
                "users/bob",
                serde_json::Map::from_iter([("name".to_string(), json!("bob"))]),
            );
        });
        alice.await.expect("alice seed task should join");
        bob.await.expect("bob seed task should join");

        wait_for_value(
            "concurrent trigger writes should both materialize audit rows",
            Duration::from_secs(5),
            Duration::from_millis(20),
            || async {
                table_documents(&service, &tenant_id, "audit")
                    .expect("audit query should succeed")
                    .len()
            },
            |count| *count == 2,
        )
        .await;

        let mut seen_user_ids = table_documents(&service, &tenant_id, "audit")
            .expect("audit query should succeed")
            .into_iter()
            .filter_map(|document| {
                document
                    .get_field("userId")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .collect::<Vec<_>>();
        seen_user_ids.sort();
        assert_eq!(seen_user_ids, vec!["alice".to_string(), "bob".to_string()]);
    }

    #[tokio::test]
    async fn cloud_functions_trigger_lifecycle_enforces_chain_depth_limit() {
        let repo_root = repo_root();
        if !workspace_codegen_dependencies_available(&repo_root) {
            eprintln!(
                "skipping generated firebase chain-depth lifecycle smoke; workspace JS dependencies are unavailable"
            );
            return;
        }

        let service_dir = tempdir().expect("service tempdir should build");
        let service = Arc::new(Service::new(service_dir.path()).expect("service should build"));
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");

        let app_dir = tempdir().expect("app tempdir should build");
        write_firebase_chain_depth_lifecycle_fixture(app_dir.path());
        let output = run_cloud_functions_codegen(app_dir.path());
        assert!(
            output.status.success(),
            "cloud functions codegen should pass\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let _server = ServerFixture::start(
            RouterBuildConfig::core(service.clone())
                .with_cloud_functions(registry)
                .build(),
        )
        .await;

        seed_firebase_document(
            &service,
            &tenant_id,
            "chain/step0",
            serde_json::Map::from_iter([("step".to_string(), json!(0))]),
        );

        wait_for_value(
            "chain trigger should stop at the configured depth budget",
            Duration::from_secs(5),
            Duration::from_millis(20),
            || async {
                table_documents(&service, &tenant_id, "audit")
                    .expect("audit query should succeed")
                    .len()
            },
            |count| *count == 9,
        )
        .await;
        tokio::time::sleep(Duration::from_millis(150)).await;

        assert_eq!(
            table_documents(&service, &tenant_id, "audit")
                .expect("audit query should succeed")
                .len(),
            9,
            "depth-limited trigger chain should only execute nine handlers (root plus depth 1-8)"
        );
        assert!(
            firebase_document(&service, &tenant_id, "chain/step9")
                .expect("chain step9 read should succeed")
                .is_some(),
            "the last committed child document should still be written before the next over-depth trigger is suppressed"
        );
        assert!(
            firebase_document(&service, &tenant_id, "chain/step10")
                .expect("chain step10 read should succeed")
                .is_none(),
            "no trigger should run beyond the configured chain depth budget"
        );
    }

    #[test]
    fn cloud_functions_trigger_executor_reports_missing_runtime_handler() {
        let service_dir = tempdir().expect("service tempdir should build");
        let service = Arc::new(Service::new(service_dir.path()).expect("service should build"));
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");

        let users = TableName::new("users").expect("users table should parse");
        let user_id = neovex_core::DocumentId::from_key("alice").expect("user id should parse");
        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(
            app_dir.path(),
            &[CloudFunctionsTargetDefinition {
                name: "syncUser".to_string(),
                entrypoint: "exports.missing".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
                signature_type: CloudFunctionsSignatureType::CloudEvent,
                binding: CloudFunctionsTargetBinding::FirestoreDocument {
                    event_type: FirestoreCloudEventType::Written,
                    database: "(default)".to_string(),
                    document: "users/{userId}".to_string(),
                    namespace: None,
                    execution: CloudFunctionsExecutionBinding::Service,
                },
            }],
            r#"
globalThis.__neovexInvoke = async function (request) {
  if (request.function_name !== "exports.syncUser") {
    throw new Error(`unknown handler ${request.function_name}`);
  }
  return { ok: true };
};

export {};
"#,
        );
        let registry = Arc::new(
            CloudFunctionsRegistry::from_app_dir(app_dir.path()).expect("registry should load"),
        );
        let runtime_service_registry: Arc<dyn RuntimeServiceRegistry> = Arc::new(
            SandboxCatalogRuntimeServiceRegistry::new(Arc::new(EmptySandboxCatalog)),
        );
        let executor =
            CloudFunctionsTriggerExecutor::new(service, registry, runtime_service_registry);

        let outcome = executor.execute_invocation(
            &tenant_id,
            &sample_trigger_record("syncUser", &users, &user_id),
        );
        assert!(matches!(
            outcome,
            TriggerInvocationExecution::TerminalFailure { ref error }
                if error.contains("unknown handler exports.missing")
        ));
    }

    fn sample_trigger_record(
        registration_id: &str,
        table: &TableName,
        document_id: &neovex_core::DocumentId,
    ) -> TriggerInvocationRecord {
        let document_path = DocumentPath::from_segments([table.as_str(), document_id.as_str()])
            .expect("document path should parse");
        let after = Document::with_id(
            document_id.clone(),
            table.clone(),
            serde_json::Map::from_iter([("name".to_string(), json!("after"))]),
        );
        TriggerInvocationRecord::pending(
            TriggerInvocationKey::new(registration_id, "event-1")
                .expect("invocation key should parse"),
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
                    BTreeMap::from([("userId".to_string(), document_id.to_string())]),
                ),
                DocumentEventData::new(
                    Some(DocumentEventDocument::new(document_path, after)),
                    None,
                    None,
                ),
                TriggerCommitMetadata::new(SequenceNumber(1), Timestamp(1)),
                TriggerExecutionPrincipal::service(PrincipalContext::system()),
            ),
        )
    }

    fn write_cloud_functions_artifact(
        app_dir: &Path,
        targets: &[CloudFunctionsTargetDefinition],
        bundle: &str,
    ) {
        let artifact_dir = app_dir.join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR);
        fs::create_dir_all(&artifact_dir).expect("artifact dir should create");
        fs::write(
            artifact_dir.join(CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE),
            serde_json::to_vec_pretty(&CloudFunctionsArtifactManifest::v1())
                .expect("manifest should encode"),
        )
        .expect("manifest should write");
        fs::write(
            artifact_dir.join(CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE),
            serde_json::to_vec_pretty(
                &CloudFunctionsTargetsManifest::v1(targets.to_vec())
                    .expect("targets should validate"),
            )
            .expect("targets should encode"),
        )
        .expect("targets should write");

        let bundle_path = artifact_dir.join("bundle.mjs");
        fs::write(&bundle_path, bundle).expect("bundle should write");
        let bundle_sha256 = neovex_runtime::RuntimeBundle::compute_sha256_for_path(&bundle_path)
            .expect("bundle hash should load");
        fs::write(
            bundle_path.with_extension("sha256"),
            format!("{bundle_sha256}\n"),
        )
        .expect("bundle sha should write");
    }

    fn repo_root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repo root should exist")
            .to_path_buf()
    }

    fn run_cloud_functions_codegen(app_dir: &Path) -> std::process::Output {
        Command::new("node")
            .current_dir(repo_root())
            .arg("./packages/codegen/src/cli.mjs")
            .arg("--app")
            .arg(app_dir)
            .output()
            .expect("cloud functions codegen should run")
    }

    fn workspace_codegen_dependencies_available(repo_root: &Path) -> bool {
        repo_root.join("node_modules").join("esbuild").is_dir()
            && repo_root
                .join("packages")
                .join("codegen")
                .join("src")
                .join("cli.mjs")
                .is_file()
    }

    fn write_firebase_cloud_functions_fixture(app_dir: &Path) {
        let functions_dir = app_dir.join("functions");
        let source_dir = functions_dir.join("src");
        fs::create_dir_all(&source_dir).expect("firebase functions source dir should create");
        fs::write(
            app_dir.join("firebase.json"),
            r#"{
  "functions": { "source": "functions" }
}
"#,
        )
        .expect("firebase.json should write");
        fs::write(
            functions_dir.join("package.json"),
            r#"{
  "main": "lib/index.js"
}
"#,
        )
        .expect("functions package.json should write");
        fs::write(
            source_dir.join("index.ts"),
            r#"
import { setGlobalOptions } from "firebase-functions/v2";
import { onDocumentWritten } from "firebase-functions/v2/firestore";

setGlobalOptions({ retry: true });

export const syncUser = onDocumentWritten("users/{userId}", async (event) => {
  const ctx = globalThis.__neovexCreateContext({
    request: {
      function_name: "exports.syncUser",
      args: event,
    },
    sessionId: `trigger:${event.id}`,
  });

  await ctx.db.insert("audit", {
    userId: event.params.userId,
    retry: true,
    nextName: event.data.after.data().name,
  });
  return { ok: true };
});
"#,
        )
        .expect("firebase source fixture should write");
    }

    fn write_firebase_admin_cloud_functions_fixture(app_dir: &Path) {
        let functions_dir = app_dir.join("functions");
        let source_dir = functions_dir.join("src");
        fs::create_dir_all(&source_dir).expect("firebase functions source dir should create");
        fs::write(
            app_dir.join("firebase.json"),
            r#"{
  "functions": { "source": "functions" }
}
"#,
        )
        .expect("firebase.json should write");
        fs::write(
            functions_dir.join("package.json"),
            r#"{
  "main": "lib/index.js"
}
"#,
        )
        .expect("functions package.json should write");
        fs::write(
            source_dir.join("index.ts"),
            r#"
import { onDocumentWritten } from "firebase-functions/v2/firestore";
import { Timestamp, getFirestore } from "firebase-admin/firestore";

export const syncUser = onDocumentWritten("users/{userId}", async (event) => {
  const firestore = getFirestore();
  const userRef = firestore.collection("users").doc(event.params.userId);
  const snapshot = await userRef.get();
  await firestore.doc(`audit/${event.params.userId}`).set({
    beforeName: snapshot.get("profile.name"),
    recordedAt: Timestamp.fromMillis(42),
  });
  await userRef.update({
    processed: true,
  });
  await firestore.doc(`trash/${event.params.userId}`).delete();
  return { ok: true };
});
"#,
        )
        .expect("firebase admin source fixture should write");
    }

    fn write_firebase_admin_trigger_lifecycle_fixture(app_dir: &Path) {
        let functions_dir = app_dir.join("functions");
        let source_dir = functions_dir.join("src");
        fs::create_dir_all(&source_dir).expect("firebase functions source dir should create");
        fs::write(
            app_dir.join("firebase.json"),
            r#"{
  "functions": { "source": "functions" }
}
"#,
        )
        .expect("firebase.json should write");
        fs::write(
            functions_dir.join("package.json"),
            r#"{
  "main": "lib/index.js"
}
"#,
        )
        .expect("functions package.json should write");
        fs::write(
            source_dir.join("index.ts"),
            r#"
import { setGlobalOptions } from "firebase-functions/v2";
import { onDocumentWritten } from "firebase-functions/v2/firestore";
import { getFirestore } from "firebase-admin/firestore";

setGlobalOptions({ retry: true });

export const syncUser = onDocumentWritten("users/{userId}", async (event) => {
  const firestore = getFirestore();
  const snapshot = await firestore.doc(`users/${event.params.userId}`).get();
  await firestore.doc(`audit/${event.params.userId}`).set({
    retry: true,
    type: event.type,
    name: snapshot.get("profile.name"),
  });
  return { ok: true };
});
"#,
        )
        .expect("firebase lifecycle source fixture should write");
    }

    fn write_firebase_updated_noop_lifecycle_fixture(app_dir: &Path) {
        let functions_dir = app_dir.join("functions");
        let source_dir = functions_dir.join("src");
        fs::create_dir_all(&source_dir).expect("firebase functions source dir should create");
        fs::write(
            app_dir.join("firebase.json"),
            r#"{
  "functions": { "source": "functions" }
}
"#,
        )
        .expect("firebase.json should write");
        fs::write(
            functions_dir.join("package.json"),
            r#"{
  "main": "lib/index.js"
}
"#,
        )
        .expect("functions package.json should write");
        fs::write(
            source_dir.join("index.ts"),
            r#"
import { onDocumentUpdated } from "firebase-functions/v2/firestore";

export const syncUser = onDocumentUpdated("users/{userId}", async (event) => {
  const ctx = globalThis.__neovexCreateContext({
    request: {
      function_name: "exports.syncUser",
      args: event,
    },
    sessionId: `trigger:${event.id}`,
  });

  await ctx.db.insert("audit", {
    userId: event.params.userId,
    name: event.data.after.data().profile.name,
  });
  return { ok: true };
});
"#,
        )
        .expect("firebase noop-update source fixture should write");
    }

    fn write_firebase_concurrent_trigger_lifecycle_fixture(app_dir: &Path) {
        let functions_dir = app_dir.join("functions");
        let source_dir = functions_dir.join("src");
        fs::create_dir_all(&source_dir).expect("firebase functions source dir should create");
        fs::write(
            app_dir.join("firebase.json"),
            r#"{
  "functions": { "source": "functions" }
}
"#,
        )
        .expect("firebase.json should write");
        fs::write(
            functions_dir.join("package.json"),
            r#"{
  "main": "lib/index.js"
}
"#,
        )
        .expect("functions package.json should write");
        fs::write(
            source_dir.join("index.ts"),
            r#"
import { onDocumentWritten } from "firebase-functions/v2/firestore";

export const syncUser = onDocumentWritten("users/{userId}", async (event) => {
  const ctx = globalThis.__neovexCreateContext({
    request: {
      function_name: "exports.syncUser",
      args: event,
    },
    sessionId: `trigger:${event.id}`,
  });

  await ctx.db.insert("audit", {
    userId: event.params.userId,
  });
  return { ok: true };
});
"#,
        )
        .expect("firebase concurrent-trigger source fixture should write");
    }

    fn write_firebase_chain_depth_lifecycle_fixture(app_dir: &Path) {
        let functions_dir = app_dir.join("functions");
        let source_dir = functions_dir.join("src");
        fs::create_dir_all(&source_dir).expect("firebase functions source dir should create");
        fs::write(
            app_dir.join("firebase.json"),
            r#"{
  "functions": { "source": "functions" }
}
"#,
        )
        .expect("firebase.json should write");
        fs::write(
            functions_dir.join("package.json"),
            r#"{
  "main": "lib/index.js"
}
"#,
        )
        .expect("functions package.json should write");
        fs::write(
            source_dir.join("index.ts"),
            r#"
import { onDocumentWritten } from "firebase-functions/v2/firestore";
import { getFirestore } from "firebase-admin/firestore";

export const cascade = onDocumentWritten("chain/{docId}", async (event) => {
  const ctx = globalThis.__neovexCreateContext({
    request: {
      function_name: "exports.cascade",
      args: event,
    },
    sessionId: `trigger:${event.id}`,
  });
  const firestore = getFirestore();
  const step = event.data.after.data().step;

  await ctx.db.insert("audit", {
    docId: event.params.docId,
    step,
  });

  if (step < 12) {
    await firestore.doc(`chain/step${step + 1}`).set({
      step: step + 1,
    });
  }
  return { ok: true };
});
"#,
        )
        .expect("firebase chain-depth source fixture should write");
    }

    fn write_framework_admin_trigger_lifecycle_fixture(app_dir: &Path) {
        let source_dir = app_dir.join("src");
        fs::create_dir_all(&source_dir).expect("framework source dir should create");
        fs::create_dir_all(app_dir.join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR))
            .expect("framework artifact dir should create");
        fs::write(
            app_dir.join("package.json"),
            r#"{
  "main": "dist/index.js",
  "dependencies": {
    "@google-cloud/functions-framework": "^3.4.5"
  }
}
"#,
        )
        .expect("framework package.json should write");
        fs::write(
            source_dir.join("index.ts"),
            r#"
import functions from "@google-cloud/functions-framework";
import { getFirestore } from "firebase-admin/firestore";

functions.cloudEvent("syncUser", async (event) => {
  const firestore = getFirestore();
  const documentPath = event.subject.replace(/^documents\//, "");
  const snapshot = await firestore.doc(documentPath).get();
  await firestore.doc(`audit/${snapshot.id}`).set({
    type: event.type,
    name: snapshot.get("profile.name"),
    subject: event.subject,
  });
  return { ok: true };
});
"#,
        )
        .expect("framework lifecycle source fixture should write");
        fs::write(
            app_dir
                .join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR)
                .join(CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE),
            serde_json::to_vec_pretty(
                &CloudFunctionsTargetsManifest::v1(vec![CloudFunctionsTargetDefinition {
                    name: "syncUser".to_string(),
                    entrypoint: "registry.syncUser".to_string(),
                    authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
                    signature_type: CloudFunctionsSignatureType::CloudEvent,
                    binding: CloudFunctionsTargetBinding::FirestoreDocument {
                        event_type: FirestoreCloudEventType::Written,
                        database: "(default)".to_string(),
                        document: "users/{userId}".to_string(),
                        namespace: None,
                        execution: CloudFunctionsExecutionBinding::Service,
                    },
                }])
                .expect("framework targets should validate"),
            )
            .expect("framework targets should encode"),
        )
        .expect("framework targets should write");
    }

    fn seed_firebase_document(
        service: &Arc<Service>,
        tenant_id: &TenantId,
        document_path: &str,
        fields: serde_json::Map<String, serde_json::Value>,
    ) {
        let document_path = DocumentPath::from_segments(document_path.split('/'))
            .expect("document path should parse");
        let locator =
            locator_for_document_path(&document_path).expect("firebase locator should resolve");
        let execution_unit = service
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("execution unit should start");
        execution_unit
            .execute_atomic_write_batch(
                AtomicWriteBatch::new(vec![AtomicWrite::Set {
                    key: WriteKey::from(ResourcePathBinding::new(locator, document_path)),
                    document: fields,
                    mode: WriteSetMode::Overwrite,
                    precondition: WritePrecondition::default(),
                    transforms: Vec::new(),
                }])
                .expect("batch should build"),
            )
            .expect("batch should execute");
    }

    fn firebase_document(
        service: &Arc<Service>,
        tenant_id: &TenantId,
        document_path: &str,
    ) -> neovex_core::Result<Option<Document>> {
        let document_path = DocumentPath::from_segments(document_path.split('/'))
            .expect("document path should parse");
        let locator = locator_for_document_path(&document_path)?;
        match service.get_document(tenant_id, &locator.table, locator.id) {
            Ok(document) => Ok(Some(document)),
            Err(Error::DocumentNotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn table_documents(
        service: &Arc<Service>,
        tenant_id: &TenantId,
        table: &str,
    ) -> neovex_core::Result<Vec<Document>> {
        service.query_documents(
            tenant_id,
            &neovex_core::Query {
                table: TableName::new(table).expect("table name should parse"),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
        )
    }
}
