use super::*;
use std::time::Duration;

use nimbus_server::serve_leased;
use nimbus_testing::{EngineFixture, wait_for_condition};
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
        nimbus_server::nimbus_owned_workload_ingress_registration(),
    )
    .expect("compose-backed network composition should prepare");
    let service_manager = prepared_network
        .local_service_manager()
        .expect("compose-backed service manager should load");
    let fixture = EngineFixture::new(|path| nimbus::Engine::new(path));
    let options = prepared_network
        .prepare_server_workload_profile()
        .expect("compose-backed server profile should prepare")
        .complete(fixture.engine())
        .expect("compose-backed server profile should complete with the caller engine")
        .with_convex_registry(registry);
    let requested_addr = "127.0.0.1:0"
        .parse()
        .expect("provider-assigned test address should parse");
    let prepared_listener = options
        .prepare_main_listener(requested_addr)
        .expect("managed server listener should reserve");
    let listener = tokio::net::TcpListener::bind(requested_addr)
        .await
        .expect("managed server listener should bind");
    let listener = prepared_listener
        .adopt(listener)
        .expect("managed server listener should activate");
    let server_addr = listener
        .local_addr()
        .expect("managed server address should resolve");
    let server = tokio::spawn(async move {
        serve_leased(listener, options)
            .await
            .expect("managed server should run");
    });
    let client = reqwest::Client::new();
    let base_url = format!("http://{server_addr}");

    assert_eq!(
        client
            .post(format!("{base_url}/api/tenants"))
            .json(&json!({ "id": "demo" }))
            .send()
            .await
            .expect("tenant request should succeed")
            .status(),
        reqwest::StatusCode::CREATED
    );

    let response = client
        .post(format!("{base_url}/convex/demo/query"))
        .json(&json!({ "name": "services:activate", "args": {} }))
        .send()
        .await;
    let response = response.expect("service activation query should succeed");
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

    let delete = client
        .delete(format!("{base_url}/api/tenants/demo"))
        .send()
        .await
        .expect("tenant deletion request should succeed");
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
    server.abort();
    let _ = server.await;
}
