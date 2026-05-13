use super::*;
use std::sync::Arc;
use std::time::Duration;

use nimbus_sandbox::backends::krun::{KrunLaunchMode, KrunSandboxBackend};
use nimbus_server::build_router_with_convex_and_sandbox_service_manager;
use nimbus_testing::{HttpApiFixture, ServerFixture, ServiceFixture, wait_for_condition};
use tempfile::tempdir;

#[tokio::test]
#[ignore = "requires Linux KVM host with krun toolchain"]
async fn convex_runtime_query_starts_real_krun_service_from_compose_file_and_tears_it_down() {
    let tempdir = tempdir().expect("compose + convex tempdir should build");
    let tenant_id = nimbus::TenantId::new("demo").expect("tenant id should be valid");
    let host_port = env_u16("NIMBUS_KRUN_SMOKE_M5_HOST_PORT").unwrap_or(18091);
    let guest_port = env_u16("NIMBUS_KRUN_SMOKE_M5_GUEST_PORT").unwrap_or(8091);
    let compose_path = write_compose_smoke_fixture(tempdir.path(), host_port, guest_port);
    let registry = write_convex_service_query_fixture(tempdir.path());

    let base_dir = env_path("NIMBUS_KRUN_SMOKE_WORKDIR");
    let control_data_dir = base_dir.join("m5-compose-control");
    let context = crate::compose::load_compose_project_context(&compose_path, &control_data_dir)
        .expect("compose project context should load");
    if let Some(metadata_path) = env::var_os("NIMBUS_KRUN_SMOKE_M5_METADATA_FILE") {
        let metadata_path = PathBuf::from(metadata_path);
        if let Some(parent) = metadata_path.parent() {
            fs::create_dir_all(parent).expect("metadata parent should build");
        }
        fs::write(
            &metadata_path,
            serde_json::to_vec_pretty(&json!({
                "project_root": context.control_plane.project_root,
                "project_key": context.control_plane.project_key,
            }))
            .expect("metadata json should serialize"),
        )
        .expect("metadata file should write");
    }
    println!(
        "M5_PROJECT_ROOT={}",
        context.control_plane.project_root.display()
    );
    println!("M5_PROJECT_KEY={}", context.control_plane.project_key);
    let mut config = context.control_plane.krun_backend_config();
    config.launch_mode = KrunLaunchMode::Execute;
    if let Some(runtime_path) = env::var_os("NIMBUS_KRUN_SMOKE_RUNTIME") {
        config.runtime_path = runtime_path.into();
    }
    if let Some(conmon_path) = env::var_os("NIMBUS_KRUN_SMOKE_CONMON") {
        config.conmon_path = conmon_path.into();
    }
    if let Some(buildah_path) = env::var_os("NIMBUS_KRUN_SMOKE_BUILDAH") {
        config.buildah_path = buildah_path.into();
    }

    let sandbox_service_manager = Arc::new(
        crate::compose::load_sandbox_service_manager(
            &compose_path,
            Arc::new(KrunSandboxBackend::new(config)),
        )
        .expect("compose-backed sandbox service manager should load")
        .with_activation_poll_interval(Duration::from_millis(50))
        .with_activation_timeout(Duration::from_secs(30)),
    );
    let fixture = ServiceFixture::new(|path| nimbus::Service::new(path));
    let server = ServerFixture::start(build_router_with_convex_and_sandbox_service_manager(
        fixture.service(),
        registry,
        sandbox_service_manager.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        reqwest::StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "services:activate", json!({}))
        .await;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let port = response
        .json::<serde_json::Value>()
        .await
        .expect("activation response should parse")
        .as_u64()
        .expect("port should be numeric");
    assert_eq!(port, u64::from(host_port));

    let http_response = wait_for_http_response(host_port, Duration::from_secs(15)).await;
    assert!(
        http_response.starts_with("HTTP/1.") || http_response.contains("404"),
        "expected HTTP response from compose-backed krun service, got: {http_response}"
    );
    assert!(
        sandbox_service_manager
            .sandboxes_for_tenant(&tenant_id)
            .contains_key("db"),
        "compose-backed manager should expose the declared db binding"
    );

    let delete = api.delete_tenant("demo").await;
    assert_eq!(delete.status(), reqwest::StatusCode::NO_CONTENT);
    wait_for_condition(
        "compose-backed krun service should disappear after tenant deletion",
        Duration::from_secs(10),
        Duration::from_millis(100),
        || async {
            reqwest::get(format!("http://127.0.0.1:{host_port}/"))
                .await
                .is_err()
                && sandbox_service_manager
                    .sandboxes_for_tenant(&tenant_id)
                    .is_empty()
        },
    )
    .await;
}
