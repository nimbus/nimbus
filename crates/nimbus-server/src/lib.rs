//! Nimbus server crate.

mod adapters;
mod application_auth;
mod config;
mod construction;
mod error_envelope;
mod http;
mod latency;
mod license;
mod local_server;
mod owned_tasks;
mod protocol;
mod router;
mod state;
mod system;
mod system_tenant;
mod tenant;
#[cfg(test)]
mod tenant_isolation_drift;
mod tls;
mod ws;

// CP1: runtime execution, artifact/provenance admission, machine lifecycle,
// and service manager wiring moved to the transport-free nimbus-compute
// crate. Bringing them in as crate-root `use` items (rather than re-declaring
// `mod`) keeps every existing `crate::execution::...` etc. call site in this
// crate unchanged. (`artifact_verifier_effects` has no consumers left in this
// crate — its only caller, `execution::invocations::provenance`, moved with
// it — so it is not re-imported here.)
use nimbus_compute::execution;
pub use nimbus_compute::machine_lifecycle;
#[cfg(test)]
use nimbus_compute::service_manager;

pub use adapters::cloud_functions::{CloudFunctionsHttpTenantBinding, CloudFunctionsRegistry};
pub use adapters::cloudflare::{
    CloudflareBindingRegistry, CloudflareConfig, D1DatabaseBinding, DurableObjectBinding,
    KvNamespaceBinding, R2BucketBinding, WranglerConfigError,
};
pub use adapters::convex::ConvexRegistry;
pub use adapters::convex::{ConvexTenancyConfig, PrincipalTeamRegistry, SiloTeamRegistry, TeamId};
pub use adapters::dynamodb::DynamoDbConfig;
pub use adapters::firebase::{FirebaseConfig, ProjectSpecError, ProjectTenantRegistry};
pub use adapters::s3::S3Config;
/// Enables Firebase Emulator token-verification bypass for dev/test servers.
///
/// The default Firebase config rejects unverified emulator tokens and uses a
/// strict empty project registry. This helper opts into the loopback-only
/// dev-mode bypass and identity project registry together, matching local
/// emulator semantics without weakening production defaults.
#[must_use]
pub fn enable_firebase_emulator_token_verification_bypass(
    firebase_config: FirebaseConfig,
) -> FirebaseConfig {
    firebase_config
        .with_emulator_token_verification_bypass()
        .with_project_registry(ProjectTenantRegistry::identity())
}
pub use adapters::mongodb::{
    AuthConfig as MongoDbAuthConfig, CredentialRegistry as MongoDbCredentialRegistry, MongoDbConfig,
};
pub use construction::{ServeOptions, serve};
pub use local_server::{
    SERVER_DISCOVERY_PROTOCOL_VERSIONS, ServerDiscoveryLease, ServerDiscoveryRecord,
    read_live_server_discovery,
};
pub use nimbus_dynamodb::AccessKeyRegistry as DynamoDbAccessKeyRegistry;
pub use nimbus_s3::AccessKeyRegistry as S3AccessKeyRegistry;
pub use router::{RouterOptions, build_router, normalize_cors_origin};
pub use tls::TlsConfig;

#[cfg(test)]
mod tests;
