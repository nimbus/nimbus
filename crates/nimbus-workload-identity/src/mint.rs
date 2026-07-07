use std::time::Duration;

use nimbus_tenant::{TenantIsolationDecision, TenantIsolationDecisionId, WorkloadIdentity};

use crate::audit::IdentityAuditEventParts;
use crate::{
    CredentialClaims, IdentityAuditEvent, IdentityAuditOutcome, IdentityIssueError, IdentityIssuer,
    MintedCredential, ProviderAuthPolicy, ProviderAuthRule,
};

/// Admission-anchored mint request.
pub struct IdentityMintRequest<'a> {
    identity: WorkloadIdentity,
    decision_id: &'a TenantIsolationDecisionId,
    identity_grants: Vec<String>,
    audience: String,
    requested_ttl: Duration,
}

impl<'a> IdentityMintRequest<'a> {
    pub fn for_decision(
        decision: &'a TenantIsolationDecision,
        audience: impl Into<String>,
        requested_ttl: Duration,
    ) -> Self {
        let mut identity_grants = decision.runtime().grants().identity.clone();
        identity_grants.sort();
        identity_grants.dedup();
        Self {
            identity: decision.workload_identity(),
            decision_id: decision.id(),
            identity_grants,
            audience: audience.into(),
            requested_ttl,
        }
    }
}

pub struct MintParams {
    pub issued_at_epoch_ms: u64,
    pub credential_instance_id: String,
}

pub fn authorize_mint(
    policy: &ProviderAuthPolicy,
    request: &IdentityMintRequest<'_>,
    params: &MintParams,
) -> MintAuthorization {
    let outcome = authorize_claims(policy, request, params);
    let audit = audit_event(request, &outcome);
    MintAuthorization { outcome, audit }
}

pub struct MintAuthorization {
    pub outcome: Result<CredentialClaims, IdentityMintError>,
    pub audit: IdentityAuditEvent,
}

pub struct CredentialMint {
    pub outcome: Result<MintedCredential, CredentialMintError>,
    pub audit: IdentityAuditEvent,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CredentialMintError {
    #[error(transparent)]
    Authorization(IdentityMintError),
    #[error(transparent)]
    Issuance(IdentityIssueError),
}

pub fn mint_credential(
    policy: &ProviderAuthPolicy,
    request: &IdentityMintRequest<'_>,
    params: &MintParams,
    issuer: &dyn IdentityIssuer,
) -> CredentialMint {
    let MintAuthorization { outcome, audit } = authorize_mint(policy, request, params);
    let claims = match outcome {
        Ok(claims) => claims,
        Err(error) => {
            return CredentialMint {
                outcome: Err(CredentialMintError::Authorization(error)),
                audit,
            };
        }
    };
    match issuer.mint(&claims) {
        Ok(credential) => CredentialMint {
            outcome: Ok(credential),
            audit,
        },
        Err(error) => CredentialMint {
            audit: audit_issuance_failure(request, &claims, &error),
            outcome: Err(CredentialMintError::Issuance(error)),
        },
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdentityMintError {
    #[error("workload has no identity grant; identity minting requires an explicit identity grant")]
    IdentityGrantMissing,
    #[error("no provider auth rule matched workload subject `{subject}`")]
    NoMatchingSubjectRule { subject: String },
    #[error(
        "provider auth policy does not allow audience `{audience}` for workload subject `{subject}`"
    )]
    AudienceNotAllowed { subject: String, audience: String },
    #[error("requested TTL must be positive")]
    TtlInvalid,
    #[error("invalid mint params: {reason}")]
    InvalidParams { reason: String },
}

fn authorize_claims(
    policy: &ProviderAuthPolicy,
    request: &IdentityMintRequest<'_>,
    params: &MintParams,
) -> Result<CredentialClaims, IdentityMintError> {
    if request.identity_grants.is_empty() {
        return Err(IdentityMintError::IdentityGrantMissing);
    }

    let subject = request.identity.subject();
    let matching_subject_rules = policy
        .rules()
        .iter()
        .filter(|rule| rule.subject().matches(&subject));
    let Some(rule) = select_audience_rule(matching_subject_rules, &request.audience) else {
        if policy
            .rules()
            .iter()
            .any(|rule| rule.subject().matches(&subject))
        {
            return Err(IdentityMintError::AudienceNotAllowed {
                subject,
                audience: request.audience.clone(),
            });
        }
        return Err(IdentityMintError::NoMatchingSubjectRule { subject });
    };
    if request.requested_ttl.is_zero() {
        return Err(IdentityMintError::TtlInvalid);
    }
    if params.credential_instance_id.is_empty() {
        return Err(IdentityMintError::InvalidParams {
            reason: "credential instance id must be non-empty".to_string(),
        });
    }
    let effective_ttl = request.requested_ttl.min(rule.max_ttl());
    let exp_epoch_ms = params
        .issued_at_epoch_ms
        .saturating_add(duration_millis_saturating(effective_ttl));
    Ok(CredentialClaims::new(
        &request.identity,
        request.decision_id,
        request.audience.clone(),
        params.issued_at_epoch_ms,
        exp_epoch_ms,
        params.credential_instance_id.clone(),
    ))
}

fn select_audience_rule<'a>(
    rules: impl IntoIterator<Item = &'a ProviderAuthRule>,
    audience: &str,
) -> Option<&'a ProviderAuthRule> {
    rules
        .into_iter()
        .find(|rule| rule.audiences().iter().any(|allowed| allowed == audience))
}

fn duration_millis_saturating(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn audit_event(
    request: &IdentityMintRequest<'_>,
    outcome: &Result<CredentialClaims, IdentityMintError>,
) -> IdentityAuditEvent {
    let (audit_outcome, exp_epoch_ms, credential_instance_id) = match outcome {
        Ok(claims) => (
            IdentityAuditOutcome::Minted,
            Some(claims.exp_epoch_ms()),
            Some(claims.jti().to_string()),
        ),
        Err(error) => (
            IdentityAuditOutcome::Denied {
                reason: error.to_string(),
            },
            None,
            None,
        ),
    };
    IdentityAuditEvent::from_parts(IdentityAuditEventParts {
        tenant_id: request.identity.tenant_id().to_string(),
        decision_id: request.decision_id.as_str().to_string(),
        workload_subject: request.identity.subject(),
        workload_audit_projection: request.identity.audit_projection(),
        identity_grants: request.identity_grants.clone(),
        audience: request.audience.clone(),
        outcome: audit_outcome,
        exp_epoch_ms,
        credential_instance_id,
    })
}

fn audit_issuance_failure(
    request: &IdentityMintRequest<'_>,
    claims: &CredentialClaims,
    error: &IdentityIssueError,
) -> IdentityAuditEvent {
    IdentityAuditEvent::from_parts(IdentityAuditEventParts {
        tenant_id: request.identity.tenant_id().to_string(),
        decision_id: request.decision_id.as_str().to_string(),
        workload_subject: request.identity.subject(),
        workload_audit_projection: request.identity.audit_projection(),
        identity_grants: request.identity_grants.clone(),
        audience: request.audience.clone(),
        outcome: IdentityAuditOutcome::Denied {
            reason: format!("issuance failed: {error}"),
        },
        exp_epoch_ms: Some(claims.exp_epoch_ms()),
        credential_instance_id: Some(claims.jti().to_string()),
    })
}
