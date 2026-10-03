use super::*;
use nimbus_convex_host::{
    bootstrap_runtime_named_subscription_async, next_runtime_subscription_server_request_id,
};

mod socket;

pub(super) async fn handle_convex_socket_for_tenant(
    socket: WebSocket,
    state: Arc<AppState>,
    admission: super::socket_auth::ConvexSocketAdmission,
    protocol: crate::ws::NegotiatedWebSocketProtocol,
) {
    socket::handle_convex_socket_for_tenant(socket, state, admission, protocol).await;
}
