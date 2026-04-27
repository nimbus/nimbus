use std::collections::HashMap;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::extract::{OriginalUri, Query as AxumQuery, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::Response;
use neovex_core::{Error, Result, TenantId};
use neovex_runtime::{InvocationAuth, InvocationKind, InvocationRequest};
use serde::Deserialize;
use serde_json::Value;

mod callable;

use callable::{CallableHttpRequest, handle_callable_target};

use super::host_bridge::CloudFunctionsHostBridge;
use super::{CloudFunctionsHttpExposure, CloudFunctionsRegistry, CloudFunctionsTargetBinding};
use crate::application_auth::normalize_principal_context;
use crate::execution::errors::runtime_error_to_core;
use crate::execution::invocations::{
    RuntimeBundleInvocationOptions, invoke_runtime_bundle_blocking_with_host,
    next_runtime_server_request_id,
};
use crate::runtime_host::{RuntimeHostInvocation, RuntimeHostScope};
use crate::state::{AppError, AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CloudFunctionsHttpBodyKind {
    Json,
    Text,
}

#[derive(Debug, Deserialize)]
struct CloudFunctionsHttpResponseEnvelope {
    #[serde(default)]
    status: Option<u16>,
    #[serde(default)]
    headers: Option<HashMap<String, Value>>,
    #[serde(default)]
    body_kind: Option<CloudFunctionsHttpBodyKind>,
    #[serde(default)]
    body: Value,
}

pub(crate) async fn http_handler(
    State(state): State<Arc<AppState>>,
    method: Method,
    headers: HeaderMap,
    original_uri: OriginalUri,
    query: AxumQuery<HashMap<String, String>>,
    body: Bytes,
) -> std::result::Result<Response, AppError> {
    let Some(registry) = state.cloud_functions_registry.current() else {
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
                &original_uri,
                &request_path,
                query.0,
                body,
            )?;
            execute_http_target(state, registry, tenant_id, entrypoint, args, None)
        }
        CloudFunctionsHttpExposure::Callable => {
            handle_callable_target(
                state,
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

fn resolve_cloud_functions_http_tenant(
    state: &AppState,
) -> std::result::Result<TenantId, AppError> {
    let tenants = state.service.list_tenants().map_err(AppError::from)?;
    match tenants.as_slice() {
        [tenant_id] => Ok(tenant_id.clone()),
        [] => Err(AppError::from(Error::Conflict(
            "cloud functions http handlers require exactly one tenant, but no tenants exist"
                .to_string(),
        ))),
        _ => Err(AppError::from(Error::Conflict(
            "cloud functions http handlers require exactly one tenant; explicit multi-tenant HTTP binding is deferred to a later cloud functions phase"
                .to_string(),
        ))),
    }
}

fn build_http_request_args(
    method: &Method,
    headers: &HeaderMap,
    original_uri: &OriginalUri,
    request_path: &str,
    query: HashMap<String, String>,
    body: Bytes,
) -> Result<Value> {
    let normalized_headers = normalized_headers(headers);
    let raw_body = if body.is_empty() {
        String::new()
    } else {
        std::str::from_utf8(&body)
            .map_err(|error| {
                Error::InvalidInput(format!(
                    "cloud functions http handlers only cover UTF-8 request bodies in the first slice: {error}"
                ))
            })?
            .to_string()
    };
    let body = if raw_body.is_empty() {
        Value::Null
    } else if header_value_contains(headers, header::CONTENT_TYPE, "json") {
        serde_json::from_str(&raw_body).map_err(|error| {
            Error::InvalidInput(format!(
                "cloud functions http handler could not parse JSON request body: {error}"
            ))
        })?
    } else {
        Value::String(raw_body.clone())
    };

    Ok(serde_json::json!({
        "method": method.as_str(),
        "path": request_path,
        "original_url": request_url(headers, original_uri, request_path),
        "query": query,
        "headers": normalized_headers,
        "body": body,
        "raw_body": raw_body,
    }))
}

fn request_url(headers: &HeaderMap, original_uri: &OriginalUri, request_path: &str) -> String {
    let scheme = headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("http");
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    let query_suffix = original_uri
        .0
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    format!("{scheme}://{host}{request_path}{query_suffix}")
}

fn normalized_headers(headers: &HeaderMap) -> HashMap<String, String> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect()
}

fn header_value_contains(headers: &HeaderMap, name: header::HeaderName, needle: &str) -> bool {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains(needle))
}

fn execute_http_target(
    state: Arc<AppState>,
    registry: Arc<CloudFunctionsRegistry>,
    tenant_id: TenantId,
    function_name: String,
    args: Value,
    auth: Option<InvocationAuth>,
) -> std::result::Result<Response, AppError> {
    let server_request_id = next_runtime_server_request_id("cloud-functions-http");
    let services = state
        .runtime_service_registry()
        .snapshot_for_tenant(&tenant_id);
    let request = InvocationRequest {
        kind: InvocationKind::Mutation,
        function_name,
        args,
        page_size: None,
        cursor: None,
        auth: auth.clone(),
        services: services.clone(),
    };
    let bridge = Arc::new(CloudFunctionsHostBridge::build(
        RuntimeHostScope::new(
            state.service.clone(),
            registry.runtime_policy(),
            tenant_id.clone(),
        ),
        RuntimeHostInvocation::new(
            normalize_principal_context(auth.as_ref()),
            Some(server_request_id.clone()),
            InvocationKind::Mutation,
        ),
    )?);

    let runtime_response = invoke_runtime_bundle_blocking_with_host(
        &registry.runtime_executor(),
        registry.runtime_policy(),
        bridge.clone(),
        registry.runtime_bundle(),
        request,
        RuntimeBundleInvocationOptions::enforcing_policy_limit(
            &tenant_id,
            Some(server_request_id.as_str()),
            None,
        ),
    )
    .map_err(runtime_error_to_core)?;
    let response = build_http_response(runtime_response)?;
    bridge.commit_mutation_execution_unit()?;
    Ok(response)
}

fn build_http_response(value: Value) -> std::result::Result<Response, AppError> {
    let envelope: CloudFunctionsHttpResponseEnvelope =
        serde_json::from_value(value).map_err(|error| {
            AppError::from(Error::InvalidInput(format!(
                "cloud functions http handler must return a response envelope: {error}"
            )))
        })?;
    let status = envelope
        .status
        .map(StatusCode::from_u16)
        .transpose()
        .map_err(|error| {
            AppError::from(Error::InvalidInput(format!(
                "cloud functions http handler returned an invalid status code: {error}"
            )))
        })?
        .unwrap_or(StatusCode::OK);
    let mut builder = Response::builder().status(status);
    let mut has_content_type = false;

    for (name, value) in parse_headers(envelope.headers)? {
        if name.eq_ignore_ascii_case("content-type") {
            has_content_type = true;
        }
        builder = builder.header(name, value);
    }

    let body_kind = envelope.body_kind.unwrap_or(match &envelope.body {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
            CloudFunctionsHttpBodyKind::Text
        }
        _ => CloudFunctionsHttpBodyKind::Json,
    });
    if matches!(body_kind, CloudFunctionsHttpBodyKind::Json) && !has_content_type {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let body = match body_kind {
        CloudFunctionsHttpBodyKind::Json => serde_json::to_vec(&envelope.body)
            .map_err(|error| AppError::from(Error::Serialization(error.to_string())))?,
        CloudFunctionsHttpBodyKind::Text => render_text_body(envelope.body)?,
    };
    builder.body(Body::from(body)).map_err(|error| {
        AppError::from(Error::Internal(format!(
            "cloud functions http response could not build: {error}"
        )))
    })
}

fn parse_headers(
    headers: Option<HashMap<String, Value>>,
) -> std::result::Result<Vec<(String, String)>, AppError> {
    let Some(headers) = headers else {
        return Ok(Vec::new());
    };
    headers
        .into_iter()
        .filter_map(|(name, value)| match value {
            Value::Null => None,
            Value::String(value) => Some(Ok((name, value))),
            Value::Number(value) => Some(Ok((name, value.to_string()))),
            Value::Bool(value) => Some(Ok((name, value.to_string()))),
            _ => Some(Err(AppError::from(Error::InvalidInput(format!(
                "cloud functions http header `{name}` must resolve to a string-coercible value"
            ))))),
        })
        .collect()
}

fn render_text_body(body: Value) -> std::result::Result<Vec<u8>, AppError> {
    match body {
        Value::Null => Ok(Vec::new()),
        Value::String(value) => Ok(value.into_bytes()),
        Value::Bool(value) => Ok(value.to_string().into_bytes()),
        Value::Number(value) => Ok(value.to_string().into_bytes()),
        _ => Err(AppError::from(Error::InvalidInput(
            "cloud functions http text responses must resolve to a string-coercible value"
                .to_string(),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use neovex_core::{Query, TableName};
    use neovex_engine::Service;
    use neovex_testing::{ServerFixture, ServiceFixture};
    use reqwest::StatusCode;
    use tempfile::tempdir;

    use super::*;
    use crate::adapters::cloud_functions::{
        CLOUD_FUNCTIONS_ARTIFACT_MANIFEST_FILE, CLOUD_FUNCTIONS_INTERNAL_ARTIFACT_DIR,
        CLOUD_FUNCTIONS_TARGETS_MANIFEST_FILE, CloudFunctionsArtifactManifest,
        CloudFunctionsAuthoringSurface, CloudFunctionsExecutionBinding, CloudFunctionsHttpExposure,
        CloudFunctionsSignatureType, CloudFunctionsTargetBinding, CloudFunctionsTargetDefinition,
        CloudFunctionsTargetsManifest,
    };

    #[tokio::test]
    async fn cloud_functions_http_handler_dispatches_exact_path_and_commits_writes() {
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
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
                    execution: CloudFunctionsExecutionBinding::Request,
                },
            }],
            r#"
globalThis.__neovexInvoke = async function (request) {
  if (request.function_name !== "registry.helloWorld") {
    throw new Error(`unknown handler ${request.function_name}`);
  }
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `http:${request.function_name}`,
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
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
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
                    execution: CloudFunctionsExecutionBinding::Request,
                },
            }],
            r#"
globalThis.__neovexInvoke = async function () {
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
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
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
                    execution: CloudFunctionsExecutionBinding::Request,
                },
            }],
            r#"
globalThis.__neovexInvoke = async function (request) {
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
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
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
                    execution: CloudFunctionsExecutionBinding::Request,
                },
            }],
            r#"
globalThis.__neovexInvoke = async function () {
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
        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
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
                    execution: CloudFunctionsExecutionBinding::Request,
                },
            }],
            r#"
globalThis.__neovexInvoke = async function () {
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
                    "message": "no auth providers are configured; check convex/auth.config.ts",
                },
            })
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

        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
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
                .get("x-neovex-surface")
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

        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
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
                .get("x-neovex-http")
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

        let fixture = ServiceFixture::new(|path| Service::new(path));
        let service = fixture.service();
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
  res.status(201).set("x-neovex-surface", "framework").json({
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
                        execution: CloudFunctionsExecutionBinding::Request,
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
  res.status(202).set("x-neovex-http", req.path).json({
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
