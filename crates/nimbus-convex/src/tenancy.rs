//! Convex team-binding tenancy — the #41 complete fix (server-side, binding (b)).
//!
//! **The gap (#41).** The convex application surface (`/convex/{tenant_id}/…`)
//! selects a data silo from the caller-supplied URL with no verified
//! principal→silo binding, so an unverified caller can reach an arbitrary silo
//! (confirmed cross-tenant read + write). A verified convex token carries
//! `issuer`/`subject`/`custom_claims` but **no team/tenant signal** (the firebase
//! difference — there the issuer carried the project), and the auth verifier is
//! global, so the binding cannot be a token parse. It must be **server-side and
//! admin-provisioned**.
//!
//! **The model.** Nimbus models one tenancy level: `TenantId` = data partition =
//! the per-project **silo** (Convex isolates data per project). This adds the
//! missing authz level — a **team** that owns silos — *entirely within the convex
//! adapter*. `TenantId` stays the silo and the data-isolation unit; the engine
//! still partitions per-silo. "Team" never partitions data — it only gates
//! **which** silos a principal may select. So a team principal may reach any of
//! its team's silos, each still isolated.
//!
//! Two registries (admin-provisioned config):
//! - [`SiloTeamRegistry`]: silo (`TenantId`) → [`TeamId`]
//! - [`PrincipalTeamRegistry`]: a verified principal (`subject` then `issuer`) → [`TeamId`]
//!
//! [`authorize_silo_selection`] is **all-fail-closed** (the M9 lesson — absent
//! means refuse, never fall through): admit iff the URL silo resolves to a team
//! AND the principal resolves to a team AND the two teams match. Any of the three
//! absent → refuse.

use std::collections::BTreeMap;
use std::fmt;

use nimbus_core::{PrincipalContext, TenantId};

/// A team — the authz boundary that owns silos. Pure adapter-level authz label;
/// never a data-partition key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TeamId(String);

impl TeamId {
    /// Build a team id. Must be non-empty and free of whitespace and `:`
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

/// Maps each Convex silo (`TenantId`, the URL segment / data partition) to the
/// team that owns it. Strict: an unregistered silo resolves to nothing → the
/// admission refuses it (fail-closed).
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

    /// The owning team of `silo`, or `None` if the silo is not registered.
    #[must_use]
    pub fn team_for_silo(&self, silo: &TenantId) -> Option<&TeamId> {
        self.bindings.get(silo.as_str())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Parse the operator spec (`NIMBUS_CONVEX_SILO_TEAMS`): comma-separated
    /// `SILO:TEAM` entries. Split on the **last** `:` so a silo id may itself
    /// contain `:`; the team (which may not contain `:`) is the final segment.
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

/// Maps a verified principal to its team, by `subject` then `issuer` (whichever
/// is registered). Admin-provisioned because a convex token carries no team.
/// Strict: an unregistered (or anonymous) principal resolves to nothing → the
/// admission refuses it (fail-closed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrincipalTeamRegistry {
    bindings: BTreeMap<String, TeamId>,
}

impl PrincipalTeamRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn bind(mut self, principal_key: impl Into<String>, team: TeamId) -> Self {
        self.bindings.insert(principal_key.into(), team);
        self
    }

    /// The team of a **verified** principal, read from `verified_claims` only
    /// (never the unverified `claims` — a caller must not name its own team).
    /// Tries the verified `subject`, then the verified `issuer`. An anonymous
    /// principal has neither and resolves to `None`.
    #[must_use]
    pub fn team_for_principal(&self, principal: &PrincipalContext) -> Option<&TeamId> {
        for key in ["subject", "issuer"] {
            if let Some(value) = principal.verified_claims.get(key).and_then(|v| v.as_str())
                && let Some(team) = self.bindings.get(value)
            {
                return Some(team);
            }
        }
        None
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Parse the operator spec (`NIMBUS_CONVEX_PRINCIPAL_TEAMS`): comma-separated
    /// `PRINCIPAL:TEAM` entries, where PRINCIPAL is a verified `subject` or
    /// `issuer`. Split on the **last** `:` so an issuer URL (which contains `:`)
    /// is preserved as the key; the team is the final segment.
    pub fn from_operator_spec(spec: &str) -> Result<Self, TenancySpecError> {
        let mut registry = Self::new();
        for entry in spec.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (principal_key, team) = entry.rsplit_once(':').ok_or_else(|| {
                spec_error(format!(
                    "invalid principal→team binding `{entry}`: expected PRINCIPAL:TEAM"
                ))
            })?;
            if principal_key.is_empty() || team.is_empty() {
                return Err(spec_error(format!(
                    "invalid principal→team binding `{entry}`: every segment must be non-empty"
                )));
            }
            let team_id = TeamId::new(team).map_err(|e| {
                spec_error(format!("invalid principal→team binding `{entry}`: {e}"))
            })?;
            registry = registry.bind(principal_key, team_id);
        }
        Ok(registry)
    }
}

/// Why a convex silo selection was refused. Every variant is a hard refusal —
/// there is no admit-on-absence path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvexTeamAuthzError {
    /// The URL silo is not registered to any team.
    UnregisteredSilo { silo: String },
    /// The principal (anonymous or unprovisioned) resolves to no team.
    PrincipalHasNoTeam { silo: String },
    /// The principal's team does not own the URL silo's team.
    CrossTeam {
        silo: String,
        silo_team: String,
        principal_team: String,
    },
}

impl fmt::Display for ConvexTeamAuthzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConvexTeamAuthzError::UnregisteredSilo { silo } => write!(
                f,
                "Convex silo `{silo}` is not registered to a team; the request cannot select an \
                 unregistered silo"
            ),
            ConvexTeamAuthzError::PrincipalHasNoTeam { silo } => write!(
                f,
                "the request principal is not authorized for any team, so it cannot select Convex \
                 silo `{silo}` (anonymous and unprovisioned principals are refused)"
            ),
            ConvexTeamAuthzError::CrossTeam {
                silo,
                silo_team,
                principal_team,
            } => write!(
                f,
                "Convex silo `{silo}` belongs to team `{silo_team}`, but the principal is \
                 authorized for team `{principal_team}`; a principal may only select silos within \
                 its own team"
            ),
        }
    }
}

impl std::error::Error for ConvexTeamAuthzError {}

/// The two #41 registries bundled for the convex adapter. Travels in deployment
/// state and is read at the convex admission gate. **Defaults to empty** — an
/// unconfigured deployment refuses every application-convex request (fail-closed
/// by construction; the operator turns convex on by provisioning teams, not by
/// leaving it unconfigured).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConvexTenancyConfig {
    silo_teams: SiloTeamRegistry,
    principal_teams: PrincipalTeamRegistry,
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
    pub fn with_principal_teams(mut self, principal_teams: PrincipalTeamRegistry) -> Self {
        self.principal_teams = principal_teams;
        self
    }

    /// All-fail-closed authorization for an application-convex silo selection
    /// (see [`authorize_silo_selection`]).
    pub fn authorize_silo_selection(
        &self,
        url_silo: &TenantId,
        principal: &PrincipalContext,
    ) -> Result<(), ConvexTeamAuthzError> {
        authorize_silo_selection(&self.silo_teams, &self.principal_teams, url_silo, principal)
    }
}

/// The #41 admission decision — **all-fail-closed**. Admit iff the URL silo
/// resolves to a team, the principal resolves to a team, and the two match.
///
/// This is the binding that supersedes the #43 network-bind stopgap: it refuses
/// cross-team selection on **every** bind (loopback included), not just
/// non-loopback. Data stays per-silo isolated; this only gates selection.
pub fn authorize_silo_selection(
    silo_registry: &SiloTeamRegistry,
    principal_registry: &PrincipalTeamRegistry,
    url_silo: &TenantId,
    principal: &PrincipalContext,
) -> Result<(), ConvexTeamAuthzError> {
    let silo = url_silo.as_str().to_string();
    let silo_team = silo_registry
        .team_for_silo(url_silo)
        .ok_or_else(|| ConvexTeamAuthzError::UnregisteredSilo { silo: silo.clone() })?;
    let principal_team = principal_registry
        .team_for_principal(principal)
        .ok_or_else(|| ConvexTeamAuthzError::PrincipalHasNoTeam { silo: silo.clone() })?;
    if silo_team != principal_team {
        return Err(ConvexTeamAuthzError::CrossTeam {
            silo,
            silo_team: silo_team.to_string(),
            principal_team: principal_team.to_string(),
        });
    }
    Ok(())
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
    use serde_json::json;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant id")
    }

    fn team(id: &str) -> TeamId {
        TeamId::new(id).expect("team id")
    }

    /// A verified principal carrying `subject` and `issuer` (as
    /// `normalize_principal_context` records them for a verified convex token).
    fn verified_principal(subject: &str, issuer: &str) -> PrincipalContext {
        PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::new(),
            verified_claims: serde_json::Map::from_iter([
                ("subject".to_string(), json!(subject)),
                ("issuer".to_string(), json!(issuer)),
            ]),
        }
    }

    fn registries() -> (SiloTeamRegistry, PrincipalTeamRegistry) {
        // team-a owns silo-1 and silo-2; team-b owns silo-3.
        let silos = SiloTeamRegistry::new()
            .bind(&tenant("silo-1"), team("team-a"))
            .bind(&tenant("silo-2"), team("team-a"))
            .bind(&tenant("silo-3"), team("team-b"));
        // principal "user-a" (by subject) is on team-a; "user-b" on team-b.
        let principals = PrincipalTeamRegistry::new()
            .bind("user-a", team("team-a"))
            .bind("user-b", team("team-b"));
        (silos, principals)
    }

    #[test]
    fn anonymous_principal_is_refused() {
        let (silos, principals) = registries();
        let error = authorize_silo_selection(
            &silos,
            &principals,
            &tenant("silo-1"),
            &PrincipalContext::anonymous(),
        )
        .expect_err("anonymous must be refused");
        assert!(matches!(
            error,
            ConvexTeamAuthzError::PrincipalHasNoTeam { .. }
        ));
    }

    #[test]
    fn cross_team_silo_selection_is_refused() {
        let (silos, principals) = registries();
        // user-a (team-a) names silo-3 (team-b) → refused.
        let principal = verified_principal("user-a", "https://idp.example.com");
        let error = authorize_silo_selection(&silos, &principals, &tenant("silo-3"), &principal)
            .expect_err("cross-team must be refused");
        match error {
            ConvexTeamAuthzError::CrossTeam {
                silo_team,
                principal_team,
                ..
            } => {
                assert_eq!(silo_team, "team-b");
                assert_eq!(principal_team, "team-a");
            }
            other => panic!("expected CrossTeam, got {other:?}"),
        }
    }

    #[test]
    fn same_team_same_silo_is_admitted() {
        let (silos, principals) = registries();
        let principal = verified_principal("user-a", "https://idp.example.com");
        authorize_silo_selection(&silos, &principals, &tenant("silo-1"), &principal)
            .expect("same-team same-silo must be admitted");
    }

    #[test]
    fn same_team_other_silo_is_admitted_many_silos_per_team() {
        // The non-vacuous case: user-a (team-a) reaches silo-2 (also team-a) —
        // proves many-silos-per-team works (the multi-project-per-tenant capability).
        let (silos, principals) = registries();
        let principal = verified_principal("user-a", "https://idp.example.com");
        authorize_silo_selection(&silos, &principals, &tenant("silo-2"), &principal)
            .expect("same-team other-silo must be admitted (many silos per team)");
    }

    #[test]
    fn unregistered_silo_is_refused() {
        let (silos, principals) = registries();
        let principal = verified_principal("user-a", "https://idp.example.com");
        let error =
            authorize_silo_selection(&silos, &principals, &tenant("silo-unknown"), &principal)
                .expect_err("unregistered silo must be refused");
        assert!(matches!(
            error,
            ConvexTeamAuthzError::UnregisteredSilo { .. }
        ));
    }

    #[test]
    fn unprovisioned_principal_is_refused() {
        let (silos, principals) = registries();
        // Authenticated, but neither subject nor issuer is registered.
        let principal = verified_principal("user-unknown", "https://idp.unknown.com");
        let error = authorize_silo_selection(&silos, &principals, &tenant("silo-1"), &principal)
            .expect_err("unprovisioned principal must be refused");
        assert!(matches!(
            error,
            ConvexTeamAuthzError::PrincipalHasNoTeam { .. }
        ));
    }

    #[test]
    fn principal_resolves_by_issuer_when_subject_unregistered() {
        let silos = SiloTeamRegistry::new().bind(&tenant("silo-1"), team("team-a"));
        // Provision by ISSUER (the per-deployment-IdP→team model).
        let principals =
            PrincipalTeamRegistry::new().bind("https://idp.example.com", team("team-a"));
        let principal = verified_principal("any-user", "https://idp.example.com");
        authorize_silo_selection(&silos, &principals, &tenant("silo-1"), &principal)
            .expect("issuer-keyed principal binding should resolve");
    }

    #[test]
    fn unverified_claims_cannot_name_a_team() {
        let silos = SiloTeamRegistry::new().bind(&tenant("silo-1"), team("team-a"));
        let principals = PrincipalTeamRegistry::new().bind("user-a", team("team-a"));
        // subject only in UNVERIFIED claims → must not resolve.
        let spoofed = PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::from_iter([("subject".to_string(), json!("user-a"))]),
            verified_claims: serde_json::Map::new(),
        };
        let error = authorize_silo_selection(&silos, &principals, &tenant("silo-1"), &spoofed)
            .expect_err("an unverified subject must not resolve a team");
        assert!(matches!(
            error,
            ConvexTeamAuthzError::PrincipalHasNoTeam { .. }
        ));
    }

    #[test]
    fn operator_specs_parse_many_silos_per_team_and_issuer_keys() {
        let silos =
            SiloTeamRegistry::from_operator_spec("silo-1:team-a, silo-2:team-a , silo-3:team-b,")
                .expect("silo spec should parse");
        assert_eq!(
            silos.team_for_silo(&tenant("silo-1")),
            Some(&team("team-a"))
        );
        assert_eq!(
            silos.team_for_silo(&tenant("silo-2")),
            Some(&team("team-a"))
        );
        assert_eq!(
            silos.team_for_silo(&tenant("silo-3")),
            Some(&team("team-b"))
        );
        assert_eq!(silos.team_for_silo(&tenant("silo-x")), None);

        // Issuer keys contain `:` (https://…) — rsplit on the last `:` keeps them whole.
        let principals = PrincipalTeamRegistry::from_operator_spec(
            "https://idp.example.com:team-a, user-b:team-b",
        )
        .expect("principal spec should parse");
        let by_issuer = verified_principal("whoever", "https://idp.example.com");
        assert_eq!(
            principals.team_for_principal(&by_issuer),
            Some(&team("team-a"))
        );
    }

    #[test]
    fn operator_specs_reject_malformed_entries() {
        assert!(SiloTeamRegistry::from_operator_spec("no-colon").is_err());
        assert!(SiloTeamRegistry::from_operator_spec("silo:").is_err());
        assert!(SiloTeamRegistry::from_operator_spec(":team").is_err());
        assert!(PrincipalTeamRegistry::from_operator_spec("no-colon").is_err());
    }
}
