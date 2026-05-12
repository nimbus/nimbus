use nimbus_engine::Service;
use nimbus_testing::{DeterministicTestCase, ServiceFixture};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::adapters::mongodb::listener::run_listener;
use crate::adapters::mongodb::wire::OP_MSG;

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

pub(crate) async fn mongodb_wire_crud_roundtrip_inner() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let service = fixture.service();
    tokio::spawn(run_listener(listener, service));

    let mut stream = TcpStream::connect(addr).await.expect("connect");

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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local addr");
    let service = fixture.service();
    tokio::spawn(run_listener(listener, service));

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

#[tokio::test]
async fn mongodb_wire_crud_roundtrip() {
    mongodb_wire_crud_roundtrip_inner().await;
}

#[tokio::test]
async fn mongodb_wire_handshake() {
    mongodb_wire_handshake_inner().await;
}
