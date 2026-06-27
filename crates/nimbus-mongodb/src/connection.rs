use std::net::SocketAddr;
use std::sync::atomic::{AtomicI64, Ordering};

use nimbus_core::{PrincipalContext, TenantId};
use serde_json::{Map, Value};

use super::commands::cursor::CursorStore;
use super::commands::session::SessionStore;
use super::commands::tenant::AUTHENTICATED_TENANT_CLAIM;

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
    /// The raw (SCRAM-unescaped) authenticated username, carried from `saslStart`
    /// so `saslContinue` re-resolves the same credential material and tenant.
    pub username: String,
    pub client_nonce: String,
    pub server_nonce: String,
    pub salt: Vec<u8>,
    pub iterations: u32,
    pub auth_message: String,
    pub server_key: Vec<u8>,
}

pub struct ConnectionState {
    pub(crate) remote_addr: SocketAddr,
    pub(crate) connection_id: i64,
    pub(crate) authenticated: bool,
    pub(crate) auth_user: Option<String>,
    /// The tenant authentication bound this connection to, set on a successful
    /// SCRAM handshake in bound mode ([`crate::MongoAuth::Bound`]) and left
    /// `None` in tenant-agnostic unbound mode. When set, it — not the wire
    /// `$db` — decides the tenant for every command on this connection.
    pub(crate) authenticated_tenant: Option<TenantId>,
    pub(crate) scram_state: Option<ScramState>,
    pub(crate) cursor_store: CursorStore,
    pub(crate) session_store: SessionStore,
}

impl ConnectionState {
    pub fn new(remote_addr: SocketAddr) -> Self {
        Self {
            remote_addr,
            connection_id: next_connection_id(),
            authenticated: false,
            auth_user: None,
            authenticated_tenant: None,
            scram_state: None,
            cursor_store: CursorStore::default(),
            session_store: SessionStore::default(),
        }
    }

    /// The tenant authentication bound this connection to, if any.
    pub(crate) fn authenticated_tenant(&self) -> Option<&TenantId> {
        self.authenticated_tenant.as_ref()
    }

    pub(crate) fn authenticated_principal(&self) -> Option<PrincipalContext> {
        if !self.authenticated {
            return None;
        }
        let user = self.auth_user.as_ref()?;
        let mut claims = Map::new();
        claims.insert("subject".to_string(), Value::String(user.clone()));
        claims.insert("sub".to_string(), Value::String(user.clone()));
        claims.insert("mongodb_user".to_string(), Value::String(user.clone()));
        claims.insert("provider".to_string(), Value::String("mongodb".to_string()));

        // Carry the authentication-bound tenant into the principal so the
        // tenant-resolution path (which only receives the principal) reads it
        // back via `tenant::authenticated_tenant_from_principal`. Placed in
        // `verified_claims` because it is established by the server's own SCRAM
        // handshake, not asserted by the client.
        let mut verified_claims = Map::new();
        if let Some(tenant) = self.authenticated_tenant() {
            verified_claims.insert(
                AUTHENTICATED_TENANT_CLAIM.to_string(),
                Value::String(tenant.as_str().to_string()),
            );
        }

        Some(PrincipalContext {
            authenticated: true,
            claims,
            verified_claims,
        })
    }
}
