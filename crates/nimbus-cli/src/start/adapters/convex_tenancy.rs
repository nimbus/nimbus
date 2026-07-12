use nimbus::{Error, TenantId};
use nimbus_server::{ConvexTenancyConfig, PrincipalTeamRegistry, SiloTeamRegistry, TeamId};

use crate::start::StartCommand;

/// Per-silo team bindings (#41 application-Convex team-binding gate).
/// Comma-separated `SILO_TENANT:TEAM` entries, parsed by
/// [`SiloTeamRegistry::from_operator_spec`]. When set, the application-Convex
/// admission funnel authorizes a request's silo selection against the
/// principal's team. When unset the deployment keeps the empty, fail-closed
/// [`ConvexTenancyConfig::default`], which refuses every silo selection because
/// no silo maps to a team. A malformed spec is a hard boot error, never a
/// silent permissive default.
pub(super) const CONVEX_SILO_TEAMS_ENV: &str = "NIMBUS_CONVEX_SILO_TEAMS";
/// Verified-principal team bindings (#41 application-Convex team-binding gate).
/// Comma-separated `SUBJECT@ISSUER:TEAM` entries, parsed by
/// [`PrincipalTeamRegistry::from_operator_spec`]. Pairs with
/// [`CONVEX_SILO_TEAMS_ENV`]: a principal may only select a silo owned by its
/// bound team. When unset the deployment keeps the empty, fail-closed
/// [`ConvexTenancyConfig::default`]. A malformed spec is a hard boot error.
pub(super) const CONVEX_PRINCIPAL_TEAMS_ENV: &str = "NIMBUS_CONVEX_PRINCIPAL_TEAMS";
/// Anonymous-principal team binding (#41 gate, EX3.7 dev-parity addition). A
/// single `TEAM` name — unlike the other two envs this is not a comma-separated
/// mapping, since it names exactly one team that every anonymous
/// (unauthenticated) application-Convex request is treated as belonging to,
/// via [`ConvexTenancyConfig::with_anonymous_team`]. Pairs with
/// [`CONVEX_SILO_TEAMS_ENV`]: anonymous requests may still only select a silo
/// bound to that team. Verified principals are entirely unaffected — this env
/// never widens what a verified caller can reach. When unset the deployment
/// keeps refusing every anonymous application-Convex request, exactly as
/// before this env existed. This is an explicit operator opt-in for `start`;
/// see [`resolve_convex_tenancy`]'s `nimbus dev` auto-provisioning behavior for
/// the intended local-development path that does not require setting it.
pub(super) const CONVEX_ANONYMOUS_TEAM_ENV: &str = "NIMBUS_CONVEX_ANONYMOUS_TEAM";

/// The team name `nimbus dev` auto-provisions for its local development loop
/// when no `NIMBUS_CONVEX_*` env is set. Fixed and process-internal: it only
/// needs to stay consistent between the silo binding and the anonymous
/// binding created together in [`resolve_convex_tenancy`].
const DEV_AUTO_TEAM: &str = "nimbus-dev";

/// Resolve the application-Convex team-binding tenancy config (#41 / EX3.7),
/// ingesting the silo->team, principal->team, and anonymous-team registries
/// from [`CONVEX_SILO_TEAMS_ENV`], [`CONVEX_PRINCIPAL_TEAMS_ENV`], and
/// [`CONVEX_ANONYMOUS_TEAM_ENV`] when present. Returns the resolved config
/// alongside an optional loud startup notice to surface whenever anonymous
/// Convex access ends up enabled, by whichever path.
///
/// **Any of the three envs present** ingests an operator-provisioned config
/// wholesale — the registries are parsed by the same `from_operator_spec` the
/// gate uses elsewhere, and this applies identically under `start` or
/// `nimbus dev`; there is no merging with the dev-mode default below. A
/// malformed spec is a hard `InvalidInput` boot error, mirroring the Firebase
/// project ingestion; an unset env never falls back to a permissive registry.
///
/// **All three envs absent, and this is a `nimbus dev` boot**
/// (`command.auto_tenant` is `Some`): auto-provisions a dev-only team that owns
/// the auto-tenant's silo, with anonymous requests bound to that same team, so
/// anonymous local application-Convex traffic — what every generated Convex
/// client sends by default, with no JWT ceremony — reaches the dev tenant.
/// This never applies to plain `start`: an operator running `start` who wants
/// anonymous access must opt in explicitly via [`CONVEX_ANONYMOUS_TEAM_ENV`].
///
/// **All three envs absent, plain `start`**: returns `(None, None)`, so the
/// deployment keeps the fail-closed [`ConvexTenancyConfig::default`], which
/// refuses every application-Convex silo selection — the pre-EX3.7 behavior,
/// unchanged.
pub(super) fn resolve_convex_tenancy(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<(Option<ConvexTenancyConfig>, Option<String>), Error> {
    let silo_raw = env_lookup(CONVEX_SILO_TEAMS_ENV);
    let principal_raw = env_lookup(CONVEX_PRINCIPAL_TEAMS_ENV);
    let anonymous_raw = env_lookup(CONVEX_ANONYMOUS_TEAM_ENV);

    if silo_raw.is_none() && principal_raw.is_none() && anonymous_raw.is_none() {
        let Some(auto_tenant) = &command.auto_tenant else {
            return Ok((None, None));
        };
        let tenant = TenantId::new(auto_tenant)
            .map_err(|error| Error::InvalidInput(format!("invalid auto tenant: {error}")))?;
        let team =
            TeamId::new(DEV_AUTO_TEAM).expect("DEV_AUTO_TEAM is a fixed, valid team id literal");
        let config = ConvexTenancyConfig::new()
            .with_silo_teams(SiloTeamRegistry::new().bind(&tenant, team.clone()))
            .with_anonymous_team(team);
        let notice = format!(
            "convex tenancy:\tanonymous access enabled for local development (silo `{auto_tenant}` \
             auto-bound to a dev team); set {CONVEX_SILO_TEAMS_ENV}, {CONVEX_PRINCIPAL_TEAMS_ENV}, \
             or {CONVEX_ANONYMOUS_TEAM_ENV} to override"
        );
        return Ok((Some(config), Some(notice)));
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
    let notice = if let Some(raw) = anonymous_raw {
        let team = TeamId::new(raw.trim()).map_err(|error| {
            Error::InvalidInput(format!(
                "invalid {CONVEX_ANONYMOUS_TEAM_ENV} value: {error}"
            ))
        })?;
        let notice = format!(
            "convex tenancy:\tanonymous access enabled via {CONVEX_ANONYMOUS_TEAM_ENV} (team `{team}`)"
        );
        config = config.with_anonymous_team(team);
        Some(notice)
    } else {
        None
    };
    Ok((Some(config), notice))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_with_auto_tenant(tenant: &str) -> StartCommand {
        StartCommand {
            auto_tenant: Some(tenant.to_string()),
            ..StartCommand::default()
        }
    }

    #[test]
    fn start_with_no_envs_stays_refuse_everything() {
        let command = StartCommand::default();
        let (config, notice) =
            resolve_convex_tenancy(&command, &|_| None).expect("plain start should resolve");
        assert!(
            config.is_none(),
            "plain start with no envs must not plumb a convex_tenancy config"
        );
        assert!(notice.is_none(), "no envs, no dev boot => no notice");
    }

    #[test]
    fn dev_with_no_envs_auto_provisions_anonymous_access() {
        let command = command_with_auto_tenant("demo");
        let (config, notice) =
            resolve_convex_tenancy(&command, &|_| None).expect("dev boot should resolve");
        let config = config.expect("nimbus dev with no envs must auto-provision a config");
        let tenant = TenantId::new("demo").expect("tenant id");
        config
            .authorize_silo_selection(&tenant, &nimbus_core::PrincipalContext::anonymous())
            .expect("anonymous access to the auto-tenant's silo must be admitted");
        let other_tenant = TenantId::new("other").expect("tenant id");
        config
            .authorize_silo_selection(&other_tenant, &nimbus_core::PrincipalContext::anonymous())
            .expect_err("an unregistered silo must still be refused");
        assert!(
            notice
                .expect("dev auto-provisioning must print a loud notice")
                .contains("demo"),
            "the notice should name the auto-provisioned tenant"
        );
    }

    #[test]
    fn dev_with_explicit_env_skips_auto_provisioning_wholesale() {
        // An operator setting any one of the three envs opts out of dev-mode
        // auto-provisioning entirely — no merging.
        let command = command_with_auto_tenant("demo");
        let (config, notice) = resolve_convex_tenancy(&command, &|name| {
            (name == CONVEX_SILO_TEAMS_ENV).then(|| "demo:team-a".to_string())
        })
        .expect("dev boot with an explicit env should resolve");
        let config = config.expect("an explicit env must still produce a config");
        let tenant = TenantId::new("demo").expect("tenant id");
        config
            .authorize_silo_selection(&tenant, &nimbus_core::PrincipalContext::anonymous())
            .expect_err(
                "explicit env config wins wholesale: anonymous must stay refused when \
                 NIMBUS_CONVEX_ANONYMOUS_TEAM was not itself set",
            );
        assert!(
            notice.is_none(),
            "no notice when anonymous access was not actually enabled"
        );
    }

    #[test]
    fn start_with_explicit_anonymous_team_env_enables_anonymous_access() {
        let command = StartCommand::default();
        let (config, notice) = resolve_convex_tenancy(&command, &|name| match name {
            CONVEX_SILO_TEAMS_ENV => Some("demo:team-a".to_string()),
            CONVEX_ANONYMOUS_TEAM_ENV => Some("team-a".to_string()),
            _ => None,
        })
        .expect("start with explicit envs should resolve");
        let config = config.expect("explicit envs must produce a config");
        let tenant = TenantId::new("demo").expect("tenant id");
        config
            .authorize_silo_selection(&tenant, &nimbus_core::PrincipalContext::anonymous())
            .expect("anonymous access bound to the matching team must be admitted");
        assert!(
            notice
                .expect("explicit anonymous-team opt-in must print a notice")
                .contains(CONVEX_ANONYMOUS_TEAM_ENV),
            "the notice should name the governing env var"
        );
    }

    #[test]
    fn malformed_anonymous_team_env_is_a_hard_boot_error() {
        let command = StartCommand::default();
        let error = resolve_convex_tenancy(&command, &|name| {
            (name == CONVEX_ANONYMOUS_TEAM_ENV).then(|| "bad team".to_string())
        })
        .expect_err("a team id with whitespace must be rejected");
        assert!(matches!(error, Error::InvalidInput(_)));
    }
}
