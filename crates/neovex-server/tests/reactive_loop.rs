use std::fs;

use neovex_engine::{Service, run_scheduler};
use neovex_runtime::RuntimeBundle;
use neovex_server::{ConvexRegistry, build_router, build_router_with_convex};
use neovex_test_support::{
    BlockingFaultInjector, DeterministicHarness, HttpApiFixture, ScenarioMetadata, ServerFixture,
    ServiceFixture, WebSocketFixture,
};
use serde_json::json;
use tempfile::tempdir;
use tokio::sync::watch;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Error as WebSocketError;

fn convex_registry(functions: serde_json::Value) -> ConvexRegistry {
    convex_registry_with_bundle(functions, None)
}

fn convex_registry_with_bundle(
    functions: serde_json::Value,
    bundle: Option<&str>,
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

#[path = "reactive_loop/manifest/mod.rs"]
mod manifest;
#[path = "reactive_loop/runtime_paginated/mod.rs"]
mod runtime_paginated;
#[path = "reactive_loop/runtime_queries.rs"]
mod runtime_queries;
#[path = "reactive_loop/socket/mod.rs"]
mod socket;
#[path = "reactive_loop/transport/mod.rs"]
mod transport;
