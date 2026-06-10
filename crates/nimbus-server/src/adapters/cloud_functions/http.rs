use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{OriginalUri, Query as AxumQuery, State};
use axum::http::{HeaderMap, Method};
use axum::response::Response;

mod callable;
mod invocation;
mod response;
mod tenant;

use callable::{CallableHttpRequest, handle_callable_target};
use invocation::{ServerCloudFunctionsHttpInvocation, execute_http_target};
use nimbus_cloud_functions::build_http_request_args;
use tenant::resolve_cloud_functions_http_tenant;

use super::{CloudFunctionsHttpExposure, CloudFunctionsRegistry, CloudFunctionsTargetBinding};
use crate::state::{AppError, AppState};

pub(crate) async fn http_handler(
    State(state): State<Arc<AppState>>,
    method: Method,
    headers: HeaderMap,
    original_uri: OriginalUri,
    query: AxumQuery<HashMap<String, String>>,
    body: Bytes,
) -> std::result::Result<Response, AppError> {
    let deployment = state.current_deployment();
    let Some(registry) = deployment.cloud_functions_registry() else {
        return Err(AppError::not_found(
            "cloud functions http handler requires an active Cloud Functions registry",
        ));
    };
    let request_path = original_uri.0.path().to_string();
    let Some(target) = registry.resolve_https_target(&request_path) else {
        return Err(AppError::not_found(
            "cloud functions http handler not found",
        ));
    };
    let entrypoint = target.entrypoint.clone();
    let tenant_id = resolve_cloud_functions_http_tenant(&state)?;
    let exposure = match &target.binding {
        CloudFunctionsTargetBinding::Https { exposure, .. } => *exposure,
        _ => unreachable!("resolve_https_target only returns https bindings"),
    };
    match exposure {
        CloudFunctionsHttpExposure::Http => {
            let args = build_http_request_args(
                &method,
                &headers,
                original_uri.0.query(),
                &request_path,
                query.0,
                &body,
            )?;
            execute_http_target(ServerCloudFunctionsHttpInvocation {
                engine: state.engine.clone(),
                runtime_service_registry: state.runtime_service_registry(),
                tenant_isolation_mode: state.tenant_isolation_mode,
                registry,
                deployment_generation: deployment.generation,
                tenant_id,
                function_name: entrypoint,
                args,
                auth: None,
            })
        }
        CloudFunctionsHttpExposure::Callable => {
            handle_callable_target(
                state,
                deployment,
                registry,
                tenant_id,
                entrypoint,
                CallableHttpRequest {
                    method: &method,
                    headers: &headers,
                    original_uri: &original_uri,
                    request_path: &request_path,
                    query: query.0,
                    body,
                },
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::sync::Arc;

    use axum::http::header;
    use futures::future::BoxFuture;
    use nimbus_core::{Query, TableName, TenantId};
    use nimbus_engine::Engine;
    use nimbus_runtime::{
        InvocationAuth, RuntimeUserIdentity, VerifiedUserIdentity, VerifiedUserIdentityKind,
    };
    use nimbus_testing::{EngineFixture, ServerFixture};
    use reqwest::StatusCode;
    use serde_json::Value;
    use tempfile::tempdir;

    use super::*;
    use crate::adapters::cloud_functions::{
        CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE, CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR,
        CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE, CloudFunctionsArtifactManifest,
        CloudFunctionsAuthoringSurface, CloudFunctionsExecutionPrincipal,
        CloudFunctionsHttpExposure, CloudFunctionsSignatureType, CloudFunctionsTargetBinding,
        CloudFunctionsTargetDefinition, CloudFunctionsTargetsManifest,
    };

    struct TenantClaimApplicationAuthVerifier;

    impl nimbus_auth::ApplicationAuthVerifier for TenantClaimApplicationAuthVerifier {
        fn verify_bearer_token<'a>(
            &'a self,
            token: &'a str,
        ) -> BoxFuture<'a, std::result::Result<InvocationAuth, nimbus_auth::ApplicationAuthError>>
        {
            let token = token.to_string();
            Box::pin(async move {
                let Some(tenant_id) = token.strip_prefix("tenant:") else {
                    return Err(nimbus_auth::ApplicationAuthError::unauthorized(
                        "test bearer token must use tenant:<tenant_id>",
                    ));
                };
                Ok(invocation_auth_with_tenant_claim(tenant_id))
            })
        }
    }

    fn invocation_auth_with_tenant_claim(tenant_id: &str) -> InvocationAuth {
        InvocationAuth::with_identities(
            runtime_identity_with_tenant_claim(tenant_id),
            verified_identity_with_tenant_claim(tenant_id),
            false,
        )
    }

    fn runtime_identity_with_tenant_claim(tenant_id: &str) -> RuntimeUserIdentity {
        RuntimeUserIdentity {
            token_identifier: format!("test|user-123|{tenant_id}"),
            subject: "user-123".to_string(),
            issuer: "https://cloud-functions-auth.example.com".to_string(),
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
            custom_claims: tenant_claims(tenant_id),
        }
    }

    fn verified_identity_with_tenant_claim(tenant_id: &str) -> VerifiedUserIdentity {
        VerifiedUserIdentity {
            kind: VerifiedUserIdentityKind::CustomJwt,
            token_identifier: format!("test|user-123|{tenant_id}"),
            subject: "user-123".to_string(),
            issuer: "https://cloud-functions-auth.example.com".to_string(),
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
            custom_claims: tenant_claims(tenant_id),
        }
    }

    fn tenant_claims(tenant_id: &str) -> serde_json::Map<String, Value> {
        serde_json::Map::from_iter([(
            "tenant_id".to_string(),
            Value::String(tenant_id.to_string()),
        )])
    }

    #[tokio::test]
    async fn cloud_functions_http_handler_dispatches_exact_path_and_commits_writes() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let service = fixture.engine();
        service
            .create_tenant(TenantId::new("demo").expect("tenant id should parse"))
            .expect("tenant should create");
        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(
            app_dir.path(),
            &[CloudFunctionsTargetDefinition {
                name: "helloWorld".to_string(),
                entrypoint: "registry.helloWorld".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Http,
                    path: "/hello".to_string(),
                    execution: CloudFunctionsExecutionPrincipal::RequestPrincipal,
                },
            }],
            r#"
globalThis.__nimbusInvoke = async function (request) {
  if (request.function_name !== "registry.helloWorld") {
    throw new Error(`unknown handler ${request.function_name}`);
  }
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
  });
  await ctx.db.insert("audit", {
    path: request.args.path,
    method: request.args.method,
    name: request.args.query.name ?? null,
  });
  return {
    status: 201,
    headers: {
      "x-cloud-functions-target": "helloWorld",
    },
    body_kind: "json",
    body: {
      method: request.args.method,
      path: request.args.path,
      originalUrl: request.args.original_url,
      query: request.args.query,
      body: request.args.body,
      rawBody: request.args.raw_body,
      header: request.args.headers["x-test"] ?? null,
    },
  };
};

export {};
"#,
        );
        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(service.clone())
                .with_cloud_functions(registry)
                .build(),
        )
        .await;

        let response = server
            .client()
            .post(server.http_url("/hello?name=jack"))
            .header("x-test", "present")
            .json(&serde_json::json!({ "hello": "world" }))
            .send()
            .await
            .expect("request should send");

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response
                .headers()
                .get("x-cloud-functions-target")
                .and_then(|value| value.to_str().ok()),
            Some("helloWorld")
        );
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            response
                .json::<Value>()
                .await
                .expect("response body should decode"),
            serde_json::json!({
                "method": "POST",
                "path": "/hello",
                "originalUrl": server.http_url("/hello?name=jack"),
                "query": {
                    "name": "jack",
                },
                "body": {
                    "hello": "world",
                },
                "rawBody": "{\"hello\":\"world\"}",
                "header": "present",
            })
        );

        let audit_documents = service
            .query_documents(
                &TenantId::new("demo").expect("tenant id should parse"),
                &Query {
                    table: TableName::new("audit").expect("table should parse"),
                    filters: Vec::new(),
                    order: None,
                    limit: None,
                },
            )
            .expect("audit query should succeed");
        assert_eq!(audit_documents.len(), 1);
        assert_eq!(
            audit_documents[0].get_field("path"),
            Some(&Value::String("/hello".into()))
        );
        assert_eq!(
            audit_documents[0].get_field("name"),
            Some(&Value::String("jack".into()))
        );
    }

    #[tokio::test]
    async fn cloud_functions_http_handler_rejects_ambiguous_multi_tenant_binding() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let service = fixture.engine();
        service
            .create_tenant(TenantId::new("alpha").expect("tenant id should parse"))
            .expect("first tenant should create");
        service
            .create_tenant(TenantId::new("beta").expect("tenant id should parse"))
            .expect("second tenant should create");
        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(
            app_dir.path(),
            &[CloudFunctionsTargetDefinition {
                name: "helloWorld".to_string(),
                entrypoint: "registry.helloWorld".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Http,
                    path: "/hello".to_string(),
                    execution: CloudFunctionsExecutionPrincipal::RequestPrincipal,
                },
            }],
            r#"
globalThis.__nimbusInvoke = async function () {
  return { status: 200, body_kind: "text", body: "ok" };
};

export {};
"#,
        );
        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(service)
                .with_cloud_functions(registry)
                .build(),
        )
        .await;

        let response = server
            .client()
            .get(server.http_url("/hello"))
            .send()
            .await
            .expect("request should send");

        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert!(
            response
                .text()
                .await
                .expect("error body should decode")
                .contains("exactly one tenant")
        );
    }

    #[tokio::test]
    async fn cloud_functions_callable_handler_supports_preflight_and_json_envelope() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let service = fixture.engine();
        service
            .create_tenant(TenantId::new("demo").expect("tenant id should parse"))
            .expect("tenant should create");
        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(
            app_dir.path(),
            &[CloudFunctionsTargetDefinition {
                name: "hello".to_string(),
                entrypoint: "exports.hello".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Callable,
                    path: "/hello".to_string(),
                    execution: CloudFunctionsExecutionPrincipal::RequestPrincipal,
                },
            }],
            r#"
globalThis.__nimbusInvoke = async function (request) {
  if (request.function_name !== "exports.hello") {
    throw new Error(`unknown handler ${request.function_name}`);
  }
  return {
    status: 200,
    headers: {
      "x-cloud-functions-target": "hello",
    },
    body_kind: "json",
    body: {
      data: {
        method: request.args.method,
        body: request.args.body,
        data: request.args.callable.data,
        auth: request.args.callable.auth ?? null,
        instanceIdToken: request.args.callable.instance_id_token ?? null,
        rawBody: request.args.raw_body,
      },
    },
  };
};

export {};
"#,
        );
        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(service)
                .with_cloud_functions(registry)
                .build(),
        )
        .await;
        let allowed_origin = server.http_url("").trim_end_matches('/').to_string();

        let preflight = server
            .client()
            .request(reqwest::Method::OPTIONS, server.http_url("/hello"))
            .header("origin", &allowed_origin)
            .header("access-control-request-method", "POST")
            .header(
                "access-control-request-headers",
                "authorization, content-type, firebase-instance-id-token, x-firebase-appcheck",
            )
            .send()
            .await
            .expect("preflight should send");
        assert_eq!(preflight.status(), StatusCode::OK);
        assert_eq!(
            preflight
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some(allowed_origin.as_str())
        );
        assert_eq!(
            preflight
                .headers()
                .get("access-control-allow-methods")
                .and_then(|value| value.to_str().ok()),
            Some("GET,POST,PUT,PATCH,DELETE,OPTIONS")
        );
        let allow_headers = preflight
            .headers()
            .get("access-control-allow-headers")
            .and_then(|value| value.to_str().ok())
            .expect("preflight should expose allow headers");
        assert!(allow_headers.contains("authorization"));
        assert!(allow_headers.contains("content-type"));
        assert!(allow_headers.contains("firebase-instance-id-token"));
        assert!(allow_headers.contains("x-firebase-appcheck"));

        let response = server
            .client()
            .post(server.http_url("/hello"))
            .header("origin", &allowed_origin)
            .header("firebase-instance-id-token", "iid-123")
            .json(&serde_json::json!({ "data": { "hello": "world" } }))
            .send()
            .await
            .expect("callable request should send");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some(allowed_origin.as_str())
        );
        assert_eq!(
            response
                .headers()
                .get("x-cloud-functions-target")
                .and_then(|value| value.to_str().ok()),
            Some("hello")
        );
        assert_eq!(
            response
                .json::<Value>()
                .await
                .expect("callable body should decode"),
            serde_json::json!({
                "data": {
                    "method": "POST",
                    "body": {
                        "data": {
                            "hello": "world",
                        },
                    },
                    "data": {
                        "hello": "world",
                    },
                    "auth": null,
                    "instanceIdToken": "iid-123",
                    "rawBody": "{\"data\":{\"hello\":\"world\"}}",
                },
            })
        );
    }

    #[tokio::test]
    async fn cloud_functions_callable_handler_rejects_invalid_input_and_app_check_headers() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let service = fixture.engine();
        service
            .create_tenant(TenantId::new("demo").expect("tenant id should parse"))
            .expect("tenant should create");
        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(
            app_dir.path(),
            &[CloudFunctionsTargetDefinition {
                name: "hello".to_string(),
                entrypoint: "exports.hello".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Callable,
                    path: "/hello".to_string(),
                    execution: CloudFunctionsExecutionPrincipal::RequestPrincipal,
                },
            }],
            r#"
globalThis.__nimbusInvoke = async function () {
  return {
    status: 200,
    headers: {
      "content-type": "application/json",
    },
    body_kind: "json",
    body: {
      data: {
        ok: true,
      },
    },
  };
};

export {};
"#,
        );
        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(service)
                .with_cloud_functions(registry)
                .build(),
        )
        .await;
        let allowed_origin = server.http_url("").trim_end_matches('/').to_string();

        let invalid_body = server
            .client()
            .post(server.http_url("/hello"))
            .header("origin", &allowed_origin)
            .json(&serde_json::json!({}))
            .send()
            .await
            .expect("invalid callable request should send");
        assert_eq!(invalid_body.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            invalid_body
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some(allowed_origin.as_str())
        );
        assert_eq!(
            invalid_body
                .json::<Value>()
                .await
                .expect("error body should decode"),
            serde_json::json!({
                "error": {
                    "status": "INVALID_ARGUMENT",
                    "message": "invalid input: cloud functions callable handlers require a top-level JSON `data` field",
                },
            })
        );

        let app_check = server
            .client()
            .post(server.http_url("/hello"))
            .header("origin", &allowed_origin)
            .header("x-firebase-appcheck", "token")
            .json(&serde_json::json!({ "data": null }))
            .send()
            .await
            .expect("app check request should send");
        assert_eq!(app_check.status(), StatusCode::NOT_IMPLEMENTED);
        assert_eq!(
            app_check
                .json::<Value>()
                .await
                .expect("app check error body should decode"),
            serde_json::json!({
                "error": {
                    "status": "UNIMPLEMENTED",
                    "message": "cloud functions callable App Check verification is not covered in the first callable slice",
                },
            })
        );
    }

    #[tokio::test]
    async fn cloud_functions_callable_handler_fails_closed_when_bearer_auth_cannot_be_verified() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let service = fixture.engine();
        service
            .create_tenant(TenantId::new("demo").expect("tenant id should parse"))
            .expect("tenant should create");
        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(
            app_dir.path(),
            &[CloudFunctionsTargetDefinition {
                name: "hello".to_string(),
                entrypoint: "exports.hello".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Callable,
                    path: "/hello".to_string(),
                    execution: CloudFunctionsExecutionPrincipal::RequestPrincipal,
                },
            }],
            r#"
globalThis.__nimbusInvoke = async function () {
  return {
    status: 200,
    body_kind: "json",
    body: {
      data: {
        ok: true,
      },
    },
  };
};

export {};
"#,
        );
        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(service)
                .with_cloud_functions(registry)
                .build(),
        )
        .await;
        let allowed_origin = server.http_url("").trim_end_matches('/').to_string();

        let response = server
            .client()
            .post(server.http_url("/hello"))
            .header("origin", &allowed_origin)
            .header("authorization", "Bearer not-a-real-token")
            .json(&serde_json::json!({ "data": { "hello": "world" } }))
            .send()
            .await
            .expect("callable request should send");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some(allowed_origin.as_str())
        );
        assert_eq!(
            response
                .json::<Value>()
                .await
                .expect("callable auth error body should decode"),
            serde_json::json!({
                "error": {
                    "status": "UNAUTHENTICATED",
                    "message": "no application auth providers are configured for the active deployment",
                },
            })
        );
    }

    #[tokio::test]
    async fn cloud_functions_callable_rejects_application_bearer_for_different_tenant() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let service = fixture.engine();
        service
            .create_tenant(TenantId::new("tenant-a").expect("tenant id should parse"))
            .expect("tenant should create");
        let app_dir = tempdir().expect("app tempdir should build");
        write_cloud_functions_artifact(
            app_dir.path(),
            &[CloudFunctionsTargetDefinition {
                name: "hello".to_string(),
                entrypoint: "exports.hello".to_string(),
                authoring_surface: CloudFunctionsAuthoringSurface::FirebaseV2,
                signature_type: CloudFunctionsSignatureType::Http,
                binding: CloudFunctionsTargetBinding::Https {
                    exposure: CloudFunctionsHttpExposure::Callable,
                    path: "/hello".to_string(),
                    execution: CloudFunctionsExecutionPrincipal::RequestPrincipal,
                },
            }],
            r#"
globalThis.__nimbusInvoke = async function () {
  return {
    status: 200,
    body_kind: "json",
    body: {
      data: {
        ok: true,
      },
    },
  };
};

export {};
"#,
        );
        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(service)
                .with_application_auth_verifier(Arc::new(TenantClaimApplicationAuthVerifier))
                .with_cloud_functions(registry)
                .build(),
        )
        .await;
        let allowed_origin = server.http_url("").trim_end_matches('/').to_string();

        let authorized = server
            .client()
            .post(server.http_url("/hello"))
            .header("origin", &allowed_origin)
            .header("authorization", "Bearer tenant:tenant-a")
            .json(&serde_json::json!({ "data": { "hello": "world" } }))
            .send()
            .await
            .expect("same-tenant callable request should send");
        let authorized_status = authorized.status();
        let authorized_body = authorized
            .text()
            .await
            .expect("same-tenant callable body should read");
        assert_eq!(
            authorized_status,
            StatusCode::OK,
            "same-tenant callable body: {authorized_body}"
        );

        let rejected = server
            .client()
            .post(server.http_url("/hello"))
            .header("origin", &allowed_origin)
            .header("authorization", "Bearer tenant:tenant-b")
            .json(&serde_json::json!({ "data": { "hello": "world" } }))
            .send()
            .await
            .expect("swapped-tenant callable request should send");
        let rejected_status = rejected.status();
        let rejected_body = rejected
            .text()
            .await
            .expect("swapped-tenant callable body should read");
        assert_eq!(
            rejected_status,
            StatusCode::FORBIDDEN,
            "swapped-tenant callable body: {rejected_body}"
        );
        assert!(
            rejected_body.contains("PERMISSION_DENIED"),
            "callable response should use Firebase permission denial status: {rejected_body}"
        );
        assert!(
            rejected_body.contains("authorizes tenant `tenant-b`"),
            "swapped-tenant callable error should name the verified tenant claim: {rejected_body}"
        );
        assert!(
            rejected_body.contains("targeted tenant `tenant-a`"),
            "swapped-tenant callable error should name the implicit target tenant: {rejected_body}"
        );
    }

    #[tokio::test]
    async fn cloud_functions_http_handler_runs_generated_framework_bundle_end_to_end() {
        let repo_root = repo_root();
        if !workspace_codegen_dependencies_available(&repo_root) {
            eprintln!(
                "skipping generated functions.http() end-to-end smoke; workspace JS dependencies are unavailable"
            );
            return;
        }

        let fixture = EngineFixture::new(|path| Engine::new(path));
        let service = fixture.engine();
        service
            .create_tenant(TenantId::new("demo").expect("tenant id should parse"))
            .expect("tenant should create");
        let app_dir = tempdir().expect("app tempdir should build");
        write_generated_framework_http_fixture(app_dir.path());
        let output = run_cloud_functions_codegen(app_dir.path());
        assert!(
            output.status.success(),
            "cloud functions codegen should pass\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(service)
                .with_cloud_functions(registry)
                .build(),
        )
        .await;

        let response = server
            .client()
            .post(server.http_url("/hello?name=jack"))
            .header("x-test", "present")
            .json(&serde_json::json!({ "hello": "world" }))
            .send()
            .await
            .expect("request should send");

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response
                .headers()
                .get("x-nimbus-surface")
                .and_then(|value| value.to_str().ok()),
            Some("framework")
        );
        assert_eq!(
            response
                .json::<Value>()
                .await
                .expect("response body should decode"),
            serde_json::json!({
                "method": "POST",
                "path": "/hello",
                "originalUrl": server.http_url("/hello?name=jack"),
                "query": {
                    "name": "jack",
                },
                "body": {
                    "hello": "world",
                },
                "header": "present",
            })
        );
    }

    #[tokio::test]
    async fn cloud_functions_http_handler_runs_generated_firebase_onrequest_bundle_end_to_end() {
        let repo_root = repo_root();
        if !workspace_codegen_dependencies_available(&repo_root) {
            eprintln!(
                "skipping generated firebase onRequest() end-to-end smoke; workspace JS dependencies are unavailable"
            );
            return;
        }

        let fixture = EngineFixture::new(|path| Engine::new(path));
        let service = fixture.engine();
        service
            .create_tenant(TenantId::new("demo").expect("tenant id should parse"))
            .expect("tenant should create");
        let app_dir = tempdir().expect("app tempdir should build");
        write_generated_firebase_onrequest_fixture(app_dir.path());
        let output = run_cloud_functions_codegen(app_dir.path());
        assert!(
            output.status.success(),
            "cloud functions codegen should pass\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(service)
                .with_cloud_functions(registry)
                .build(),
        )
        .await;

        let response = server
            .client()
            .post(server.http_url("/hello?name=jack"))
            .header("x-test", "present")
            .json(&serde_json::json!({ "hello": "world" }))
            .send()
            .await
            .expect("request should send");

        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(
            response
                .headers()
                .get("x-nimbus-http")
                .and_then(|value| value.to_str().ok()),
            Some("/hello")
        );
        assert_eq!(
            response
                .json::<Value>()
                .await
                .expect("response body should decode"),
            serde_json::json!({
                "method": "POST",
                "path": "/hello",
                "originalUrl": server.http_url("/hello?name=jack"),
                "query": {
                    "name": "jack",
                },
                "body": {
                    "hello": "world",
                },
                "rawBody": "{\"hello\":\"world\"}",
                "header": "present",
            })
        );
    }

    #[tokio::test]
    async fn cloud_functions_callable_handler_runs_generated_firebase_oncall_bundle_end_to_end() {
        let repo_root = repo_root();
        if !workspace_codegen_dependencies_available(&repo_root) {
            eprintln!(
                "skipping generated firebase onCall() end-to-end smoke; workspace JS dependencies are unavailable"
            );
            return;
        }

        let fixture = EngineFixture::new(|path| Engine::new(path));
        let service = fixture.engine();
        service
            .create_tenant(TenantId::new("demo").expect("tenant id should parse"))
            .expect("tenant should create");
        let app_dir = tempdir().expect("app tempdir should build");
        write_generated_firebase_oncall_fixture(app_dir.path());
        let output = run_cloud_functions_codegen(app_dir.path());
        assert!(
            output.status.success(),
            "cloud functions codegen should pass\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );

        let registry = CloudFunctionsRegistry::from_app_dir(app_dir.path())
            .expect("cloud functions registry should load");
        let server = ServerFixture::start(
            crate::router::RouterBuildConfig::core(service)
                .with_cloud_functions(registry)
                .build(),
        )
        .await;
        let allowed_origin = server.http_url("").trim_end_matches('/').to_string();

        let success = server
            .client()
            .post(server.http_url("/hello"))
            .header("origin", &allowed_origin)
            .header("firebase-instance-id-token", "iid-123")
            .json(&serde_json::json!({ "data": { "hello": "world" } }))
            .send()
            .await
            .expect("callable request should send");

        assert_eq!(success.status(), StatusCode::OK);
        assert_eq!(
            success
                .headers()
                .get("access-control-allow-origin")
                .and_then(|value| value.to_str().ok()),
            Some(allowed_origin.as_str())
        );
        assert_eq!(
            success
                .json::<Value>()
                .await
                .expect("callable success body should decode"),
            serde_json::json!({
                "data": {
                    "acceptsStreaming": false,
                    "app": null,
                    "auth": null,
                    "data": {
                        "hello": "world",
                    },
                    "instanceIdToken": "iid-123",
                    "path": "/hello",
                    "sendChunkType": "function",
                },
            })
        );

        let failure = server
            .client()
            .post(server.http_url("/hello"))
            .header("origin", &allowed_origin)
            .json(&serde_json::json!({ "data": { "fail": true } }))
            .send()
            .await
            .expect("callable failure request should send");

        assert_eq!(failure.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            failure
                .json::<Value>()
                .await
                .expect("callable failure body should decode"),
            serde_json::json!({
                "error": {
                    "status": "INVALID_ARGUMENT",
                    "message": "bad input",
                    "details": {
                        "reason": "fail",
                    },
                },
            })
        );
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
        let bundle_sha256 = nimbus_runtime::RuntimeBundle::compute_sha256_for_path(&bundle_path)
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

    fn write_generated_framework_http_fixture(app_dir: &Path) {
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

functions.http("helloWorld", async (req, res) => {
  res.status(201).set("x-nimbus-surface", "framework").json({
    method: req.method,
    path: req.path,
    originalUrl: req.originalUrl,
    query: req.query,
    body: req.body,
    header: req.get("x-test"),
  });
});
"#,
        )
        .expect("framework source fixture should write");
        fs::write(
            app_dir
                .join(CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR)
                .join(CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE),
            serde_json::to_vec_pretty(
                &CloudFunctionsTargetsManifest::v1(vec![CloudFunctionsTargetDefinition {
                    name: "helloWorld".to_string(),
                    entrypoint: "registry.helloWorld".to_string(),
                    authoring_surface: CloudFunctionsAuthoringSurface::FunctionsFramework,
                    signature_type: CloudFunctionsSignatureType::Http,
                    binding: CloudFunctionsTargetBinding::Https {
                        exposure: CloudFunctionsHttpExposure::Http,
                        path: "/hello".to_string(),
                        execution: CloudFunctionsExecutionPrincipal::RequestPrincipal,
                    },
                }])
                .expect("framework targets should validate"),
            )
            .expect("framework targets should encode"),
        )
        .expect("framework targets should write");
    }

    fn write_generated_firebase_onrequest_fixture(app_dir: &Path) {
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
import { onRequest } from "firebase-functions/v2/https";

export const hello = onRequest(async (req, res) => {
  res.status(202).set("x-nimbus-http", req.path).json({
    method: req.method,
    path: req.path,
    originalUrl: req.originalUrl,
    query: req.query,
    body: req.body,
    rawBody: req.rawBody,
    header: req.get("x-test"),
  });
});
"#,
        )
        .expect("firebase onRequest source fixture should write");
    }

    fn write_generated_firebase_oncall_fixture(app_dir: &Path) {
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
import { HttpsError, onCall } from "firebase-functions/v2/https";

export const hello = onCall(async (request, response) => {
  if (request.data?.fail) {
    throw new HttpsError("invalid-argument", "bad input", {
      reason: "fail",
    });
  }

  return {
    acceptsStreaming: request.acceptsStreaming,
    app: request.app ?? null,
    auth: request.auth ?? null,
    data: request.data,
    instanceIdToken: request.instanceIdToken ?? null,
    path: request.rawRequest.path,
    sendChunkType: typeof response.sendChunk,
  };
});
"#,
        )
        .expect("firebase onCall source fixture should write");
    }
}
