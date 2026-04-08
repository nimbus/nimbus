pub(super) use super::auth_fixtures::*;
pub(super) use super::*;
use std::sync::OnceLock;
use tokio::sync::{Mutex, MutexGuard};

pub(super) async fn auth_test_guard() -> MutexGuard<'static, ()> {
    static AUTH_TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    AUTH_TEST_MUTEX.get_or_init(|| Mutex::new(())).lock().await
}

#[path = "auth/http_bearer/mod.rs"]
mod http_bearer;
#[path = "auth/oidc/mod.rs"]
mod oidc;
#[path = "auth/websocket_auth.rs"]
pub(super) mod websocket_auth;
