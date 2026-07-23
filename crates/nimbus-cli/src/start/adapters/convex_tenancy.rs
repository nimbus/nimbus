use nimbus::{Error, TenantId};
use nimbus_server::{ConvexTenancyConfig, SiloTeamRegistry, TeamId};

use crate::CONVEX_SILO_ENV;
use crate::start::StartCommand;

/// Per-silo team bindings used only by the explicit anonymous-access policy.
/// Comma-separated `SILO_TENANT:TEAM` entries.
pub(super) const CONVEX_SILO_TEAMS_ENV: &str = "NIMBUS_CONVEX_SILO_TEAMS";
/// The team assigned to anonymous Convex requests. It must match the team of a
/// requested silo in [`CONVEX_SILO_TEAMS_ENV`]. Authenticated requests never
/// consult this policy; their URL silo selects a verifier directly.
pub(super) const CONVEX_ANONYMOUS_TEAM_ENV: &str = "NIMBUS_CONVEX_ANONYMOUS_TEAM";

const DEV_AUTO_TEAM: &str = "nimbus-dev";

pub(super) fn resolve_convex_auth_silos(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<Vec<TenantId>, Error> {
    let raw = env_lookup(CONVEX_SILO_ENV).or_else(|| command.auto_tenant.clone());
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let silo = TenantId::new(raw.trim())
        .map_err(|error| Error::InvalidInput(format!("invalid {CONVEX_SILO_ENV}: {error}")))?;
    Ok(vec![silo])
}

/// Resolve anonymous Convex access policy.
///
/// Plain `start` is fail-closed unless the operator supplies configuration.
/// `nimbus dev` auto-binds its generated tenant for anonymous local clients.
/// Supplying either environment variable disables that dev default wholesale.
pub(super) fn resolve_convex_tenancy(
    command: &StartCommand,
    env_lookup: &impl Fn(&str) -> Option<String>,
) -> Result<(Option<ConvexTenancyConfig>, Option<String>), Error> {
    let silo_raw = env_lookup(CONVEX_SILO_TEAMS_ENV);
    let anonymous_raw = env_lookup(CONVEX_ANONYMOUS_TEAM_ENV);

    if silo_raw.is_none() && anonymous_raw.is_none() {
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
             auto-bound to a dev team); set {CONVEX_SILO_TEAMS_ENV} or \
             {CONVEX_ANONYMOUS_TEAM_ENV} to override"
        );
        return Ok((Some(config), Some(notice)));
    }

    let mut config = ConvexTenancyConfig::new();
    if let Some(raw) = silo_raw {
        let registry = SiloTeamRegistry::from_operator_spec(&raw)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        config = config.with_silo_teams(registry);
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
        let (config, notice) = resolve_convex_tenancy(&StartCommand::default(), &|_| None)
            .expect("plain start should resolve");
        assert!(config.is_none());
        assert!(notice.is_none());
    }

    #[test]
    fn dev_with_no_envs_auto_provisions_anonymous_access() {
        let (config, notice) = resolve_convex_tenancy(&command_with_auto_tenant("demo"), &|_| None)
            .expect("dev boot should resolve");
        let config = config.expect("dev must auto-provision a policy");
        config
            .authorize_anonymous_silo_selection(&TenantId::new("demo").expect("tenant id"))
            .expect("the auto tenant must be anonymously reachable");
        config
            .authorize_anonymous_silo_selection(&TenantId::new("other").expect("tenant id"))
            .expect_err("another silo must remain refused");
        assert!(notice.expect("notice").contains("demo"));
    }

    #[test]
    fn explicit_env_skips_dev_auto_provisioning_wholesale() {
        let (config, notice) = resolve_convex_tenancy(&command_with_auto_tenant("demo"), &|name| {
            (name == CONVEX_SILO_TEAMS_ENV).then(|| "demo:team-a".to_string())
        })
        .expect("explicit config should resolve");
        config
            .expect("explicit config")
            .authorize_anonymous_silo_selection(&TenantId::new("demo").expect("tenant id"))
            .expect_err("silo binding alone must not enable anonymous access");
        assert!(notice.is_none());
    }

    #[test]
    fn explicit_matching_policy_enables_anonymous_access() {
        let (config, notice) =
            resolve_convex_tenancy(&StartCommand::default(), &|name| match name {
                CONVEX_SILO_TEAMS_ENV => Some("demo:team-a".to_string()),
                CONVEX_ANONYMOUS_TEAM_ENV => Some("team-a".to_string()),
                _ => None,
            })
            .expect("explicit config should resolve");
        config
            .expect("explicit config")
            .authorize_anonymous_silo_selection(&TenantId::new("demo").expect("tenant id"))
            .expect("matching policy must admit anonymous access");
        assert!(notice.expect("notice").contains(CONVEX_ANONYMOUS_TEAM_ENV));
    }

    #[test]
    fn malformed_anonymous_team_is_a_hard_boot_error() {
        let error = resolve_convex_tenancy(&StartCommand::default(), &|name| {
            (name == CONVEX_ANONYMOUS_TEAM_ENV).then(|| "bad team".to_string())
        })
        .expect_err("whitespace must be rejected");
        assert!(matches!(error, Error::InvalidInput(_)));
    }

    #[test]
    fn startup_auth_silo_is_explicit_and_dev_uses_its_auto_tenant() {
        assert!(
            resolve_convex_auth_silos(&StartCommand::default(), &|_| None)
                .expect("plain start should resolve")
                .is_empty()
        );
        assert_eq!(
            resolve_convex_auth_silos(&command_with_auto_tenant("demo"), &|_| None)
                .expect("dev should resolve")[0]
                .as_str(),
            "demo"
        );
        assert_eq!(
            resolve_convex_auth_silos(&command_with_auto_tenant("demo"), &|name| {
                (name == CONVEX_SILO_ENV).then(|| "production".to_string())
            })
            .expect("explicit startup silo should resolve")[0]
                .as_str(),
            "production"
        );
    }
}
