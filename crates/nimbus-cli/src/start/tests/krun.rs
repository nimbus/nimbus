use super::*;
use std::time::Duration;

use nimbus_server::{RouterOptions, build_router};
use nimbus_testing::{EngineFixture, HttpApiFixture, ServerFixture, wait_for_condition};
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
    let selection = crate::compose::discovery::ResolvedComposeSelection::explicit(compose_path);
    let network_path = base_dir.join("m5-network");
    let network_root =
        nimbus_operator::LocalNodeNetworkRoot::resolve_for_current_platform(Some(&network_path))
            .expect("smoke network root should resolve");
    let staged_network =
        crate::network_composition::StagedLocalNetworkComposition::claim(&network_root)
            .expect("smoke network manager should claim");
    let prepared_network = crate::network_composition::PreparedLocalNetworkComposition::prepare(
        staged_network,
        Some(&selection),
        &control_data_dir,
        nimbus_tenant::TenantIsolationMode::LocalDevelopment,
        nimbus_server::nimbus_owned_local_ingress_registration(false),
    )
    .expect("compose-backed network composition should prepare");
    let service_manager = prepared_network
        .local_service_manager()
        .expect("compose-backed service manager should load");
    let fixture = EngineFixture::new(|path| nimbus::Engine::new(path));
    let server = ServerFixture::start(build_router(
        RouterOptions::new(fixture.engine(), prepared_network.manager())
            .with_convex_registry(registry)
            .with_service_manager(service_manager.clone()),
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
        service_manager
            .service_instances_for_tenant(&tenant_id)
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
                && service_manager
                    .service_instances_for_tenant(&tenant_id)
                    .is_empty()
        },
    )
    .await;
}
