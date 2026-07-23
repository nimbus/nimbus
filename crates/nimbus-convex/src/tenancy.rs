//! Explicit anonymous-access policy for Convex silos.
//!
//! Authenticated Convex callers are authorized by [`crate::ConvexSiloAuthRegistry`]:
//! the URL silo selects a deployment-provisioned verifier before Nimbus examines
//! the bearer token. A token accepted by that verifier is therefore intrinsically
//! bound to the selected silo. No subject, issuer, or caller-controlled claim is
//! translated into a silo authorization here.
//!
//! Anonymous callers have no verifier proof. They remain fail-closed unless an
//! operator explicitly binds both the requested silo and anonymous traffic to the
//! same team. This separate policy exists for local development and deliberate
//! public-function deployments; it never widens authenticated access.

use std::collections::BTreeMap;
use std::fmt;

use nimbus_core::TenantId;

/// An operator-defined group of silos that may share an anonymous-access policy.
/// It is an adapter-level authorization label, never a data-partition key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TeamId(String);

impl TeamId {
    /// Build a team id. It must be non-empty and free of whitespace and `:`
    /// (the operator-spec separator).
    pub fn new(value: impl Into<String>) -> Result<Self, TeamIdError> {
        let value = value.into();
        if value.is_empty() {
            return Err(TeamIdError("team id must not be empty".to_string()));
        }
        if value.contains(char::is_whitespace) {
            return Err(TeamIdError(format!(
                "team id `{value}` must not contain whitespace"
            )));
        }
        if value.contains(':') {
            return Err(TeamIdError(format!(
                "team id `{value}` must not contain `:`"
            )));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TeamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamIdError(String);

impl fmt::Display for TeamIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TeamIdError {}

/// Maps each Convex silo to the team that owns its anonymous-access policy.
/// An unregistered silo is never anonymously reachable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SiloTeamRegistry {
    bindings: BTreeMap<String, TeamId>,
}

impl SiloTeamRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn bind(mut self, silo: &TenantId, team: TeamId) -> Self {
        self.bindings.insert(silo.as_str().to_string(), team);
        self
    }

    #[must_use]
    pub fn team_for_silo(&self, silo: &TenantId) -> Option<&TeamId> {
        self.bindings.get(silo.as_str())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Provisioned silo ids. Values were validated when inserted or parsed.
    #[must_use]
    pub fn silos(&self) -> Vec<TenantId> {
        self.bindings
            .keys()
            .map(|silo| TenantId::new(silo).expect("stored silo ids were validated"))
            .collect()
    }

    /// Parse `NIMBUS_CONVEX_SILO_TEAMS`: comma-separated `SILO:TEAM` entries.
    /// Splitting on the last `:` permits `:` inside a silo id.
    pub fn from_operator_spec(spec: &str) -> Result<Self, TenancySpecError> {
        let mut registry = Self::new();
        for entry in spec.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (silo, team) = entry.rsplit_once(':').ok_or_else(|| {
                spec_error(format!(
                    "invalid silo→team binding `{entry}`: expected SILO:TEAM"
                ))
            })?;
            if silo.is_empty() || team.is_empty() {
                return Err(spec_error(format!(
                    "invalid silo→team binding `{entry}`: every segment must be non-empty"
                )));
            }
            let silo_id = TenantId::new(silo)
                .map_err(|e| spec_error(format!("invalid silo→team binding `{entry}`: {e}")))?;
            let team_id = TeamId::new(team)
                .map_err(|e| spec_error(format!("invalid silo→team binding `{entry}`: {e}")))?;
            registry = registry.bind(&silo_id, team_id);
        }
        Ok(registry)
    }
}

/// Why an anonymous Convex silo selection was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvexTeamAuthzError {
    UnregisteredSilo {
        silo: String,
    },
    AnonymousAccessDisabled {
        silo: String,
    },
    AnonymousTeamDoesNotOwnSilo {
        silo: String,
        silo_team: String,
        anonymous_team: String,
    },
}

impl fmt::Display for ConvexTeamAuthzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnregisteredSilo { silo } => write!(
                f,
                "Convex silo `{silo}` has no anonymous-access policy; anonymous selection is refused"
            ),
            Self::AnonymousAccessDisabled { silo } => write!(
                f,
                "anonymous Convex access is disabled, so silo `{silo}` cannot be selected"
            ),
            Self::AnonymousTeamDoesNotOwnSilo {
                silo,
                silo_team,
                anonymous_team,
            } => write!(
                f,
                "Convex silo `{silo}` belongs to team `{silo_team}`, but anonymous access is bound \
                 to team `{anonymous_team}`"
            ),
        }
    }
}

impl std::error::Error for ConvexTeamAuthzError {}

/// Convex anonymous-access policy.
///
/// Defaults are empty and fail-closed. Authenticated requests do not consult
/// this policy; they are admitted only by the verifier bound to their URL silo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConvexTenancyConfig {
    silo_teams: SiloTeamRegistry,
    anonymous_team: Option<TeamId>,
}

impl ConvexTenancyConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_silo_teams(mut self, silo_teams: SiloTeamRegistry) -> Self {
        self.silo_teams = silo_teams;
        self
    }

    #[must_use]
    pub fn silos(&self) -> Vec<TenantId> {
        self.silo_teams.silos()
    }

    /// Explicitly bind anonymous requests to a team. This is intended for local
    /// development or deliberate public-function policy.
    #[must_use]
    pub fn with_anonymous_team(mut self, anonymous_team: TeamId) -> Self {
        self.anonymous_team = Some(anonymous_team);
        self
    }

    /// Authorize an anonymous request to select `url_silo`.
    pub fn authorize_anonymous_silo_selection(
        &self,
        url_silo: &TenantId,
    ) -> Result<(), ConvexTeamAuthzError> {
        let silo = url_silo.as_str().to_string();
        let silo_team = self
            .silo_teams
            .team_for_silo(url_silo)
            .ok_or_else(|| ConvexTeamAuthzError::UnregisteredSilo { silo: silo.clone() })?;
        let anonymous_team = self
            .anonymous_team
            .as_ref()
            .ok_or_else(|| ConvexTeamAuthzError::AnonymousAccessDisabled { silo: silo.clone() })?;
        if silo_team != anonymous_team {
            return Err(ConvexTeamAuthzError::AnonymousTeamDoesNotOwnSilo {
                silo,
                silo_team: silo_team.to_string(),
                anonymous_team: anonymous_team.to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenancySpecError(String);

impl TenancySpecError {
    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TenancySpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TenancySpecError {}

fn spec_error(message: String) -> TenancySpecError {
    TenancySpecError(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant id")
    }

    fn team(id: &str) -> TeamId {
        TeamId::new(id).expect("team id")
    }

    fn policy() -> ConvexTenancyConfig {
        ConvexTenancyConfig::new()
            .with_silo_teams(
                SiloTeamRegistry::new()
                    .bind(&tenant("silo-1"), team("team-a"))
                    .bind(&tenant("silo-2"), team("team-a"))
                    .bind(&tenant("silo-3"), team("team-b")),
            )
            .with_anonymous_team(team("team-a"))
    }

    #[test]
    fn anonymous_access_is_fail_closed_by_default() {
        let error = ConvexTenancyConfig::new()
            .authorize_anonymous_silo_selection(&tenant("silo-1"))
            .expect_err("unconfigured policy must refuse anonymous access");
        assert!(matches!(
            error,
            ConvexTeamAuthzError::UnregisteredSilo { .. }
        ));
    }

    #[test]
    fn registered_silo_still_requires_an_explicit_anonymous_team() {
        let config = ConvexTenancyConfig::new()
            .with_silo_teams(SiloTeamRegistry::new().bind(&tenant("silo-1"), team("team-a")));
        let error = config
            .authorize_anonymous_silo_selection(&tenant("silo-1"))
            .expect_err("registered silo alone must not enable anonymous access");
        assert!(matches!(
            error,
            ConvexTeamAuthzError::AnonymousAccessDisabled { .. }
        ));
    }

    #[test]
    fn anonymous_access_is_limited_to_the_bound_team() {
        policy()
            .authorize_anonymous_silo_selection(&tenant("silo-2"))
            .expect("another silo owned by the anonymous team should be admitted");
        let error = policy()
            .authorize_anonymous_silo_selection(&tenant("silo-3"))
            .expect_err("another team's silo must be refused");
        assert!(matches!(
            error,
            ConvexTeamAuthzError::AnonymousTeamDoesNotOwnSilo { .. }
        ));
    }

    #[test]
    fn unregistered_silo_is_refused_even_when_anonymous_access_is_enabled() {
        let error = policy()
            .authorize_anonymous_silo_selection(&tenant("unknown"))
            .expect_err("unregistered silo must be refused");
        assert!(matches!(
            error,
            ConvexTeamAuthzError::UnregisteredSilo { .. }
        ));
    }

    #[test]
    fn operator_spec_parses_multiple_silos_and_rejects_malformed_entries() {
        let registry =
            SiloTeamRegistry::from_operator_spec("silo-1:team-a, silo-2:team-a, silo-3:team-b,")
                .expect("silo spec should parse");
        assert_eq!(
            registry.team_for_silo(&tenant("silo-2")),
            Some(&team("team-a"))
        );
        assert_eq!(registry.silos().len(), 3);
        assert!(SiloTeamRegistry::from_operator_spec("no-colon").is_err());
        assert!(SiloTeamRegistry::from_operator_spec("silo:").is_err());
        assert!(SiloTeamRegistry::from_operator_spec(":team").is_err());
    }
}
