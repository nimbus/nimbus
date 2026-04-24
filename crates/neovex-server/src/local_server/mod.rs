mod access;
mod audit;
mod discovery;
mod middleware;
mod paths;
mod policy;
mod token;

pub(crate) use access::LOCAL_SESSION_COOKIE_NAME;
pub use access::{
    IssuedSessionCookie, LocalServerSecurityState, SessionBootstrapFailure, SessionValidationResult,
};
#[cfg(test)]
pub(crate) use audit::LocalServerAuditRecord;
pub(crate) use audit::{LocalServerAuditEvent, origin_from_headers, tenant_id_from_path};
pub use discovery::{
    SERVER_DISCOVERY_PROTOCOL_VERSIONS, ServerDiscoveryLease, ServerDiscoveryRecord,
    read_live_server_discovery,
};
pub(crate) use middleware::{
    LocalServerAccessPolicy, origin_allowlist_middleware, route_family_gate_middleware,
    server_access_extract_middleware,
};
pub use paths::{LocalServerPaths, LocalServerPlatform};
pub(crate) use policy::LocalServerRouteFamily;
pub use token::{
    LOCAL_ADMIN_TOKEN_SCOPE, LocalAdminTokenRecord, load_local_admin_token,
    load_or_create_local_admin_token, rotate_local_admin_token_offline,
};
