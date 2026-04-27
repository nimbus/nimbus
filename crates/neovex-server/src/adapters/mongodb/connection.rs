use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, Ordering};

use super::commands::change_stream::ChangeStreamStore;
use super::commands::cursor::CursorStore;
use super::commands::session::SessionStore;

static NEXT_REQUEST_ID: AtomicI64 = AtomicI64::new(1);
static NEXT_CONNECTION_ID: AtomicI64 = AtomicI64::new(1);

pub fn next_request_id() -> i32 {
    NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed) as i32
}

pub fn next_connection_id() -> i64 {
    NEXT_CONNECTION_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug)]
pub struct ScramState {
    pub conversation_id: i32,
    pub client_nonce: String,
    pub server_nonce: String,
    pub salt: Vec<u8>,
    pub iterations: u32,
    pub auth_message: String,
    pub server_key: Vec<u8>,
}

pub struct ConnectionState {
    pub remote_addr: SocketAddr,
    pub connection_id: i64,
    pub authenticated: bool,
    pub auth_user: Option<String>,
    pub scram_state: Option<ScramState>,
    pub cursor_store: CursorStore,
    pub session_store: SessionStore,
    pub change_stream_store: ChangeStreamStore,
}

impl ConnectionState {
    pub fn new(remote_addr: SocketAddr) -> Self {
        Self {
            remote_addr,
            connection_id: next_connection_id(),
            authenticated: false,
            auth_user: None,
            scram_state: None,
            cursor_store: CursorStore::default(),
            session_store: SessionStore::default(),
            change_stream_store: ChangeStreamStore::default(),
        }
    }
}
