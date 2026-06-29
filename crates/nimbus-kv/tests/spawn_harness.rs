use std::env;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const TENANT: &str = "tenant-a";
const PASSWORD: &str = "secret";

#[test]
#[ignore = "requires REDISRS_SERVER_BIN or NIMBUS_KV_SERVER_BIN pointing at a built nimbus binary"]
fn redis_rs_spawned_nimbus_kv_binary_smoke_resp2_and_resp3() {
    let addr = free_loopback_addr();
    let mut server = SpawnedServer::start(addr);
    server.wait_ready();

    let url = format!("redis://:{PASSWORD}@{addr}/");
    let client = redis::Client::open(url).expect("redis-rs URL parses");
    let mut connection = client
        .get_connection()
        .expect("redis-rs connects to spawned nimbus-kv");

    let pong: String = redis::cmd("PING")
        .query(&mut connection)
        .expect("PING should round-trip");
    assert_eq!(pong, "PONG");

    let flush: String = redis::cmd("FLUSHALL")
        .query(&mut connection)
        .expect("FLUSHALL should clear the smoke namespace");
    assert_eq!(flush, "OK");

    let set: String = redis::cmd("SET")
        .arg("spawn:counter")
        .arg("41")
        .query(&mut connection)
        .expect("SET should succeed through redis-rs");
    assert_eq!(set, "OK");

    let get: String = redis::cmd("GET")
        .arg("spawn:counter")
        .query(&mut connection)
        .expect("GET should return the committed value");
    assert_eq!(get, "41");

    let incr: i64 = redis::cmd("INCR")
        .arg("spawn:counter")
        .query(&mut connection)
        .expect("INCR should route through the transactional tier");
    assert_eq!(incr, 42);

    let expire: i64 = redis::cmd("EXPIRE")
        .arg("spawn:counter")
        .arg(60)
        .query(&mut connection)
        .expect("EXPIRE should update durable expiry");
    assert_eq!(expire, 1);

    let ttl: i64 = redis::cmd("TTL")
        .arg("spawn:counter")
        .query(&mut connection)
        .expect("TTL should expose the durable expiry");
    assert!(ttl > 0, "TTL should be positive, got {ttl}");

    let del: i64 = redis::cmd("DEL")
        .arg("spawn:counter")
        .query(&mut connection)
        .expect("DEL should delete the key");
    assert_eq!(del, 1);

    resp3_smoke(addr);
}

struct SpawnedServer {
    child: Child,
    addr: SocketAddr,
}

impl SpawnedServer {
    fn start(addr: SocketAddr) -> Self {
        let bin = server_bin();
        let child = Command::new(&bin)
            .arg("kv")
            .arg("--bind")
            .arg(addr.to_string())
            .arg("--tenant")
            .arg(TENANT)
            .arg("--username")
            .arg(TENANT)
            .arg("--password")
            .arg(PASSWORD)
            .arg("--no-disk")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|error| panic!("failed to spawn {}: {error}", bin.display()));
        Self { child, addr }
    }

    fn wait_ready(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(15);
        let url = format!("redis://:{PASSWORD}@{}/", self.addr);
        let mut last_error = None;

        while Instant::now() < deadline {
            match self.child.try_wait() {
                Ok(Some(status)) => panic!("nimbus-kv exited before readiness: {status}"),
                Ok(None) => {}
                Err(error) => panic!("failed to poll nimbus-kv child: {error}"),
            }

            match redis::Client::open(url.as_str())
                .and_then(|client| client.get_connection())
                .and_then(|mut connection| redis::cmd("PING").query::<String>(&mut connection))
            {
                Ok(pong) if pong == "PONG" => return,
                Ok(other) => last_error = Some(format!("unexpected PING response {other:?}")),
                Err(error) => last_error = Some(error.to_string()),
            }
            thread::sleep(Duration::from_millis(100));
        }

        panic!(
            "nimbus-kv did not become ready at {}: {}",
            self.addr,
            last_error.unwrap_or_else(|| "no connection attempt was made".to_owned())
        );
    }
}

impl Drop for SpawnedServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn server_bin() -> PathBuf {
    let configured = env::var_os("REDISRS_SERVER_BIN")
        .or_else(|| env::var_os("NIMBUS_KV_SERVER_BIN"))
        .map(PathBuf::from)
        .expect("set REDISRS_SERVER_BIN or NIMBUS_KV_SERVER_BIN to the built nimbus binary");
    if configured.is_absolute() || configured.exists() {
        return configured;
    }

    let repo_relative = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(&configured);
    if repo_relative.exists() {
        return repo_relative;
    }

    configured
}

fn free_loopback_addr() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral loopback port");
    listener.local_addr().expect("listener has local address")
}

fn resp3_smoke(addr: SocketAddr) {
    let mut stream = TcpStream::connect(addr).expect("RESP3 TCP connection should open");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("set read timeout");

    let hello = send_resp_command(&mut stream, &["HELLO", "3", "AUTH", TENANT, PASSWORD]);
    assert!(
        hello.starts_with('%') && hello.contains("proto"),
        "HELLO 3 should negotiate RESP3, got {hello:?}"
    );

    assert_eq!(send_resp_command(&mut stream, &["FLUSHALL"]), "+OK\r\n");
    assert_eq!(
        send_resp_command(&mut stream, &["SET", "spawn:resp3", "41"]),
        "+OK\r\n"
    );

    let value = send_resp_command(&mut stream, &["GET", "spawn:resp3"]);
    assert_eq!(value, "$2\r\n41\r\n");

    let incr = send_resp_command(&mut stream, &["INCR", "spawn:resp3"]);
    assert_eq!(incr, ":42\r\n");

    let expire = send_resp_command(&mut stream, &["EXPIRE", "spawn:resp3", "60"]);
    assert_eq!(expire, ":1\r\n");

    let ttl = send_resp_command(&mut stream, &["TTL", "spawn:resp3"]);
    assert!(
        ttl.starts_with(':') && !ttl.starts_with(":-"),
        "TTL should be positive, got {ttl:?}"
    );

    let del = send_resp_command(&mut stream, &["DEL", "spawn:resp3"]);
    assert_eq!(del, ":1\r\n");
}

fn send_resp_command(stream: &mut TcpStream, parts: &[&str]) -> String {
    let mut command = format!("*{}\r\n", parts.len()).into_bytes();
    for part in parts {
        command.extend_from_slice(format!("${}\r\n", part.len()).as_bytes());
        command.extend_from_slice(part.as_bytes());
        command.extend_from_slice(b"\r\n");
    }
    stream.write_all(&command).expect("write RESP command");

    let mut response = vec![0_u8; 4096];
    let read = stream.read(&mut response).expect("read RESP response");
    String::from_utf8(response[..read].to_vec()).expect("server responses are UTF-8 in smoke")
}
