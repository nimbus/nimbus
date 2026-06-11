mod discovery;
mod middleware;

pub use discovery::{
    SERVER_DISCOVERY_PROTOCOL_VERSIONS, ServerDiscoveryLease, ServerDiscoveryRecord,
    read_live_server_discovery,
};
pub(crate) use middleware::{
    LocalServerAccessPolicy, origin_allowlist_middleware, route_family_gate_middleware,
    server_access_extract_middleware,
};
pub(crate) use nimbus_operator::LOCAL_SESSION_COOKIE_NAME;
#[cfg(test)]
pub(crate) use nimbus_operator::LocalServerAuditRecord;
pub(crate) use nimbus_operator::LocalServerRouteFamily;
pub use nimbus_operator::{
    IssuedSessionCookie, LocalServerSecurityState, SessionBootstrapFailure, SessionValidationResult,
};
pub use nimbus_operator::{
    LOCAL_ADMIN_HEADER_NAME, LOCAL_ADMIN_TOKEN_SCOPE, LocalAdminTokenRecord,
    load_local_admin_token, load_or_create_local_admin_token, rotate_local_admin_token_offline,
};
pub(crate) use nimbus_operator::{
    LocalServerAuditEvent, origin_from_headers, tenant_id_from_request,
};
pub use nimbus_operator::{LocalServerPaths, LocalServerPlatform};
pub(crate) use nimbus_operator::{
    LocalServerPolicyError, authorize_deploy_admin_bearer, authorize_standard_server_access,
    extract_required_bearer_token,
};
