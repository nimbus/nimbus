//! LR6 integration smokes: a server booted from CLI-resolved adapter
//! configs answers natively on every enabled surface — and the surfaces
//! stay off without the flags.

use std::sync::Arc;

use super::*;

fn adapter_serve_options(
    engine: &Arc<nimbus::Engine>,
    enablement: super::super::adapters::AdapterEnablement,
) -> nimbus_server::ServeOptions {
    enablement.apply_to(nimbus_server::ServeOptions::new(engine.clone()))
}

#[tokio::test]
async fn cli_enabled_adapters_answer_natively_and_stay_off_by_default() {
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

    // Default-off first: a server without the flags mounts none of the
    // surfaces. (Runs before the enabled server because sibling adapter
    // listeners spawned by `serve` outlive an aborted parent task.)
    let client = reqwest::Client::new();
    let default_enablement = super::super::adapters::resolve_adapter_enablement_with_env(
        &StartCommand::default(),
        |_| None,
    )
    .expect("default enablement should resolve");
    let default_listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("default listener should bind");
    let default_addr = default_listener
        .local_addr()
        .expect("default addr should resolve");
    let default_task = tokio::spawn(nimbus_server::serve(
        default_listener,
        adapter_serve_options(&engine, default_enablement),
    ));
    crate::test_support::wait_for_live_server_health(
        "default smoke server should answer /health",
        default_addr,
        &default_task,
    )
    .await;
    let firestore_off = client
        .get(format!(
            "http://{default_addr}/v1/projects/demo/databases/(default)/documents/notes"
        ))
        .send()
        .await
        .expect("firestore-off request should send");
    assert_eq!(
        firestore_off.status(),
        reqwest::StatusCode::NOT_FOUND,
        "firestore routes must stay unmounted without --firestore"
    );
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", mongodb_port))
            .await
            .is_err(),
        "mongodb listener must stay off without --mongodb-port"
    );
    default_task.abort();
    let _ = default_task.await;

    let mut command = StartCommand {
        firestore: true,
        mongodb_port: Some(mongodb_port),
        mongodb_username: Some("ops".to_string()),
        dynamodb_port: Some(dynamodb_port),
        ..StartCommand::default()
    };
    command.dynamodb_access_key = vec!["AKIDEXAMPLE:secret:demo".to_string()];

    // Resolve through the same code path `nimbus start` uses, with the
    // env-only MongoDB password injected deterministically.
    let enablement =
        super::super::adapters::resolve_adapter_enablement_with_env(&command, |name| match name {
            "NIMBUS_MONGODB_PASSWORD" => Some("scram-secret".to_string()),
            _ => None,
        })
        .expect("adapter enablement should resolve");

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
        "firestore routes should be mounted when --firestore is set"
    );

    // MongoDB: the wire-protocol listener accepts TCP connections.
    let mongo_conn = tokio::net::TcpStream::connect(("127.0.0.1", mongodb_port)).await;
    assert!(
        mongo_conn.is_ok(),
        "mongodb listener should accept connections on the CLI-configured port"
    );

    // DynamoDB: the target-dispatched endpoint answers in native dialect.
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
