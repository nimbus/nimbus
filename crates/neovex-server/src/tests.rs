use std::fs;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::SystemTime;

use axum::{Json, Router, extract::State, routing::get};
use base64::Engine;
use neovex_core::{TableName, TenantId};
use neovex_engine::{Service, run_scheduler};
use neovex_runtime::{RuntimeBundle, RuntimeLimits};
use neovex_test_support::{HttpApiFixture, ServerFixture, ServiceFixture, WebSocketFixture};
use reqwest::StatusCode;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair};
use serde_json::json;
use tempfile::tempdir;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::time::{Duration, timeout};

use crate::{
    ConvexRegistry, LicenseDocument, LicenseEntitlements, LicenseKind, LicenseSourceInfo,
    LicenseSourceKind, LicenseState, build_router, build_router_with_convex,
    build_router_with_license,
};

#[test]
fn async_runtime_integration_removes_hot_path_blocking_adapters() {
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let engine_service_mod =
        fs::read_to_string(workspace_root.join("../neovex-engine/src/service/mod.rs"))
            .expect("engine service module should be readable");
    assert!(
        !engine_service_mod.contains("call_blocking("),
        "engine service should not retain the call_blocking adapter"
    );

    let async_host_calls =
        fs::read_to_string(workspace_root.join("src/runtime/host_calls/async_calls.rs"))
            .expect("runtime async host call module should be readable");
    assert!(
        !async_host_calls.contains("spawn_blocking("),
        "runtime async host calls should await real futures instead of spawn_blocking wrappers"
    );
    assert!(
        !async_host_calls.contains("execute_async_blocking_host_call"),
        "runtime async host calls should not retain the blocking adapter helper"
    );
}

fn convex_registry(functions: serde_json::Value) -> ConvexRegistry {
    convex_registry_with_routes(functions, json!([]))
}

fn convex_registry_with_routes(
    functions: serde_json::Value,
    routes: serde_json::Value,
) -> ConvexRegistry {
    convex_registry_with_routes_and_bundle_and_auth(functions, routes, None, None)
}

fn convex_registry_with_routes_and_bundle(
    functions: serde_json::Value,
    routes: serde_json::Value,
    bundle: Option<&str>,
) -> ConvexRegistry {
    convex_registry_with_routes_and_bundle_and_auth(functions, routes, bundle, None)
}

fn convex_registry_with_routes_and_bundle_and_auth(
    functions: serde_json::Value,
    routes: serde_json::Value,
    bundle: Option<&str>,
    auth_config: Option<serde_json::Value>,
) -> ConvexRegistry {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".neovex").join("convex");
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
    let registry =
        ConvexRegistry::from_app_dir(tempdir.path()).expect("convex registry should load");
    std::mem::forget(tempdir);
    registry
}

async fn open_json_post_stream(
    server: &ServerFixture,
    path: &str,
    body: &serde_json::Value,
) -> TcpStream {
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
    stream
}

async fn wait_for_runtime_metrics(
    registry: &ConvexRegistry,
    description: &str,
    predicate: impl Fn(&neovex_runtime::RuntimeMetricsSnapshot) -> bool,
) -> neovex_runtime::RuntimeMetricsSnapshot {
    let started_at = tokio::time::Instant::now();
    loop {
        let metrics = registry.runtime_metrics_snapshot();
        if predicate(&metrics) {
            return metrics;
        }
        assert!(
            started_at.elapsed() < Duration::from_secs(3),
            "timed out waiting for {description}; last runtime metrics: {metrics:?}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
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
#[path = "tests/registry_and_license/mod.rs"]
mod registry_and_license;
#[path = "tests/scheduling.rs"]
mod scheduling;
