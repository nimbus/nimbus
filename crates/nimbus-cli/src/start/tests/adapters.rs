//! Integration smokes for the D7 adapter posture: a server booted from
//! CLI-resolved configs serves every surface by default — store-backed
//! credentials and all — and the `--no-*` opt-outs really disable them.

use std::sync::Arc;

use nimbus_process_harness::PortWindow;

use super::*;

fn adapter_serve_options(
    engine: &Arc<nimbus::Engine>,
    enablement: super::super::adapters::AdapterEnablement,
) -> nimbus_server::ServeOptions {
    enablement.apply_to(
        nimbus_server::ServeOptions::reconstruct_direct(engine.clone())
            .expect("test server authority should open"),
    )
}

#[test]
fn cloudflare_routes_refuse_non_loopback_main_bind_without_allow_network() {
    let temp = tempfile::tempdir().expect("tempdir should build");
    let command = StartCommand {
        host: "0.0.0.0".to_string(),
        firestore: false,
        mongodb: false,
        dynamodb: false,
        s3: false,
        cloudflare: true,
        ..StartCommand::default()
    };

    let error =
        super::super::adapters::resolve_adapter_enablement_with_env(&command, temp.path(), |_| {
            None
        })
        .expect_err("Cloudflare routes should share the main non-loopback bind guard");

    assert!(
        error.to_string().contains("Cloudflare routes")
            && error.to_string().contains("non-loopback"),
        "Cloudflare non-loopback refusal should name the shared bind guard: {error}"
    );
}

#[tokio::test]
async fn conventional_port_conflict_fails_through_shared_authority_with_guidance() {
    let temp = tempfile::tempdir().expect("tempdir should build");
    let command = StartCommand {
        firestore: false,
        cloudflare: false,
        mongodb: true,
        dynamodb: false,
        s3: false,
        ..StartCommand::default()
    };
    let enablement =
        super::super::adapters::resolve_adapter_enablement_with_env(&command, temp.path(), |_| {
            None
        })
        .expect("pure default desired state should resolve before availability is known");

    let conflict_authority =
        nimbus_server::PreboundServerListeners::reconstruct_direct(temp.path())
            .expect("test listener authority should open");
    let conventional_addr = format!(
        "127.0.0.1:{}",
        super::super::adapters::MONGODB_CONVENTIONAL_PORT
    )
    .parse()
    .expect("conventional MongoDB address should parse");
    let _conflicting_claim = conflict_authority
        .prepare("existing-mongodb-owner", conventional_addr)
        .expect("the earlier authority should fence the conventional port");

    let engine = Arc::new(nimbus::Engine::new(temp.path()).expect("engine should build"));
    let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("main listener should bind");
    let options = enablement.apply_to(
        nimbus_server::ServeOptions::reconstruct_direct(engine)
            .expect("test server authority should open"),
    );
    let error = nimbus_server::serve(main_listener, options)
        .await
        .expect_err("the durable conventional-port fence must fail startup");
    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
    assert!(
        error.to_string().contains("mongodb listener")
            && error.to_string().contains(&conventional_addr.to_string()),
        "the authoritative failure should identify its adapter and desired address: {error}"
    );

    let guided = super::super::boot::conventional_wire_port_guidance(&command, error);
    assert_eq!(guided.kind(), std::io::ErrorKind::AddrInUse);
    assert!(
        guided
            .to_string()
            .contains("MongoDB conventional port 27017 is busy")
            && guided.to_string().contains("--mongodb-port")
            && guided.to_string().contains("--no-mongodb"),
        "moving the decision to the shared authority must preserve recovery guidance: {guided}"
    );
}

#[tokio::test]
async fn cli_adapters_serve_store_backed_by_default_and_opt_outs_disable() {
    let temp = tempfile::tempdir().expect("tempdir should build");
    let engine =
        Arc::new(nimbus::Engine::new(temp.path().join("engine")).expect("engine should build"));
    let opted_out_engine = Arc::new(
        nimbus::Engine::new(temp.path().join("engine-opted-out"))
            .expect("opted-out engine should build"),
    );

    // One claimed window owns every sibling adapter port this fixture hands
    // the server, and stays alive for the whole case. That makes both
    // directions of the smoke sound: the served listeners cannot lose their
    // ports to another process, and the "listener must stay off" probes under
    // the opt-outs cannot be answered by an unrelated program that happened to
    // take the same number.
    let adapter_ports = PortWindow::claim();
    let mongodb_port = adapter_ports.port(0);
    let dynamodb_port = adapter_ports.port(1);
    let s3_port = adapter_ports.port(2);

    // Opt-outs first: a fully opted-out server mounts none of the
    // surfaces. (Runs before the serving server because sibling adapter
    // listeners spawned by `serve` outlive an aborted parent task.)
    let client = reqwest::Client::new();
    let opted_out_command = StartCommand {
        firestore: false,
        cloudflare: false,
        mongodb: false,
        dynamodb: false,
        s3: false,
        ..StartCommand::default()
    };
    let opted_out_enablement = super::super::adapters::resolve_adapter_enablement_with_env(
        &opted_out_command,
        temp.path(),
        |_| None,
    )
    .expect("opted-out enablement should resolve");
    let opted_out_listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("opted-out listener should bind");
    let opted_out_addr = opted_out_listener
        .local_addr()
        .expect("opted-out addr should resolve");
    let opted_out_task = tokio::spawn(nimbus_server::serve(
        opted_out_listener,
        adapter_serve_options(&opted_out_engine, opted_out_enablement),
    ));
    crate::test_support::wait_for_live_server_health(
        "opted-out smoke server should answer /health",
        opted_out_addr,
        &opted_out_task,
    )
    .await;
    let firestore_off = client
        .get(format!(
            "http://{opted_out_addr}/v1/projects/demo/databases/(default)/documents/notes"
        ))
        .send()
        .await
        .expect("firestore-off request should send");
    assert_eq!(
        firestore_off.status(),
        reqwest::StatusCode::NOT_FOUND,
        "firestore routes must unmount under --no-firestore"
    );
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", mongodb_port))
            .await
            .is_err(),
        "mongodb listener must stay off under --no-mongodb"
    );
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", s3_port))
            .await
            .is_err(),
        "s3 listener must stay off under --no-s3"
    );
    opted_out_task.abort();
    let _ = opted_out_task.await;
    opted_out_engine.quiesce().await;

    // Default-shaped boot: no credentials, no bindings — only explicit
    // ports (claimed above so the conventional ones can't flake the
    // test). Everything else is exactly what a bare `nimbus start` does:
    // the wire-credential store under the control data dir backs both
    // listeners.
    let command = StartCommand {
        mongodb_port: Some(mongodb_port),
        dynamodb_port: Some(dynamodb_port),
        s3_port: Some(s3_port),
        ..StartCommand::default()
    };
    let enablement =
        super::super::adapters::resolve_adapter_enablement_with_env(&command, temp.path(), |_| {
            None
        })
        .expect("store-backed enablement should resolve");

    let http_listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("http listener should bind");
    let http_addr = http_listener
        .local_addr()
        .expect("http addr should resolve");
    let server_task = tokio::spawn(nimbus_server::serve(
        http_listener,
        adapter_serve_options(&engine, enablement),
    ));
    crate::test_support::wait_for_live_server_health(
        "adapter smoke server should answer /health",
        http_addr,
        &server_task,
    )
    .await;

    // Firestore: the route family is mounted (native error shape, not 404).
    let firestore_response = client
        .get(format!(
            "http://{http_addr}/v1/projects/demo/databases/(default)/documents/notes"
        ))
        .send()
        .await
        .expect("firestore request should send");
    assert_ne!(
        firestore_response.status(),
        reqwest::StatusCode::NOT_FOUND,
        "firestore routes should be mounted by default"
    );

    // MongoDB: the wire-protocol listener accepts TCP connections.
    let mongo_conn = tokio::net::TcpStream::connect(("127.0.0.1", mongodb_port)).await;
    assert!(
        mongo_conn.is_ok(),
        "mongodb listener should accept connections on its configured port"
    );

    // DynamoDB: the target-dispatched endpoint answers in native dialect,
    // and the store-backed default never serves unauthenticated requests.
    let dynamo_response = client
        .post(format!("http://127.0.0.1:{dynamodb_port}/"))
        .header("x-amz-target", "DynamoDB_20120810.ListTables")
        .header("content-type", "application/x-amz-json-1.0")
        .body("{}")
        .send()
        .await
        .expect("dynamodb request should send");
    let status = dynamo_response.status();
    let body = dynamo_response
        .text()
        .await
        .expect("dynamodb body should read");
    assert!(
        status.is_client_error(),
        "unauthenticated dynamodb request should be rejected, got {status}: {body}"
    );
    assert!(
        body.contains("__type") && body.contains("com.amazon"),
        "dynamodb should answer in its native error dialect, got: {body}"
    );

    // S3: the s3s listener is mounted and rejects unsigned requests before
    // any object path can route.
    let s3_response = client
        .get(format!("http://127.0.0.1:{s3_port}/bucket/key"))
        .send()
        .await
        .expect("s3 request should send");
    assert_eq!(
        s3_response.status(),
        reqwest::StatusCode::FORBIDDEN,
        "unsigned S3 request should be rejected"
    );

    server_task.abort();
    let _ = server_task.await;
    engine.quiesce().await;
}
