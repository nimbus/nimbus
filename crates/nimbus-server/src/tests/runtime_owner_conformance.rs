use std::fs;
use std::path::Path;

use futures::future::BoxFuture;
use nimbus_engine::Engine;
use nimbus_testing::{EngineFixture, HttpApiFixture, ServerFixture};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tempfile::tempdir;

use super::*;
use crate::adapters::cloud_functions::{
    CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE, CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR,
    CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE, CloudFunctionsArtifactManifest,
    CloudFunctionsAuthoringSurface, CloudFunctionsExecutionPrincipal, CloudFunctionsHttpExposure,
    CloudFunctionsRegistry, CloudFunctionsSignatureType, CloudFunctionsTargetBinding,
    CloudFunctionsTargetDefinition, CloudFunctionsTargetsManifest,
};

trait RuntimeOwnerConformanceDriver {
    fn name(&self) -> &'static str;
    fn invoke<'a>(&'a self, sentinel: Option<&'a str>) -> BoxFuture<'a, Value>;
}

struct ConvexDriver<'a> {
    server: &'a ServerFixture,
}

impl RuntimeOwnerConformanceDriver for ConvexDriver<'_> {
    fn name(&self) -> &'static str {
        "convex"
    }

    fn invoke<'a>(&'a self, sentinel: Option<&'a str>) -> BoxFuture<'a, Value> {
        Box::pin(async move {
            let response = self
                .server
                .client()
                .post(self.server.http_url("/convex/demo/query"))
                .header(reqwest::header::AUTHORIZATION, convex_team_bearer())
                .json(&json!({
                    "name": "owner:sentinel",
                    "args": { "sentinel": sentinel },
                }))
                .send()
                .await
                .expect("Convex conformance invocation should send");
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "Convex conformance invocation should succeed"
            );
            response
                .json()
                .await
                .expect("Convex conformance response should decode")
        })
    }
}

struct CloudFunctionsDriver<'a> {
    server: &'a ServerFixture,
}

impl RuntimeOwnerConformanceDriver for CloudFunctionsDriver<'_> {
    fn name(&self) -> &'static str {
        "cloud_functions"
    }

    fn invoke<'a>(&'a self, sentinel: Option<&'a str>) -> BoxFuture<'a, Value> {
        Box::pin(async move {
            let response = self
                .server
                .client()
                .post(self.server.http_url("/owner-sentinel"))
                .json(&json!({ "sentinel": sentinel }))
                .send()
                .await
                .expect("Cloud Functions conformance invocation should send");
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "Cloud Functions conformance invocation should succeed"
            );
            response
                .json()
                .await
                .expect("Cloud Functions conformance response should decode")
        })
    }
}

async fn assert_runtime_owner_lifecycle_conformance(
    server: &ServerFixture,
    driver: &impl RuntimeOwnerConformanceDriver,
) {
    let api = HttpApiFixture::new(server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let first = driver.invoke(Some("old-owner-secret")).await;
    assert_eq!(
        first["observedBeforeWrite"],
        Value::Null,
        "{} first invocation must start without guest state",
        driver.name()
    );
    let same_owner = driver.invoke(None).await;
    assert_eq!(
        same_owner["observedBeforeWrite"],
        json!("old-owner-secret"),
        "{} must preserve intended warm state inside one live owner",
        driver.name()
    );

    assert_eq!(
        api.delete_tenant("demo").await.status(),
        StatusCode::NO_CONTENT,
        "{} tenant deletion must acknowledge runtime retirement",
        driver.name()
    );
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let recreated = driver.invoke(None).await;
    assert_eq!(
        recreated["observedBeforeWrite"],
        Value::Null,
        "{} recreated tenant incarnation must not observe retired guest state",
        driver.name()
    );
}

#[tokio::test]
async fn convex_passes_runtime_owner_lifecycle_conformance() {
    run_conformance_subprocess(
        "tests::runtime_owner_conformance::convex_passes_runtime_owner_lifecycle_conformance_subprocess",
    )
    .await;
}

#[tokio::test]
async fn cloud_functions_passes_runtime_owner_lifecycle_conformance() {
    run_conformance_subprocess(
        "tests::runtime_owner_conformance::cloud_functions_passes_runtime_owner_lifecycle_conformance_subprocess",
    )
    .await;
}

async fn run_conformance_subprocess(test_name: &str) {
    let executable = std::env::current_exe().expect("server test binary should have a path");
    let mut command = tokio::process::Command::new(executable);
    command
        .arg(test_name)
        .arg("--ignored")
        .arg("--exact")
        .arg("--nocapture")
        .arg("--test-threads=1")
        .kill_on_drop(true);
    let output = tokio::time::timeout(std::time::Duration::from_secs(45), command.output())
        .await
        .unwrap_or_else(|_| panic!("isolated conformance test {test_name} timed out"))
        .expect("isolated conformance test should start");
    assert!(
        output.status.success(),
        "isolated conformance test {test_name} should pass ({status})\nstdout:\n{stdout}\nstderr:\n{stderr}",
        status = output.status,
        stdout = String::from_utf8_lossy(&output.stdout),
        stderr = String::from_utf8_lossy(&output.stderr),
    );
}

#[tokio::test]
#[ignore = "runs in a subprocess to isolate V8 anchor state"]
async fn convex_passes_runtime_owner_lifecycle_conformance_subprocess() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([{
            "name": "owner:sentinel",
            "kind": "query",
            "visibility": "public",
            "plan": null,
            "runtime_handler": "async () => null"
        }]),
        json!([]),
        Some(CONVEX_SENTINEL_BUNDLE),
    )
    .with_runtime_limits(nimbus_testing::cooperative_warm_pool_runtime_test_limits());
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex_team(fixture.engine(), registry)).await;

    assert_runtime_owner_lifecycle_conformance(&server, &ConvexDriver { server: &server }).await;
}

#[tokio::test]
#[ignore = "runs in a subprocess to isolate V8 anchor state"]
async fn cloud_functions_passes_runtime_owner_lifecycle_conformance_subprocess() {
    let app_dir = tempdir().expect("Cloud Functions conformance app should build");
    write_cloud_functions_sentinel_artifact(app_dir.path());
    let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
        .expect("Cloud Functions conformance registry should load")
        .with_runtime_limits(nimbus_testing::cooperative_warm_pool_runtime_test_limits());
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_cloud_functions(registry)
            .build(),
    )
    .await;

    assert_runtime_owner_lifecycle_conformance(&server, &CloudFunctionsDriver { server: &server })
        .await;
}

const CONVEX_SENTINEL_BUNDLE: &str = r#"
globalThis.__nimbusInvoke = async function (request) {
  const observedBeforeWrite = globalThis.__runtimeOwnerSentinel ?? null;
  if (request.args?.sentinel != null) {
    globalThis.__runtimeOwnerSentinel = request.args.sentinel;
  }
  return { status: "ok", value: { observedBeforeWrite } };
};

export {};
"#;

const CLOUD_FUNCTIONS_SENTINEL_BUNDLE: &str = r#"
globalThis.__nimbusInvoke = async function (request) {
  const observedBeforeWrite = globalThis.__runtimeOwnerSentinel ?? null;
  if (request.args?.body?.sentinel != null) {
    globalThis.__runtimeOwnerSentinel = request.args.body.sentinel;
  }
  return {
    status: 200,
    headers: {},
    body_kind: "json",
    body: { observedBeforeWrite },
  };
};

export {};
"#;

fn write_cloud_functions_sentinel_artifact(app_dir: &Path) {
    let artifact_dir = app_dir.join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR);
    fs::create_dir_all(&artifact_dir).expect("Cloud Functions artifact dir should create");
    fs::write(
        artifact_dir.join(CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE),
        serde_json::to_vec_pretty(&CloudFunctionsArtifactManifest::v1())
            .expect("Cloud Functions manifest should encode"),
    )
    .expect("Cloud Functions manifest should write");
    let target = CloudFunctionsTargetDefinition {
        name: "ownerSentinel".to_string(),
        entrypoint: "registry.ownerSentinel".to_string(),
        authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
        signature_type: CloudFunctionsSignatureType::Http,
        binding: CloudFunctionsTargetBinding::Https {
            exposure: CloudFunctionsHttpExposure::Http,
            path: "/owner-sentinel".to_string(),
            execution: CloudFunctionsExecutionPrincipal::RequestPrincipal,
        },
    };
    fs::write(
        artifact_dir.join(CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE),
        serde_json::to_vec_pretty(
            &CloudFunctionsTargetsManifest::v1(vec![target])
                .expect("Cloud Functions conformance target should validate"),
        )
        .expect("Cloud Functions targets should encode"),
    )
    .expect("Cloud Functions targets should write");
    let bundle_path = artifact_dir.join("bundle.mjs");
    fs::write(&bundle_path, CLOUD_FUNCTIONS_SENTINEL_BUNDLE)
        .expect("Cloud Functions bundle should write");
    let sha = nimbus_runtime::RuntimeBundle::compute_sha256_for_path(&bundle_path)
        .expect("Cloud Functions bundle hash should compute");
    fs::write(bundle_path.with_extension("sha256"), format!("{sha}\n"))
        .expect("Cloud Functions bundle hash should write");
}
