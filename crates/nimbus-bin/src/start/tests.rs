use std::fs;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clap::{Parser, error::ErrorKind};
use nimbus::RuntimeLimits;
use serde_json::json;

use super::config::{
    CliTenantProvider, PersistenceEnv, PersistenceFileConfig, load_runtime_config_file,
    persistence_config_from_sources,
};
use super::*;
use crate::codegen::CodegenCommand;
use crate::test_support::with_current_dir;
use crate::{Cli, Command};

use std::env;
#[cfg(target_os = "linux")]
use std::time::Duration;
#[cfg(target_os = "linux")]
use std::time::Instant;

#[cfg(target_os = "linux")]
use nimbus::{ConvexRegistry, RuntimeBundle, SandboxCatalog};
#[cfg(target_os = "linux")]
use nimbus_testing::run_to_completion_snapshot_runtime_test_limits;

static TEST_CONFIG_COUNTER: AtomicU64 = AtomicU64::new(0);

fn write_test_config(contents: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "nimbus-bin-config-{}-{}.json",
        std::process::id(),
        TEST_CONFIG_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, contents).expect("test config file should write");
    path
}

fn parse_start<I, T>(args: I) -> StartCommand
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    let Command::Start(command) = cli.command else {
        panic!("start subcommand should parse");
    };
    *command
}

fn parse_codegen<I, T>(args: I) -> CodegenCommand
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);
    let Command::Codegen(command) = cli.command else {
        panic!("codegen subcommand should parse");
    };
    command
}

mod app_dir_codegen;
mod cli_surface;
mod encryption;
#[cfg(target_os = "linux")]
mod krun;
mod license;
mod persistence;

#[cfg(target_os = "linux")]
fn write_compose_smoke_fixture(root: &Path, host_port: u16, guest_port: u16) -> PathBuf {
    let compose_path = root.join("compose.yaml");
    fs::write(
        &compose_path,
        format!(
            r#"
name: Smoke App
services:
  db:
    image: busybox:latest
    ports:
      - "{host_port}:{guest_port}"
    command:
      - /bin/busybox
      - httpd
      - -f
      - -p
      - "{guest_port}"
    stop_grace_period: 5s
"#
        ),
    )
    .expect("compose smoke fixture should write");
    compose_path
}

fn tempdir_in_repo_target() -> tempfile::TempDir {
    let repo_root = repo_root();
    let target_dir = repo_root.join("target");
    fs::create_dir_all(&target_dir).expect("repo target dir should exist");
    tempfile::tempdir_in(&target_dir).expect("tempdir in repo target should create")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate manifest dir should have repo root")
        .to_path_buf()
}

fn workspace_codegen_dependencies_available() -> bool {
    let repo_root = repo_root();
    repo_root.join("packages/codegen/src/main.mjs").is_file()
        && (repo_root.join("node_modules/esbuild").is_dir()
            || repo_root
                .join("packages/codegen/node_modules/esbuild")
                .is_dir())
}

fn write_codegen_source_fixture(app_dir: &Path) {
    let convex_dir = app_dir.join("convex");
    fs::create_dir_all(&convex_dir).expect("convex source dir should create");
    fs::write(
        convex_dir.join("messages.ts"),
        r#"
import { query } from "./_generated/server";

export const list = query({
  args: {},
  handler: async () => [],
});
"#,
    )
    .expect("convex source fixture should write");
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
import { onDocumentCreated } from "firebase-functions/v2/firestore";

export const syncUser = onDocumentCreated("users/{userId}", async (event) => event);
"#,
    )
    .expect("firebase source fixture should write");
}

fn write_generated_cloud_functions_artifacts(app_dir: &Path) {
    let firebase_dir = app_dir.join(".nimbus").join("firebase");
    fs::create_dir_all(&firebase_dir).expect("firebase manifest directory should build");
    fs::write(
        firebase_dir.join("artifact.json"),
        r#"{"version":1,"family":"cloud_functions","runtime_bundle":{"entry_file":"bundle.mjs","sha256_file":"bundle.sha256"},"targets_manifest":"targets.json","import_resolution":{"strategy":"deploy_alias_layer","covered_specifiers":["@google-cloud/functions-framework","firebase-admin/app","firebase-admin/firestore","firebase-functions/v2","firebase-functions/v2/firestore","firebase-functions/v2/https"]}}"#,
    )
    .expect("artifact manifest should write");
    fs::write(
        firebase_dir.join("targets.json"),
        r#"{"version":1,"targets":[]}"#,
    )
    .expect("targets should write");
    fs::write(firebase_dir.join("bundle.mjs"), "export const value = 1;\n")
        .expect("bundle should write");
    fs::write(firebase_dir.join("bundle.sha256"), "a".repeat(64)).expect("bundle sha should write");
}

fn write_framework_cloud_functions_fixture(app_dir: &Path) {
    let source_dir = app_dir.join("src");
    let generated_dir = app_dir.join(".nimbus").join("firebase");
    fs::create_dir_all(&source_dir).expect("framework source dir should create");
    fs::create_dir_all(&generated_dir).expect("framework generated dir should create");
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
        generated_dir.join("targets.json"),
        r#"{
  "version": 1,
  "targets": [
    {
      "name": "syncUser",
      "entrypoint": "registry.syncUser",
      "authoring_surface": "functions_framework",
      "signature_type": "cloud_event",
      "binding": {
        "binding_kind": "firestore_document",
        "event_type": "google.cloud.firestore.document.v1.written",
        "database": "(default)",
        "document": "users/{userId}",
        "execution": "service"
      }
    }
  ]
}
"#,
    )
    .expect("framework targets manifest should write");
    fs::write(
        source_dir.join("index.ts"),
        r#"
import functions from "@google-cloud/functions-framework";

functions.cloudEvent("syncUser", async (event) => event);
"#,
    )
    .expect("framework source fixture should write");
}

#[cfg(target_os = "linux")]
fn write_convex_service_query_fixture(app_dir: &Path) -> ConvexRegistry {
    let convex_dir = app_dir.join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [{
                "name": "services:activate",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => ctx.services.db.port"
            }]
        }))
        .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex routes json should serialize"),
    )
    .expect("convex routes manifest should write");

    let bundle_path = convex_dir.join("bundle.mjs");
    fs::write(
        &bundle_path,
        r#"
const definitions = new Map([
  ["services:activate", {
    name: "services:activate",
    kind: "query",
    runtime_handler: "async (ctx) => ctx.services.db.port",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__nimbusInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__nimbusCreateContext({
          request,
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
    }
    throw error;
  }
};

export {};
"#,
    )
    .expect("convex runtime bundle should write");
    let bundle_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    fs::write(
        bundle_path.with_extension("sha256"),
        format!("{bundle_sha256}\n"),
    )
    .expect("convex runtime bundle hash should write");

    ConvexRegistry::from_app_dir(app_dir)
        .expect("convex registry should load")
        .with_runtime_limits(run_to_completion_snapshot_runtime_test_limits())
}

#[cfg(target_os = "linux")]
fn env_path(name: &str) -> PathBuf {
    PathBuf::from(env::var_os(name).unwrap_or_else(|| panic!("missing env var {name}")))
}

#[cfg(target_os = "linux")]
fn env_u16(name: &str) -> Option<u16> {
    env::var(name).ok().map(|value| {
        value
            .parse::<u16>()
            .unwrap_or_else(|error| panic!("invalid {name} value {value:?}: {error}"))
    })
}

#[cfg(target_os = "linux")]
async fn wait_for_http_response(host_port: u16, timeout: Duration) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(response) = reqwest::get(format!("http://127.0.0.1:{host_port}/")).await {
            let status = response.status();
            if let Ok(body) = response.text().await {
                return format!("HTTP/1.1 {status}\n{body}");
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for HTTP response on port {host_port}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

// -------------------------------------------------------------------------
// Encryption config tests
// -------------------------------------------------------------------------
