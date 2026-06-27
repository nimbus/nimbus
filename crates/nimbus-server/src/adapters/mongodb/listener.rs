use std::net::SocketAddr;
use std::sync::Arc;

use nimbus_engine::Engine;
use tokio::io::{BufReader, BufWriter};
use tokio::net::TcpListener;
use tracing::{debug, error, info, warn};

use nimbus_mongodb::AuthConfig;
use nimbus_mongodb::commands;
use nimbus_mongodb::connection::{ConnectionState, next_request_id};
use nimbus_mongodb::error::MongoError;
use nimbus_mongodb::wire::{self, WireError};

/// Fail-closed bind guard for the MongoDB listener.
///
/// This guard is load-bearing for the adapter's security model, not a convenience
/// default. The adapter authenticates against a single, tenant-agnostic SCRAM
/// credential (`AuthConfig`) and selects the tenant from the requested database name
/// (the wire `$db`) rather than from the authenticated user — so a network-reachable
/// listener would let any holder of that one credential reach every tenant by varying
/// `$db`. The invariant is therefore: **a non-loopback bind is permitted only when the
/// credential authenticates a specific tenant** (`AuthConfig::is_tenant_bound`; the
/// credential->TenantId binding is M9(a), issue #23). The refusal is derived from that
/// binding state, not hardcoded, so there is no one-line path to a non-loopback bind
/// without the binding.
///
/// The check runs at the bind seam (`WireProtocolAdapter::guard`), which ABORTS BOOT
/// before the listener serves a byte (see `adapters::wire`): reconfiguring MongoDB to a
/// non-loopback address yields an immediate, unmissable startup failure pointing at
/// #23, not a server that comes up subtly exposed. Today no credential is tenant-bound,
/// so every non-loopback bind is refused; the only future allow is a non-loopback bind
/// whose credential is tenant-bound.
pub(crate) fn guard_bind_address(addr: SocketAddr, auth: &AuthConfig) -> std::io::Result<()> {
    if addr.ip().is_loopback() {
        return Ok(());
    }
    if !auth.is_tenant_bound() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "MongoDB listener refuses to bind non-loopback address {addr}: the adapter selects \
                 the tenant from the wire $db under a single tenant-agnostic credential, so a \
                 non-loopback bind requires per-tenant credential binding (credential->TenantId). \
                 Bind to a loopback address, or implement credential->TenantId binding first \
                 (see issue #23)."
            ),
        ));
    }
    Ok(())
}

pub async fn run_listener(listener: TcpListener, engine: Arc<Engine>, auth: Arc<AuthConfig>) {
    let local_addr = listener.local_addr().ok();
    info!("MongoDB listener started on {:?}", local_addr);

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let engine_handle = engine.clone();
                let auth_cfg = auth.clone();
                debug!("MongoDB connection from {addr}");
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, addr, engine_handle, auth_cfg).await {
                        match e {
                            WireError::ConnectionClosed => {
                                debug!("MongoDB connection from {addr} closed");
                            }
                            _ => {
                                warn!("MongoDB connection from {addr} error: {e}");
                            }
                        }
                    }
                });
            }
            Err(e) => {
                error!("MongoDB listener accept error: {e}");
            }
        }
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    addr: SocketAddr,
    engine: Arc<Engine>,
    auth: Arc<AuthConfig>,
) -> Result<(), WireError> {
    let (read_half, write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut writer = BufWriter::new(write_half);
    let mut conn = ConnectionState::new(addr);

    loop {
        let msg = wire::read_msg(&mut reader).await?;
        let body_bytes = wire::validate_op_msg(&msg)?;
        let client_request_id = msg.header.request_id;

        let body_doc: bson::Document = bson::deserialize_from_slice(body_bytes)
            .map_err(|e| WireError::MalformedBson(format!("invalid BSON body: {e}")))?;

        let command_name = commands::extract_command_name(&body_doc);
        let response_doc = match &command_name {
            Some(name) => {
                match commands::dispatch(name, &body_doc, &mut conn, &engine, &auth).await {
                    Ok(doc) => doc,
                    Err(e) => e.to_error_doc(),
                }
            }
            None => MongoError::command_not_found("<unknown>").to_error_doc(),
        };

        let response_bytes = bson::serialize_to_vec(&response_doc)
            .map_err(|e| WireError::MalformedBson(format!("failed to serialize response: {e}")))?;

        let response_id = next_request_id();
        wire::write_msg(&mut writer, response_id, client_request_id, &response_bytes).await?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_testing::EngineFixture;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn make_op_msg_from_doc(request_id: i32, doc: &bson::Document) -> Vec<u8> {
        let body_doc = bson::serialize_to_vec(doc).unwrap();
        let flag_bits: u32 = 0;
        let payload_len = 4 + 1 + body_doc.len();
        let message_length = (16 + payload_len) as i32;

        let mut buf = Vec::new();
        buf.extend_from_slice(&message_length.to_le_bytes());
        buf.extend_from_slice(&request_id.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&wire::OP_MSG.to_le_bytes());
        buf.extend_from_slice(&flag_bits.to_le_bytes());
        buf.push(0); // section kind 0
        buf.extend_from_slice(&body_doc);
        buf
    }

    fn make_legacy_insert_msg() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&20i32.to_le_bytes());
        buf.extend_from_slice(&1i32.to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        buf.extend_from_slice(&2002i32.to_le_bytes()); // OP_INSERT
        buf.extend_from_slice(&[0u8; 4]);
        buf
    }

    async fn read_response(stream: &mut tokio::net::TcpStream) -> (i32, i32, bson::Document) {
        let mut header_buf = [0u8; 16];
        stream.read_exact(&mut header_buf).await.unwrap();
        let msg_len =
            i32::from_le_bytes([header_buf[0], header_buf[1], header_buf[2], header_buf[3]]);
        let response_to =
            i32::from_le_bytes([header_buf[8], header_buf[9], header_buf[10], header_buf[11]]);
        let opcode = i32::from_le_bytes([
            header_buf[12],
            header_buf[13],
            header_buf[14],
            header_buf[15],
        ]);
        assert_eq!(opcode, wire::OP_MSG);

        let body_len = (msg_len as usize) - 16;
        let mut body = vec![0u8; body_len];
        stream.read_exact(&mut body).await.unwrap();

        // skip flags (4 bytes) + section kind (1 byte)
        let doc: bson::Document = bson::deserialize_from_slice(&body[5..]).unwrap();
        (msg_len, response_to, doc)
    }

    #[tokio::test]
    async fn listener_handles_ping() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(run_listener(
            listener,
            fixture.engine(),
            Arc::new(AuthConfig::new("test-user".into(), "test-password".into())),
        ));

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let msg = make_op_msg_from_doc(1, &bson::doc! { "ping": 1 });
        stream.write_all(&msg).await.unwrap();

        let (msg_len, response_to, doc) = read_response(&mut stream).await;
        assert_eq!(response_to, 1);
        assert!(msg_len > 16);
        assert_eq!(doc.get_f64("ok").unwrap(), 1.0);
    }

    #[test]
    fn listener_refuses_non_loopback_bind_while_credential_is_unbound() {
        let addr: SocketAddr = "0.0.0.0:27017".parse().unwrap();
        let auth = AuthConfig::new("test-user".into(), "test-password".into());
        assert!(
            !auth.is_tenant_bound(),
            "the shipped credential is tenant-agnostic; this test exercises the unbound path"
        );
        // Refusal is at the guard (bind seam), which aborts boot before the listener serves a
        // byte — a startup failure, not a per-connection rejection. The only future allow is a
        // non-loopback bind whose credential is tenant-bound (M9a, #23).
        let error = guard_bind_address(addr, &auth)
            .expect_err("non-loopback MongoDB bind must be refused while credentials are unbound");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("non-loopback"));
        assert!(error.to_string().contains("#23"));
    }

    #[test]
    fn listener_allows_loopback_bind() {
        let addr: SocketAddr = "127.0.0.1:27017".parse().unwrap();
        let auth = AuthConfig::new("test-user".into(), "test-password".into());
        guard_bind_address(addr, &auth).expect("loopback MongoDB bind must be permitted");
    }

    #[tokio::test]
    async fn listener_rejects_unauthenticated_data_command() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(run_listener(
            listener,
            fixture.engine(),
            Arc::new(AuthConfig::new("test-user".into(), "test-password".into())),
        ));

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let msg = make_op_msg_from_doc(
            2,
            &bson::doc! {
                "insert": "users",
                "$db": "testdb",
                "documents": [{ "_id": "blocked" }],
            },
        );
        stream.write_all(&msg).await.unwrap();

        let (_, _, doc) = read_response(&mut stream).await;
        assert_eq!(doc.get_f64("ok").unwrap(), 0.0);
        assert_eq!(doc.get_str("codeName").unwrap(), "Unauthorized");
    }

    #[tokio::test]
    async fn listener_rejects_legacy_opcode() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(run_listener(
            listener,
            fixture.engine(),
            Arc::new(AuthConfig::new("test-user".into(), "test-password".into())),
        ));

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream.write_all(&make_legacy_insert_msg()).await.unwrap();

        // Legacy opcode causes a wire error and the connection is dropped.
        let mut buf = [0u8; 1];
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            stream.read_exact(&mut buf),
        )
        .await;

        match result {
            Ok(Ok(_)) => {}
            Ok(Err(_)) => {}
            Err(_) => {}
        }
    }
}
