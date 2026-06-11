//! Local and deploy operator security model.
//!
//! Here "operator" means the administrator operating this host's server, not a
//! Kubernetes-style controller or reconciler.

mod access;
mod access_policy;
mod audit;
mod paths;
mod policy;
mod token;

pub use access::{
    AuthorizedSession, IssuedSessionCookie, LOCAL_SESSION_COOKIE_NAME, LocalAdminTokenRotation,
    LocalServerSecurityState, SessionBootstrapFailure, SessionValidationResult,
};
pub use access_policy::{
    ExtractedServerAccess, ExtractedServerAccessStatus, LocalServerCredentialMode,
    LocalServerPolicyError, authorize_deploy_admin_bearer, authorize_standard_server_access,
    credential_method_hint, extract_required_bearer_token, extract_server_access, validate_origin,
};
pub use audit::{
    LocalServerAuditEvent, LocalServerAuditLog, LocalServerAuditRecord, origin_from_headers,
    tenant_id_from_path, tenant_id_from_request,
};
pub use paths::{LocalServerPaths, LocalServerPlatform};
pub use policy::{
    LOCAL_ADMIN_HEADER_NAME, LocalServerRouteFamily, is_loopback_origin, parse_origin,
};
pub use token::{
    LOCAL_ADMIN_TOKEN_SCOPE, LocalAdminTokenRecord, load_local_admin_token,
    load_or_create_local_admin_token, rotate_local_admin_token_offline,
};
