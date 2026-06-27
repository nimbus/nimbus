use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr};

use nimbus_core::TenantId;
use nimbus_kv::{CredentialRegistry, KvError, NimbusKvConfig, run_listener, serve};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

fn tenant(id: &str) -> TenantId {
    TenantId::new(id).expect("valid tenant id")
}

async fn spawn_test_server(credentials: CredentialRegistry) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener binds");
    let addr = listener.local_addr().expect("listener has addr");
    let config = NimbusKvConfig::new(addr, credentials);
    let handle = tokio::spawn(async move {
        serve(listener, config).await.expect("server should run");
    });
    (addr, handle)
}

fn resp_command(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = format!("*{}\r\n", parts.len()).into_bytes();
    for part in parts {
        out.extend_from_slice(format!("${}\r\n", part.len()).as_bytes());
        out.extend_from_slice(part);
        out.extend_from_slice(b"\r\n");
    }
    out
}

async fn read_response(stream: &mut TcpStream) -> String {
    let mut buf = vec![0_u8; 2048];
    let read = timeout(Duration::from_secs(2), stream.read(&mut buf))
        .await
        .expect("read should not time out")
        .expect("read should succeed");
    String::from_utf8_lossy(&buf[..read]).into_owned()
}

async fn write_command(stream: &mut TcpStream, parts: &[&[u8]]) -> String {
    stream
        .write_all(&resp_command(parts))
        .await
        .expect("command write should succeed");
    read_response(stream).await
}

#[tokio::test(flavor = "multi_thread")]
async fn redis_rs_client_connects_and_ping_echo_round_trip() {
    let password = "secret";
    let credentials = CredentialRegistry::single_dev(tenant("tenant-a"), password);
    let (addr, server) = spawn_test_server(credentials).await;

    let url = format!("redis://:{password}@{addr}/");
    let result = tokio::task::spawn_blocking(move || {
        let client = redis::Client::open(url).expect("redis client should parse URL");
        let mut connection = client.get_connection().expect("redis-rs client connects");
        let pong: String = redis::cmd("PING")
            .query(&mut connection)
            .expect("PING should round-trip");
        let echo: String = redis::cmd("ECHO")
            .arg("hello")
            .query(&mut connection)
            .expect("ECHO should round-trip");
        (pong, echo)
    })
    .await
    .expect("blocking redis client task joins");

    assert_eq!(result, ("PONG".to_owned(), "hello".to_owned()));
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn hello_3_negotiates_resp3() {
    let credentials = CredentialRegistry::single_dev(tenant("tenant-a"), "secret");
    let (addr, server) = spawn_test_server(credentials).await;
    let mut stream = TcpStream::connect(addr).await.expect("connects");

    let auth = write_command(&mut stream, &[b"AUTH", b"secret"]).await;
    assert_eq!(auth, "+OK\r\n");

    let hello = write_command(&mut stream, &[b"HELLO", b"3"]).await;
    assert!(
        hello.starts_with('%'),
        "HELLO 3 should return a RESP3 map, got {hello:?}"
    );
    assert!(hello.contains("proto"));

    let ping = write_command(&mut stream, &[b"PING"]).await;
    assert_eq!(ping, "+PONG\r\n");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn listener_rejects_non_loopback_bind() {
    let config = NimbusKvConfig::new(
        "0.0.0.0:0".parse().expect("addr parses"),
        CredentialRegistry::single_dev(tenant("tenant-a"), "secret"),
    );

    let error = run_listener(config)
        .await
        .expect_err("non-loopback bind should fail closed");
    match error {
        KvError::Io(error) => assert_eq!(error.kind(), ErrorKind::InvalidInput),
        other => panic!("expected InvalidInput io error, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn unauthenticated_command_is_rejected() {
    let credentials = CredentialRegistry::single_dev(tenant("tenant-a"), "secret");
    let (addr, server) = spawn_test_server(credentials).await;
    let mut stream = TcpStream::connect(addr).await.expect("connects");

    let response = write_command(&mut stream, &[b"PING"]).await;
    assert_eq!(response, "-NOAUTH Authentication required\r\n");
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn tenant_a_credential_cannot_read_tenant_b_keys() {
    let credentials = CredentialRegistry::new()
        .bind("tenant-a", "secret-a", tenant("tenant-a"))
        .bind("tenant-b", "secret-b", tenant("tenant-b"));
    let (addr, server) = spawn_test_server(credentials).await;
    let mut stream = TcpStream::connect(addr).await.expect("connects");

    let auth = write_command(&mut stream, &[b"AUTH", b"tenant-a", b"secret-a"]).await;
    assert_eq!(auth, "+OK\r\n");

    let cross_tenant = write_command(&mut stream, &[b"SELECT", b"tenant-b"]).await;
    assert!(
        cross_tenant.contains("cannot change tenant"),
        "credential bound to tenant A must not select tenant B, got {cross_tenant:?}"
    );
    server.abort();
}

#[tokio::test(flavor = "multi_thread")]
async fn resp_get_set_del_expire_ttl_incr_round_trip() {
    let password = "secret";
    let credentials = CredentialRegistry::single_dev(tenant("tenant-a"), password);
    let (addr, server) = spawn_test_server(credentials).await;

    let url = format!("redis://:{password}@{addr}/");
    let result = tokio::task::spawn_blocking(move || {
        let client = redis::Client::open(url).expect("redis client should parse URL");
        let mut connection = client.get_connection().expect("redis-rs client connects");
        let set: String = redis::cmd("SET")
            .arg("counter")
            .arg("41")
            .query(&mut connection)
            .expect("SET should succeed");
        let get: String = redis::cmd("GET")
            .arg("counter")
            .query(&mut connection)
            .expect("GET should return value");
        let incr: i64 = redis::cmd("INCR")
            .arg("counter")
            .query(&mut connection)
            .expect("INCR should succeed");
        let expire: i64 = redis::cmd("EXPIRE")
            .arg("counter")
            .arg(60)
            .query(&mut connection)
            .expect("EXPIRE should succeed");
        let ttl: i64 = redis::cmd("TTL")
            .arg("counter")
            .query(&mut connection)
            .expect("TTL should succeed");
        let del: i64 = redis::cmd("DEL")
            .arg("counter")
            .query(&mut connection)
            .expect("DEL should succeed");
        let reset_set: String = redis::cmd("SET")
            .arg("reset")
            .arg("value")
            .query(&mut connection)
            .expect("SET before FLUSHALL should succeed");
        let function_flush: String = redis::cmd("FUNCTION")
            .arg("FLUSH")
            .query(&mut connection)
            .expect("FUNCTION FLUSH should acknowledge empty function registry");
        let flushall: String = redis::cmd("FLUSHALL")
            .query(&mut connection)
            .expect("FLUSHALL should clear the tenant keyspace");
        let reset_after_flush: Option<String> = redis::cmd("GET")
            .arg("reset")
            .query(&mut connection)
            .expect("GET after FLUSHALL should succeed");
        let readiness: String = redis::cmd("NIMBUS.READY")
            .query(&mut connection)
            .expect("NIMBUS.READY should be distinct from PING");
        let metrics: String = redis::cmd("NIMBUS.METRICS")
            .query(&mut connection)
            .expect("NIMBUS.METRICS should return operator diagnostics");
        (
            set,
            get,
            incr,
            expire,
            ttl,
            del,
            reset_set,
            function_flush,
            flushall,
            reset_after_flush,
            readiness,
            metrics,
        )
    })
    .await
    .expect("blocking redis client task joins");

    assert_eq!(result.0, "OK");
    assert_eq!(result.1, "41");
    assert_eq!(result.2, 42);
    assert_eq!(result.3, 1);
    assert!(result.4 > 0, "TTL should be positive, got {}", result.4);
    assert_eq!(result.5, 1);
    assert_eq!(result.6, "OK");
    assert_eq!(result.7, "OK");
    assert_eq!(result.8, "OK");
    assert_eq!(result.9, None);
    assert_eq!(result.10, "READY");
    assert!(result.11.contains("readiness:ready"));
    assert!(result.11.contains("connected_clients:"));
    assert!(result.11.contains("cache_hits:"));
    assert!(result.11.contains("cache_misses:"));
    assert!(result.11.contains("cache_hit_ratio_ppm:"));
    assert!(result.11.contains("durable_writes_in_flight:"));
    assert!(result.11.contains("durable_write_latency_us_total:"));
    assert!(result.11.contains("command.SET.calls:"));
    assert!(result.11.contains("command.GET.latency_us_total:"));
    server.abort();
}
