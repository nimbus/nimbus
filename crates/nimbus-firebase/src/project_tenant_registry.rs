//! Firebase project → Nimbus tenant binding for the Firestore adapter (#24).
//!
//! **The contract this restores.** A tenant boundary is only as strong as
//! whatever *decides* the tenant. The trustworthy model — first implemented by
//! the DynamoDB adapter's `AccessKeyRegistry` (`crates/nimbus-dynamodb/src/
//! tenant.rs`) and mirrored by the MongoDB adapter's `CredentialRegistry` —
//! binds authentication to a tenant so that **authentication alone fixes the
//! tenant and no request-supplied field can broaden it**. A wire-supplied
//! namespace token (a DynamoDB key prefix, a MongoDB `$db`, a Firestore URL
//! `project_id`) may then only *select within* the already-authenticated
//! tenant's scope, never widen it.
//!
//! **The deviation this closes (#24).** Before this change the Firestore adapter
//! derived the tenant verbatim from the URL `project_id`
//! (`TenantId::new(project_id)`) and admitted a principal with no tenant claim —
//! including a fully anonymous (no-token) caller — to whatever project it named.
//! That let an unverified caller *select* the tenant partition. The Firestore
//! analogue of the credential→tenant binding is **the verified token's project**
//! (carried in the Firebase ID token issuer, `securetoken.google.com/<project>`,
//! which survives verification), resolved through this registry to a tenant.
//!
//! [`ProjectTenantRegistry`] maps each Firebase `project_id` to a [`TenantId`].
//! It is **strict by default**: a [`ProjectTenantRegistry::new`] registry
//! resolves nothing, and an unregistered project is refused (not admitted). The
//! 1:1 case (each project is its own tenant) is a strict registry whose rows all
//! map a project to a same-named tenant; the many-projects-per-tenant case is
//! several project rows mapping to one tenant — that is what lets a token minted
//! for project X reach project Y when both belong to the same tenant.
//!
//! [`ProjectTenantRegistry::identity`] is the **dev-only** mode: every project
//! resolves to a same-named tenant. It is wired only behind the Firebase
//! emulator-mock opt-in so `nimbus dev` stays zero-config; it does **not** relax
//! verification (an anonymous caller still has no verified project and is still
//! refused), it only relaxes the project→tenant *mapping* to today's 1:1.

use std::collections::BTreeMap;
use std::fmt;

use nimbus_core::{Error, PrincipalContext, Result, TenantId};

/// Tenants whose id begins with this prefix are Nimbus-internal. A Firebase
/// project must never resolve to one, or an authenticated request could reach an
/// internal store. nimbus-core exposes no shared reserved-tenant check today, so
/// the prefix is defined locally per adapter, mirroring the DynamoDB
/// `AccessKeyRegistry` and MongoDB `CredentialRegistry` `RESERVED_TENANT_PREFIX`.
pub(crate) const RESERVED_TENANT_PREFIX: &str = "_nimbus";

/// Whether `tenant` is a reserved Nimbus-internal tenant (see
/// [`RESERVED_TENANT_PREFIX`]).
#[must_use]
pub(crate) fn is_reserved_tenant(tenant: &TenantId) -> bool {
    tenant.as_str().starts_with(RESERVED_TENANT_PREFIX)
}

/// The Firebase ID token issuer host whose final path segment is the project id.
/// A genuine Firebase ID token carries `iss = https://securetoken.google.com/
/// <project-id>`; the issuer survives token verification while `aud` does not, so
/// it is the only reliably-verified project signal (see #24 research).
const FIREBASE_SECURETOKEN_ISSUER_HOST: &str = "securetoken.google.com";

/// Extract the Firebase project id from a token issuer string.
///
/// Returns `Some(project)` only for a genuine Firebase issuer
/// (`securetoken.google.com/<project>`) whose single trailing path segment is a
/// non-empty project id. Returns `None` for any other issuer — an Auth0/Clerk/
/// custom-JWT issuer carries no Firebase project, and a non-Firebase token has
/// no business selecting a Firestore project, so it must be refused rather than
/// guessed.
#[must_use]
pub fn firebase_project_from_issuer(issuer: &str) -> Option<String> {
    let rest = issuer
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let rest = rest.strip_prefix(FIREBASE_SECURETOKEN_ISSUER_HOST)?;
    let project = rest.strip_prefix('/')?;
    // The project must be exactly one non-empty path segment: nothing after it,
    // no query/fragment smuggled in.
    if project.is_empty() || project.contains(['/', '?', '#']) {
        return None;
    }
    Some(project.to_string())
}

/// Extract the Firebase project a principal is **verified** to hold, from its
/// verified token issuer.
///
/// Reads only `verified_claims` (the cryptographically verified identity
/// claims), never the unverified `claims` map — a caller must not be able to
/// name a project by stuffing an `issuer` into an unverified claim. An anonymous
/// (no-token) principal has empty `verified_claims` and yields `None`, so it has
/// no verified project and cannot select a tenant.
///
/// The verified issuer is recorded under the `issuer` key by
/// `nimbus_auth::normalize_principal_context` (the `VerifiedUserIdentity`
/// `issuer` field, camelCase-serialized to `issuer`).
#[must_use]
pub fn firebase_project_from_verified_principal(principal: &PrincipalContext) -> Option<String> {
    let issuer = principal.verified_claims.get("issuer")?.as_str()?;
    firebase_project_from_issuer(issuer)
}

#[derive(Debug, Clone)]
enum Mode {
    /// Production: only registered projects resolve; everything else is refused.
    Strict(BTreeMap<String, TenantId>),
    /// Dev-only (behind the emulator-mock opt-in): every project resolves to a
    /// same-named tenant. Does not relax verification, only the mapping.
    Identity,
}

/// Configured bindings from Firebase `project_id` to Nimbus [`TenantId`].
///
/// Strict by default: [`ProjectTenantRegistry::new`] resolves nothing. Use
/// [`ProjectTenantRegistry::bind`] / [`ProjectTenantRegistry::from_operator_spec`]
/// to register projects, or [`ProjectTenantRegistry::identity`] for the dev-only
/// 1:1 mode.
#[derive(Debug, Clone)]
pub struct ProjectTenantRegistry {
    mode: Mode,
}

impl Default for ProjectTenantRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectTenantRegistry {
    /// An empty strict registry. Refuses every project until one is bound — the
    /// fail-closed default for a deployment with no project bindings configured.
    #[must_use]
    pub fn new() -> Self {
        Self {
            mode: Mode::Strict(BTreeMap::new()),
        }
    }

    /// The dev-only identity registry: every project resolves to a same-named
    /// tenant (today's 1:1 semantics). Wired only behind the Firebase
    /// emulator-mock opt-in; it does not relax verification.
    #[must_use]
    pub fn identity() -> Self {
        Self {
            mode: Mode::Identity,
        }
    }

    /// Bind a Firebase `project_id` to a tenant. Builder style. No-op semantics
    /// on the identity registry are disallowed: binding is only meaningful in
    /// strict mode, so a bind converts an identity registry into a strict one
    /// seeded with this single row (callers build strict registries explicitly,
    /// so this path is exercised only in tests).
    #[must_use]
    pub fn bind(mut self, project_id: impl Into<String>, tenant: TenantId) -> Self {
        match &mut self.mode {
            Mode::Strict(bindings) => {
                bindings.insert(project_id.into(), tenant);
            }
            Mode::Identity => {
                let mut bindings = BTreeMap::new();
                bindings.insert(project_id.into(), tenant);
                self.mode = Mode::Strict(bindings);
            }
        }
        self
    }

    /// Resolve a Firebase `project_id` to its tenant.
    ///
    /// # Errors
    /// A [`Error::PermissionDenied`] if the project is not registered (strict
    /// mode), or if it would resolve to a reserved Nimbus-internal tenant — such
    /// a resolution is refused so it can never expose an internal store,
    /// regardless of how it was configured. In identity mode an invalid project
    /// id (one that is not a valid tenant id) is likewise refused.
    pub fn resolve(&self, project_id: &str) -> Result<TenantId> {
        let tenant = match &self.mode {
            Mode::Strict(bindings) => bindings.get(project_id).cloned().ok_or_else(|| {
                Error::PermissionDenied(format!(
                    "Firebase project `{project_id}` is not registered to a tenant; the request \
                     cannot select an unregistered project"
                ))
            })?,
            Mode::Identity => TenantId::new(project_id).map_err(|error| {
                Error::PermissionDenied(format!(
                    "Firebase project `{project_id}` is not a valid tenant id: {error}"
                ))
            })?,
        };
        if is_reserved_tenant(&tenant) {
            return Err(Error::PermissionDenied(format!(
                "Firebase project `{project_id}` resolves to reserved Nimbus-internal tenant \
                 `{tenant}`; refused"
            )));
        }
        Ok(tenant)
    }

    /// Whether this is the fail-closed empty strict registry (no projects bound,
    /// not identity mode). Used by the wiring layer to warn an operator that a
    /// configured Firestore surface will refuse every request until projects are
    /// registered.
    #[must_use]
    pub fn is_strict_empty(&self) -> bool {
        matches!(&self.mode, Mode::Strict(bindings) if bindings.is_empty())
    }

    /// Parse an operator project spec (the `NIMBUS_FIREBASE_PROJECTS` value) into
    /// a strict registry.
    ///
    /// The format mirrors the MongoDB `NIMBUS_MONGODB_CREDENTIALS` /
    /// DynamoDB `NIMBUS_DYNAMODB_ACCESS_KEYS` conventions: comma-separated
    /// entries, each `PROJECT:TENANT`. Surrounding whitespace on each entry is
    /// trimmed and empty entries are skipped (so a stray or trailing comma is
    /// harmless). A non-empty entry that is not two colon-separated segments, has
    /// an empty segment, names an invalid tenant id, or names a reserved
    /// Nimbus-internal tenant is a hard error so the operator sees a clean
    /// refusal at boot rather than a silent refusal later. Several projects may
    /// map to the same tenant (many-projects-per-tenant).
    ///
    /// # Errors
    /// A [`ProjectSpecError`] naming the offending entry and the expected format.
    pub fn from_operator_spec(spec: &str) -> std::result::Result<Self, ProjectSpecError> {
        let mut bindings = BTreeMap::new();
        for entry in spec
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            let mut parts = entry.splitn(2, ':');
            let (Some(project), Some(tenant)) = (parts.next(), parts.next()) else {
                return Err(spec_error(format!(
                    "invalid Firebase project binding `{entry}`: expected PROJECT:TENANT"
                )));
            };
            if project.is_empty() || tenant.is_empty() {
                return Err(spec_error(format!(
                    "invalid Firebase project binding `{entry}`: every segment must be non-empty"
                )));
            }
            if tenant.contains(':') {
                return Err(spec_error(format!(
                    "invalid Firebase project binding `{entry}`: tenant id may not contain `:`"
                )));
            }
            let tenant_id = TenantId::new(tenant).map_err(|error| {
                spec_error(format!(
                    "invalid Firebase project binding `{entry}`: {error}"
                ))
            })?;
            if is_reserved_tenant(&tenant_id) {
                return Err(spec_error(format!(
                    "invalid Firebase project binding `{entry}`: tenant `{tenant}` is reserved for \
                     Nimbus-internal use"
                )));
            }
            bindings.insert(project.to_string(), tenant_id);
        }
        Ok(Self {
            mode: Mode::Strict(bindings),
        })
    }
}

/// Failure parsing an operator project spec (see
/// [`ProjectTenantRegistry::from_operator_spec`]). Carries an operator-facing
/// message that names the offending entry and the expected `PROJECT:TENANT`
/// format, mirroring the MongoDB/DynamoDB spec errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSpecError {
    message: String,
}

impl ProjectSpecError {
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProjectSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProjectSpecError {}

fn spec_error(message: String) -> ProjectSpecError {
    ProjectSpecError { message }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tenant(id: &str) -> TenantId {
        TenantId::new(id).expect("tenant id should parse")
    }

    fn verified_principal_with_issuer(issuer: &str) -> PrincipalContext {
        PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::new(),
            verified_claims: serde_json::Map::from_iter([("issuer".to_string(), json!(issuer))]),
        }
    }

    #[test]
    fn issuer_parse_extracts_project_from_genuine_firebase_issuer() {
        assert_eq!(
            firebase_project_from_issuer("https://securetoken.google.com/my-project"),
            Some("my-project".to_string())
        );
        // Trailing slash trimmed by normalization upstream, but tolerate it here.
        assert_eq!(
            firebase_project_from_issuer("securetoken.google.com/my-project"),
            Some("my-project".to_string())
        );
    }

    #[test]
    fn issuer_parse_rejects_non_firebase_and_malformed_issuers() {
        // Auth0/Clerk/custom-JWT issuers carry no Firebase project.
        assert_eq!(
            firebase_project_from_issuer("https://acme.auth0.com/"),
            None
        );
        assert_eq!(
            firebase_project_from_issuer("https://clerk.example.com"),
            None
        );
        // securetoken host but no project segment.
        assert_eq!(
            firebase_project_from_issuer("https://securetoken.google.com"),
            None
        );
        assert_eq!(
            firebase_project_from_issuer("https://securetoken.google.com/"),
            None
        );
        // Extra path segments are not a single project id.
        assert_eq!(
            firebase_project_from_issuer("https://securetoken.google.com/proj/extra"),
            None
        );
        // A look-alike host must not match.
        assert_eq!(
            firebase_project_from_issuer("https://securetoken.google.com.evil.test/proj"),
            None
        );
    }

    #[test]
    fn verified_principal_project_reads_only_verified_claims() {
        // Verified issuer present -> project derived.
        let principal = verified_principal_with_issuer("https://securetoken.google.com/proj-a");
        assert_eq!(
            firebase_project_from_verified_principal(&principal),
            Some("proj-a".to_string())
        );

        // Anonymous (no token) -> empty verified_claims -> no project.
        assert_eq!(
            firebase_project_from_verified_principal(&PrincipalContext::anonymous()),
            None
        );

        // An issuer stuffed into UNVERIFIED claims must be ignored.
        let spoofed = PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::from_iter([(
                "issuer".to_string(),
                json!("https://securetoken.google.com/proj-a"),
            )]),
            verified_claims: serde_json::Map::new(),
        };
        assert_eq!(firebase_project_from_verified_principal(&spoofed), None);
    }

    #[test]
    fn strict_registry_resolves_registered_and_refuses_unregistered() {
        let registry = ProjectTenantRegistry::new()
            .bind("proj-a", tenant("tenant-1"))
            .bind("proj-b", tenant("tenant-1"))
            .bind("proj-c", tenant("tenant-2"));

        // Registered projects resolve.
        assert_eq!(registry.resolve("proj-a").unwrap(), tenant("tenant-1"));
        assert_eq!(registry.resolve("proj-b").unwrap(), tenant("tenant-1"));
        assert_eq!(registry.resolve("proj-c").unwrap(), tenant("tenant-2"));

        // Unregistered project is refused, not admitted.
        let error = registry
            .resolve("proj-unknown")
            .expect_err("unregistered project must be refused");
        assert!(matches!(error, Error::PermissionDenied(_)), "got {error:?}");
        assert!(!registry.is_strict_empty());
    }

    #[test]
    fn empty_strict_registry_refuses_everything_and_reports_empty() {
        let registry = ProjectTenantRegistry::new();
        assert!(registry.is_strict_empty());
        assert!(matches!(
            registry.resolve("any-project"),
            Err(Error::PermissionDenied(_))
        ));
    }

    #[test]
    fn identity_registry_maps_project_to_same_named_tenant() {
        let registry = ProjectTenantRegistry::identity();
        assert_eq!(registry.resolve("proj-a").unwrap(), tenant("proj-a"));
        assert!(!registry.is_strict_empty());
    }

    #[test]
    fn reserved_tenant_is_refused_in_both_modes() {
        let strict = ProjectTenantRegistry::new().bind("proj", tenant("_nimbus_internal"));
        assert!(matches!(
            strict.resolve("proj"),
            Err(Error::PermissionDenied(_))
        ));

        let identity = ProjectTenantRegistry::identity();
        assert!(matches!(
            identity.resolve("_nimbus_internal"),
            Err(Error::PermissionDenied(_))
        ));
    }

    #[test]
    fn operator_spec_parses_many_projects_per_tenant() {
        let registry = ProjectTenantRegistry::from_operator_spec(
            "proj-a:tenant-1, proj-b:tenant-1 , proj-c:tenant-2,",
        )
        .expect("valid spec should parse");
        assert_eq!(registry.resolve("proj-a").unwrap(), tenant("tenant-1"));
        assert_eq!(registry.resolve("proj-b").unwrap(), tenant("tenant-1"));
        assert_eq!(registry.resolve("proj-c").unwrap(), tenant("tenant-2"));
        assert!(matches!(
            registry.resolve("proj-d"),
            Err(Error::PermissionDenied(_))
        ));
    }

    #[test]
    fn operator_spec_rejects_malformed_entries() {
        assert!(ProjectTenantRegistry::from_operator_spec("just-a-project").is_err());
        assert!(ProjectTenantRegistry::from_operator_spec("proj:").is_err());
        assert!(ProjectTenantRegistry::from_operator_spec(":tenant").is_err());
        assert!(ProjectTenantRegistry::from_operator_spec("proj:_nimbus_internal").is_err());
    }

    #[test]
    fn empty_spec_is_fail_closed_empty_registry() {
        let registry =
            ProjectTenantRegistry::from_operator_spec("  ,  ").expect("blank spec parses");
        assert!(registry.is_strict_empty());
    }
}
