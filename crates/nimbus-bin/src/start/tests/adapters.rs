//! Integration smokes for the D7 adapter posture: a server booted from
//! CLI-resolved configs serves every surface by default — store-backed
//! credentials and all — and the `--no-*` opt-outs really disable them.

use std::sync::Arc;

use super::*;

fn adapter_serve_options(
    engine: &Arc<nimbus::Engine>,
    enablement: super::super::adapters::AdapterEnablement,
) -> nimbus_server::ServeOptions {
    enablement.apply_to(nimbus_server::ServeOptions::new(engine.clone()))
}

#[tokio::test]
async fn cli_adapters_serve_store_backed_by_default_and_opt_outs_disable() {
    let temp = tempfile::tempdir().expect("tempdir should build");
    let engine =
        Arc::new(nimbus::Engine::new(temp.path().join("engine")).expect("engine should build"));

    // Reserve listener ports for the sibling adapter listeners.
    let reserve = |_| async {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("port reservation should bind");
        let port = listener.local_addr().expect("addr should resolve").port();
        drop(listener);
        port
    };
    let mongodb_port = reserve(()).await;
    let dynamodb_port = reserve(()).await;

    // Opt-outs first: a fully opted-out server mounts none of the
    // surfaces. (Runs before the serving server because sibling adapter
    // listeners spawned by `serve` outlive an aborted parent task.)
    let client = reqwest::Client::new();
    let opted_out_command = StartCommand {
        firestore: false,
        mongodb: false,
        dynamodb: false,
        ..StartCommand::default()
    };
    let opted_out_enablement = super::super::adapters::resolve_adapter_enablement_with_env(
        &opted_out_command,
        temp.path(),
        |_| None,
        |_| true,
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
        adapter_serve_options(&engine, opted_out_enablement),
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
    opted_out_task.abort();
    let _ = opted_out_task.await;

    // Default-shaped boot: no credentials, no bindings — only explicit
    // ports (reserved above so the conventional ones can't flake the
    // test). Everything else is exactly what a bare `nimbus start` does:
    // the wire-credential store under the control data dir backs both
    // listeners.
    let command = StartCommand {
        mongodb_port: Some(mongodb_port),
        dynamodb_port: Some(dynamodb_port),
        ..StartCommand::default()
    };
    let enablement = super::super::adapters::resolve_adapter_enablement_with_env(
        &command,
        temp.path(),
        |_| None,
        |_| true,
    )
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

    server_task.abort();
    let _ = server_task.await;
    engine.quiesce().await;
}
