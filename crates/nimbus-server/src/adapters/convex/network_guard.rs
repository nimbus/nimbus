//! Fail-closed network-exposure stopgap for the Convex application surface (#41).

use std::sync::Arc;

use axum::extract::State;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::state::{AppError, AppState};

/// Refuse the Convex application surface on any non-loopback bind (#41 stopgap).
///
/// The convex application routes (`/convex/{tenant_id}/…`) select the tenant from
/// the caller-supplied URL path with **no verified principal→tenant binding**, so
/// an unverified caller can reach an arbitrary tenant's data partition (#41 —
/// confirmed cross-tenant read **and write**). Until that binding lands, this
/// guard refuses the **entire** convex application surface on any **non-loopback**
/// bind. It is the convex analog of the firebase
/// `ensure_firebase_bypass_loopback_only` / MongoDB `guard_bind_address`
/// "unsound mode → loopback-only" shape — applied per-request (as a route-layer
/// over `build_convex_router`, so it covers all six convex route types) rather
/// than at boot, because convex shares the main HTTP listener and a boot-level
/// refusal would block the whole server. Loopback is allowed for local dev; the
/// unset test address is treated as loopback (production always sets
/// `listen_addr` from the bound socket in `construction.rs`).
///
/// **Fork-independent:** it does not decide *how* the tenant will eventually be
/// bound (the #41 A-vs-B product call) — only that the currently-unbound surface
/// must not be network-reachable. The complete fix replaces this guard with the
/// real binding check.
pub(crate) async fn convex_application_network_bind_guard(
    State(state): State<Arc<AppState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if state
        .listen_addr
        .is_some_and(|addr| !addr.ip().is_loopback())
    {
        return AppError::forbidden(
            "the Convex application API is refused on a non-loopback bind: it selects the tenant \
             from the request URL with no verified tenant binding (#41), so it is restricted to a \
             loopback bind until the binding lands. Bind on loopback for local development.",
        )
        .into_response();
    }
    next.run(request).await
}
