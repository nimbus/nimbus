use super::execution::{
    bootstrap_runtime_named_subscription_async, next_runtime_server_request_id,
};
use super::*;

mod socket;
mod transforms;

fn next_runtime_subscription_server_request_id(prefix: &str) -> String {
    next_runtime_server_request_id(prefix)
}

pub(super) fn is_scalar_filter_value(value: &Value) -> bool {
    transforms::is_scalar_filter_value(value)
}

pub(super) fn should_replace_lower_bound(
    current: Option<&Value>,
    candidate: Option<&Value>,
    candidate_inclusive: bool,
) -> bool {
    transforms::should_replace_lower_bound(current, candidate, candidate_inclusive)
}

pub(super) fn should_replace_upper_bound(
    current: Option<&Value>,
    candidate: Option<&Value>,
    candidate_inclusive: bool,
) -> bool {
    transforms::should_replace_upper_bound(current, candidate, candidate_inclusive)
}

pub(super) async fn handle_convex_socket_for_tenant(
    socket: WebSocket,
    state: Arc<AppState>,
    admission: super::socket_auth::ConvexSocketAdmission,
    protocol: crate::ws::NegotiatedWebSocketProtocol,
) {
    socket::handle_convex_socket_for_tenant(socket, state, admission, protocol).await;
}
