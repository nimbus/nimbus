use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use hmac::{Hmac, Mac};
use nimbus_engine::Engine;
use nimbus_mongodb::AuthConfig;
use nimbus_testing::{DeterministicTestCase, EngineFixture};
use sha2::Sha256;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::adapters::mongodb::listener::{MongoAuthSource, run_listener};
use nimbus_mongodb::CredentialRegistry;
use nimbus_mongodb::wire::OP_MSG;

type HmacSha256 = Hmac<Sha256>;

const MONGODB_TEST_USER: &str = "wire-user";
const MONGODB_TEST_PASSWORD: &str = "wire-password";

pub(crate) const MONGODB_WIRE_CRUD_ROUNDTRIP_CASE: DeterministicTestCase =
    DeterministicTestCase::new(
        "mongodb-wire-crud-roundtrip",
        "run-to-completion-snapshot",
        "MongoDB wire protocol insert and find roundtrip through OP_MSG framing",
    );

pub(crate) const MONGODB_WIRE_HANDSHAKE_CASE: DeterministicTestCase = DeterministicTestCase::new(
    "mongodb-wire-handshake",
    "run-to-completion-snapshot",
    "MongoDB wire protocol hello command returns required server metadata",
);

async fn send_command(stream: &mut TcpStream, doc: &bson::Document) -> bson::Document {
    let body_bytes = bson::serialize_to_vec(doc).expect("serialize command");
    let flag_bits: u32 = 0;
    let payload_len = 4 + 1 + body_bytes.len();
    let message_length = (16 + payload_len) as i32;
    let request_id: i32 = 1;

    let mut buf = Vec::new();
    buf.extend_from_slice(&message_length.to_le_bytes());
    buf.extend_from_slice(&request_id.to_le_bytes());
    buf.extend_from_slice(&0i32.to_le_bytes());
    buf.extend_from_slice(&OP_MSG.to_le_bytes());
    buf.extend_from_slice(&flag_bits.to_le_bytes());
    buf.push(0);
    buf.extend_from_slice(&body_bytes);

    stream.write_all(&buf).await.expect("write");

    let mut header_buf = [0u8; 16];
    stream
        .read_exact(&mut header_buf)
        .await
        .expect("read header");
    let msg_len = i32::from_le_bytes([header_buf[0], header_buf[1], header_buf[2], header_buf[3]]);
    let body_len = (msg_len as usize) - 16;
    let mut body = vec![0u8; body_len];
    stream.read_exact(&mut body).await.expect("read body");
    bson::deserialize_from_slice(&body[5..]).expect("deserialize")
}

async fn authenticate(stream: &mut TcpStream, username: &str, password: &str) {
    let client_nonce = "clientnonce123";
    let client_first_bare = format!("n={username},r={client_nonce}");
    let client_first = format!("n,,{client_first_bare}");
    let step1 = send_command(
        stream,
        &bson::doc! {
            "saslStart": 1,
            "mechanism": "SCRAM-SHA-256",
            "payload": bson::Binary {
                subtype: bson::spec::BinarySubtype::Generic,
                bytes: client_first.as_bytes().to_vec(),
            },
            "$db": "admin",
        },
    )
    .await;
    assert_eq!(
        step1.get_f64("ok").unwrap(),
        1.0,
        "saslStart failed: {step1:?}"
    );

    let server_first_payload = step1.get_binary_generic("payload").unwrap();
    let server_first = std::str::from_utf8(server_first_payload.as_slice()).unwrap();
    let mut server_nonce = String::new();
    let mut salt_b64 = String::new();
    let mut iterations = 0_u32;
    for part in server_first.split(',') {
        if let Some(value) = part.strip_prefix("r=") {
            server_nonce = value.to_string();
        } else if let Some(value) = part.strip_prefix("s=") {
            salt_b64 = value.to_string();
        } else if let Some(value) = part.strip_prefix("i=") {
            iterations = value.parse().unwrap();
        }
    }

    let salt = BASE64.decode(salt_b64).unwrap();
    let salted_password = derive_salted_password(password, &salt, iterations);
    let client_key = compute_hmac(&salted_password, b"Client Key");
    let stored_key = sha256_hash(&client_key);
    let channel_binding = BASE64.encode(b"n,,");
    let client_final_without_proof = format!("c={channel_binding},r={server_nonce}");
    let auth_message = format!("{client_first_bare},{server_first},{client_final_without_proof}");
    let client_signature = compute_hmac(&stored_key, auth_message.as_bytes());
    let mut proof = client_key;
    for (i, byte) in client_signature.iter().enumerate() {
        proof[i] ^= byte;
    }
    let client_final = format!("{client_final_without_proof},p={}", BASE64.encode(proof));

    let step2 = send_command(
        stream,
        &bson::doc! {
            "saslContinue": 1,
            "conversationId": step1.get_i32("conversationId").unwrap(),
            "payload": bson::Binary {
                subtype: bson::spec::BinarySubtype::Generic,
                bytes: client_final.as_bytes().to_vec(),
            },
            "$db": "admin",
        },
    )
    .await;
    assert_eq!(
        step2.get_f64("ok").unwrap(),
        1.0,
        "saslContinue failed: {step2:?}"
    );
    assert!(step2.get_bool("done").unwrap());
}

fn derive_salted_password(password: &str, salt: &[u8], iterations: u32) -> Vec<u8> {
    let mut salted = vec![0_u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, &mut salted);
    salted
}

fn compute_hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn sha256_hash(data: &[u8]) -> Vec<u8> {
    use sha2::Digest;
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

pub(crate) async fn mongodb_wire_crud_roundtrip_inner() {
    let fixture = EngineFixture::new(|path| Engine::new_with_memory_persistence(path));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let service = fixture.engine();
    tokio::spawn(run_listener(
        listener,
        service,
        MongoAuthSource::Unbound(Arc::new(AuthConfig::new(
            MONGODB_TEST_USER.into(),
            MONGODB_TEST_PASSWORD.into(),
        ))),
    ));

    let mut stream = TcpStream::connect(addr).await.expect("connect");

    let unauthenticated = send_command(
        &mut stream,
        &bson::doc! {
            "insert": "test_col",
            "$db": "testdb",
            "documents": [{ "_id": "blocked" }],
        },
    )
    .await;
    assert_eq!(unauthenticated.get_f64("ok").unwrap(), 0.0);
    assert_eq!(unauthenticated.get_str("codeName").unwrap(), "Unauthorized");

    authenticate(&mut stream, MONGODB_TEST_USER, MONGODB_TEST_PASSWORD).await;

    let resp = send_command(
        &mut stream,
        &bson::doc! {
            "insert": "test_col",
            "$db": "testdb",
            "documents": [{ "_id": "d1", "name": "Alice", "age": 30 }],
        },
    )
    .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0, "insert failed: {resp:?}");

    let resp = send_command(
        &mut stream,
        &bson::doc! {
            "find": "test_col",
            "$db": "testdb",
            "filter": { "_id": "d1" },
        },
    )
    .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0, "find failed: {resp:?}");
    let cursor = resp.get_document("cursor").unwrap();
    let batch = cursor.get_array("firstBatch").unwrap();
    assert_eq!(batch.len(), 1);
    let doc = batch[0].as_document().unwrap();
    assert_eq!(doc.get_str("name").unwrap(), "Alice");
    assert_eq!(doc.get_i32("age").unwrap(), 30);
}

pub(crate) async fn mongodb_wire_handshake_inner() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let service = fixture.engine();
    tokio::spawn(run_listener(
        listener,
        service,
        MongoAuthSource::Unbound(Arc::new(AuthConfig::new(
            MONGODB_TEST_USER.into(),
            MONGODB_TEST_PASSWORD.into(),
        ))),
    ));

    let mut stream = TcpStream::connect(addr).await.expect("connect");

    let resp = send_command(
        &mut stream,
        &bson::doc! { "hello": 1, "helloOk": true, "$db": "admin" },
    )
    .await;
    assert_eq!(resp.get_f64("ok").unwrap(), 1.0);
    assert!(resp.get_bool("isWritablePrimary").unwrap());
    assert!(resp.get_bool("helloOk").unwrap());
    assert!(resp.get_i32("maxBsonObjectSize").is_ok());
    assert!(resp.get_i32("maxWireVersion").is_ok());
    assert!(resp.get_i64("connectionId").is_ok());
}

/// Acceptance bar (M9a): cross-tenant rejection THROUGH the ingested-and-served
/// path. The registry is built by the SAME parser the operator path uses
/// (`CredentialRegistry::from_operator_spec` on a `NIMBUS_MONGODB_CREDENTIALS`-
/// format spec), served the SAME way `nimbus-server` serves it (`run_listener`
/// over a real loopback TCP socket dispatching through `dispatch_authed`), and a
/// real SCRAM handshake as `user-a` (bound to `tenant-a`) is performed before a
/// cross-tenant `find` is refused and the same-tenant `find` is allowed.
#[tokio::test]
async fn mongodb_wire_bound_credential_cross_tenant_refused_through_served_path() {
    // 1. Ingest exactly as the operator path does: parse the env-format spec.
    let registry =
        CredentialRegistry::from_operator_spec("user-a:tenant-a:secret-a,user-b:tenant-b:secret-b")
            .expect("operator credential spec must parse");

    // 2. Serve it exactly as nimbus-server does: a bound listener over loopback.
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    tokio::spawn(run_listener(
        listener,
        fixture.engine(),
        MongoAuthSource::Bound(Arc::new(registry)),
    ));

    let mut stream = TcpStream::connect(addr).await.expect("connect");

    // 3. Real SCRAM handshake as user-a; authentication binds tenant-a.
    authenticate(&mut stream, "user-a", "secret-a").await;

    // Seed tenant-a (same-tenant write is allowed and ensures the tenant exists).
    let seeded = send_command(
        &mut stream,
        &bson::doc! {
            "insert": "users",
            "$db": "tenant-a",
            "documents": [{ "_id": "a1", "name": "alice" }],
        },
    )
    .await;
    assert_eq!(
        seeded.get_f64("ok").unwrap(),
        1.0,
        "same-tenant insert must be allowed: {seeded:?}"
    );

    // Cross-tenant find ($db = tenant-b) is REFUSED through the served path.
    let refused = send_command(
        &mut stream,
        &bson::doc! { "find": "users", "$db": "tenant-b", "filter": {} },
    )
    .await;
    let refused_code = refused.get_str("codeName").unwrap();
    println!(
        "REFUSED  cross-tenant find $db=tenant-b -> ok={} codeName={} message={:?}",
        refused.get_f64("ok").unwrap(),
        refused_code,
        refused.get_str("errmsg").unwrap_or("<none>"),
    );
    assert_eq!(
        refused.get_f64("ok").unwrap(),
        0.0,
        "cross-tenant find must be refused"
    );
    assert_eq!(
        refused_code, "Unauthorized",
        "cross-tenant refusal must be Unauthorized"
    );

    // Same-tenant find ($db = tenant-a) is ALLOWED and sees the seeded document.
    let allowed = send_command(
        &mut stream,
        &bson::doc! { "find": "users", "$db": "tenant-a", "filter": {} },
    )
    .await;
    let allowed_batch = allowed
        .get_document("cursor")
        .unwrap()
        .get_array("firstBatch")
        .unwrap()
        .len();
    println!(
        "ALLOWED  same-tenant find  $db=tenant-a -> ok={} firstBatch.len={}",
        allowed.get_f64("ok").unwrap(),
        allowed_batch,
    );
    assert_eq!(
        allowed.get_f64("ok").unwrap(),
        1.0,
        "same-tenant find must be allowed: {allowed:?}"
    );
    assert_eq!(
        allowed_batch, 1,
        "same-tenant find should see the seeded document"
    );
}

#[tokio::test]
async fn mongodb_tenant_admission_uses_provider_lifecycle() {
    mongodb_wire_crud_roundtrip_inner().await;
}

#[tokio::test]
async fn mongodb_wire_handshake() {
    mongodb_wire_handshake_inner().await;
}
