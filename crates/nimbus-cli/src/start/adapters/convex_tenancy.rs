use nimbus::Error;
use nimbus_server::{ConvexTenancyConfig, PrincipalTeamRegistry, SiloTeamRegistry};

/// Per-silo team bindings (#41 application-Convex team-binding gate).
/// Comma-separated `SILO_TENANT:TEAM` entries, parsed by
/// [`SiloTeamRegistry::from_operator_spec`]. When set, the application-Convex
/// admission funnel authorizes a request's silo selection against the
/// principal's team. When unset the deployment keeps the empty, fail-closed
/// [`ConvexTenancyConfig::default`], which refuses every silo selection because
/// no silo maps to a team. A malformed spec is a hard boot error, never a
/// silent permissive default.
const CONVEX_SILO_TEAMS_ENV: &str = "NIMBUS_CONVEX_SILO_TEAMS";
/// Verified-principal team bindings (#41 application-Convex team-binding gate).
/// Comma-separated `SUBJECT@ISSUER:TEAM` entries, parsed by
/// [`PrincipalTeamRegistry::from_operator_spec`]. Pairs with
/// [`CONVEX_SILO_TEAMS_ENV`]: a principal may only select a silo owned by its
/// bound team. When unset the deployment keeps the empty, fail-closed
/// [`ConvexTenancyConfig::default`]. A malformed spec is a hard boot error.
const CONVEX_PRINCIPAL_TEAMS_ENV: &str = "NIMBUS_CONVEX_PRINCIPAL_TEAMS";

/// Resolve the application-Convex team-binding tenancy config (#41), ingesting
/// the silo->team and principal->team registries from [`CONVEX_SILO_TEAMS_ENV`]
/// and [`CONVEX_PRINCIPAL_TEAMS_ENV`] when present.
///
/// When neither env is set this returns `Ok(None)`: the deployment keeps the
/// fail-closed [`ConvexTenancyConfig::default`] (no `convex_tenancy` plumbed),
/// so the admission funnel refuses every application-Convex silo selection. When
/// at least one env is set the registries are parsed by the same
/// `from_operator_spec` the gate uses elsewhere. A malformed spec is a hard
/// `InvalidInput` boot error, mirroring the Firebase project ingestion; an unset
/// env never falls back to a permissive registry.
pub(super) fn resolve_convex_tenancy(
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Option<ConvexTenancyConfig>, Error> {
    let silo_raw = env_lookup(CONVEX_SILO_TEAMS_ENV);
    let principal_raw = env_lookup(CONVEX_PRINCIPAL_TEAMS_ENV);
    if silo_raw.is_none() && principal_raw.is_none() {
        return Ok(None);
    }
    let mut config = ConvexTenancyConfig::new();
    if let Some(raw) = silo_raw {
        let registry = SiloTeamRegistry::from_operator_spec(&raw)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        config = config.with_silo_teams(registry);
    }
    if let Some(raw) = principal_raw {
        let registry = PrincipalTeamRegistry::from_operator_spec(&raw)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        config = config.with_principal_teams(registry);
    }
    Ok(Some(config))
}
