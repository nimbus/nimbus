use nimbus::{Error, TenantId};
use nimbus_server::{FirebaseConfig, ProjectTenantRegistry};

use crate::start::StartCommand;

/// Per-tenant Firebase project->tenant bindings. Comma-separated
/// `PROJECT:TENANT` entries, mirroring the MongoDB `MONGODB_CREDENTIALS_ENV`
/// convention. When set the Firebase adapter resolves each request's project to
/// its bound tenant through this registry; when unset the adapter keeps the
/// default empty registry from [`FirebaseConfig::new`], which refuses every
/// request because no project maps to a tenant. A malformed entry is a hard
/// boot error, never a silent permissive default.
pub(super) const FIREBASE_PROJECTS_ENV: &str = "NIMBUS_FIREBASE_PROJECTS";

/// Resolve the Firebase adapter config, ingesting the project->tenant registry
/// from [`FIREBASE_PROJECTS_ENV`] when present.
///
/// When the surface is opted out this returns `Ok(None)`. When enabled the
/// adapter starts from [`FirebaseConfig::new`] (an empty, strict refuse-all
/// registry) and installs operator bindings only when the env is set, parsed by
/// the same [`ProjectTenantRegistry::from_operator_spec`] the registry uses
/// elsewhere. A malformed spec is a hard `InvalidInput` boot error, mirroring
/// the MongoDB credential ingestion; an unset env never falls back to a
/// permissive registry.
pub(super) fn resolve_firebase(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<FirebaseConfig>, Error> {
    if !command.firestore {
        return Ok(None);
    }
    let mut config = FirebaseConfig::new();
    if let Some(auto_tenant) = &command.auto_tenant {
        let tenant = TenantId::new(auto_tenant)
            .map_err(|error| Error::InvalidInput(format!("invalid auto tenant: {error}")))?;
        config = config
            .with_emulator_token_verification_bypass()
            .with_project_registry(ProjectTenantRegistry::new().bind(auto_tenant, tenant));
    } else if let Some(raw) = env_lookup(FIREBASE_PROJECTS_ENV) {
        let registry = ProjectTenantRegistry::from_operator_spec(&raw)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        config = config.with_project_registry(registry);
    }
    Ok(Some(config))
}
