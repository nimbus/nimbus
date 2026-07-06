use std::collections::BTreeSet;
use std::time::Duration;

use nimbus_core::TenantId;
use nimbus_tenant::{
    TenantIsolationContext, TenantIsolationDecision, TenantIsolationPolicyInput,
    WorkloadAttributes, WorkloadLocation,
};
use nimbus_workload_identity::{
    CredentialKind, DenyAllIssuer, IdentityAuditOutcome, IdentityIssueError, IdentityIssuer,
    IdentityMintError, IdentityMintRequest, MintParams, MintedCredential, PolicyValidationError,
    ProviderAuthPolicy, ProviderAuthRule, SubjectMatch, authorize_mint,
};
use serde_json::Value;

const AUDIENCE: &str = "provider://oidc";

fn decision_for_tenant(tenant: &str) -> TenantIsolationDecision {
    let context = TenantIsolationContext::operator(
        TenantId::new(tenant).expect("tenant id should parse"),
        "identity.test",
    );
    context
        .admit_decision(TenantIsolationPolicyInput::new(
            WorkloadAttributes::service("worker"),
        ))
        .expect("test decision should admit")
}

fn decision_for_tenant_with_location(tenant: &str) -> TenantIsolationDecision {
    let context = TenantIsolationContext::operator(
        TenantId::new(tenant).expect("tenant id should parse"),
        "identity.test",
    )
    .with_workload_location(
        WorkloadLocation::new()
            .with_node_id("node-a")
            .with_machine_id("machine-a"),
    );
    context
        .admit_decision(TenantIsolationPolicyInput::new(
            WorkloadAttributes::service("worker")
                .with_sandbox_id("sandbox-a")
                .with_invocation_id("invoke-a"),
        ))
        .expect("test decision should admit")
}

fn rule_for_subject(subject: impl Into<String>) -> ProviderAuthRule {
    ProviderAuthRule::new(
        SubjectMatch::Exact(subject.into()),
        [AUDIENCE],
        Duration::from_secs(60),
    )
}

fn allow_subject(subject: impl Into<String>) -> ProviderAuthPolicy {
    ProviderAuthPolicy::try_new(vec![rule_for_subject(subject)]).expect("policy should validate")
}

fn params(jti: &str) -> MintParams {
    MintParams {
        issued_at_epoch_ms: 1_000,
        credential_instance_id: jti.to_string(),
    }
}

fn authorize(
    policy: &ProviderAuthPolicy,
    decision: &TenantIsolationDecision,
) -> nimbus_workload_identity::MintAuthorization {
    let request = IdentityMintRequest::for_decision(decision, AUDIENCE, Duration::from_secs(30));
    authorize_mint(policy, &request, &params("jti-1"))
}

#[test]
fn forged_foreign_identity_is_denied_and_audited_with_admitted_subject() {
    let alpha = decision_for_tenant("alpha");
    let beta = decision_for_tenant("beta");
    let alpha_subject = alpha.workload_identity().subject();
    let policy = allow_subject(beta.workload_identity().subject());

    let authorization = authorize(&policy, &alpha);

    assert!(matches!(
        &authorization.outcome,
        Err(IdentityMintError::NoMatchingSubjectRule { subject }) if subject == &alpha_subject
    ));
    assert_eq!(authorization.audit.tenant_id(), "alpha");
    assert_eq!(authorization.audit.workload_subject(), alpha_subject);
    assert!(matches!(
        authorization.audit.outcome(),
        IdentityAuditOutcome::Denied { reason } if reason.contains("NoMatchingSubjectRule") || reason.contains("no provider auth rule")
    ));
}

#[test]
fn policy_construction_rejects_unstable_or_invalid_rules() {
    let subject = decision_for_tenant("alpha").workload_identity().subject();

    let invocation_subject = format!("{subject}/invocation/invoke-a");
    assert!(matches!(
        ProviderAuthPolicy::try_new(vec![rule_for_subject(invocation_subject)]),
        Err(PolicyValidationError::PlacementSegmentForbidden {
            segment: "invocation",
            ..
        })
    ));

    let node_subject = format!("{subject}/node/node-a");
    assert!(matches!(
        ProviderAuthPolicy::try_new(vec![rule_for_subject(node_subject)]),
        Err(PolicyValidationError::PlacementSegmentForbidden {
            segment: "node",
            ..
        })
    ));

    assert!(matches!(
        ProviderAuthPolicy::try_new(vec![rule_for_subject(
            "nimbus-workload-audit:v1/tenant/alpha"
        )]),
        Err(PolicyValidationError::AuditProjectionSubject { .. })
    ));

    assert!(matches!(
        ProviderAuthPolicy::try_new(vec![rule_for_subject("other:v1/tenant/alpha")]),
        Err(PolicyValidationError::SubjectPrefixInvalid { .. })
    ));

    assert!(matches!(
        ProviderAuthPolicy::try_new(vec![ProviderAuthRule::new(
            SubjectMatch::Exact(subject.clone()),
            Vec::<String>::new(),
            Duration::from_secs(60),
        )]),
        Err(PolicyValidationError::EmptyAudiences { .. })
    ));

    assert!(matches!(
        ProviderAuthPolicy::try_new(vec![ProviderAuthRule::new(
            SubjectMatch::Exact(subject.clone()),
            [""],
            Duration::from_secs(60),
        )]),
        Err(PolicyValidationError::EmptyAudience { .. })
    ));

    assert!(matches!(
        ProviderAuthPolicy::try_new(vec![ProviderAuthRule::new(
            SubjectMatch::Exact(subject),
            [AUDIENCE],
            Duration::ZERO,
        )]),
        Err(PolicyValidationError::ZeroMaxTtl { .. })
    ));
}

#[test]
fn segment_prefix_matches_only_on_path_boundaries() {
    let acme = decision_for_tenant("acme");
    let acme_corp = decision_for_tenant("acme-corp");
    let policy = ProviderAuthPolicy::try_new(vec![ProviderAuthRule::new(
        SubjectMatch::SegmentPrefix("nimbus-workload:v1/tenant/acme".to_string()),
        [AUDIENCE],
        Duration::from_secs(60),
    )])
    .expect("prefix policy should validate");

    assert!(authorize(&policy, &acme).outcome.is_ok());
    assert!(matches!(
        authorize(&policy, &acme_corp).outcome,
        Err(IdentityMintError::NoMatchingSubjectRule { .. })
    ));
}

#[test]
fn audience_allow_list_mints_only_listed_audiences() {
    let decision = decision_for_tenant("alpha");
    let policy = allow_subject(decision.workload_identity().subject());
    let allowed = IdentityMintRequest::for_decision(&decision, AUDIENCE, Duration::from_secs(30));
    let denied =
        IdentityMintRequest::for_decision(&decision, "provider://other", Duration::from_secs(30));

    assert!(
        authorize_mint(&policy, &allowed, &params("jti-1"))
            .outcome
            .is_ok()
    );
    assert!(matches!(
        authorize_mint(&policy, &denied, &params("jti-2")).outcome,
        Err(IdentityMintError::AudienceNotAllowed { .. })
    ));
}

#[test]
fn ttl_clamps_denies_zero_and_saturates_expiration() {
    let decision = decision_for_tenant("alpha");
    let subject = decision.workload_identity().subject();
    let policy = ProviderAuthPolicy::try_new(vec![ProviderAuthRule::new(
        SubjectMatch::Exact(subject.clone()),
        [AUDIENCE],
        Duration::from_secs(10),
    )])
    .expect("policy should validate");
    let request = IdentityMintRequest::for_decision(&decision, AUDIENCE, Duration::from_secs(60));
    let minted = authorize_mint(
        &policy,
        &request,
        &MintParams {
            issued_at_epoch_ms: 2_000,
            credential_instance_id: "jti-1".to_string(),
        },
    )
    .outcome
    .expect("allowed request should mint");
    assert_eq!(minted.exp_epoch_ms(), 12_000);

    let zero_ttl = IdentityMintRequest::for_decision(&decision, AUDIENCE, Duration::ZERO);
    assert_eq!(
        authorize_mint(&policy, &zero_ttl, &params("jti-2")).outcome,
        Err(IdentityMintError::TtlInvalid)
    );

    let saturating_policy = ProviderAuthPolicy::try_new(vec![ProviderAuthRule::new(
        SubjectMatch::Exact(subject),
        [AUDIENCE],
        Duration::from_millis(u64::MAX),
    )])
    .expect("large TTL policy should validate");
    let saturating_request =
        IdentityMintRequest::for_decision(&decision, AUDIENCE, Duration::from_millis(u64::MAX));
    let saturating_claims = authorize_mint(
        &saturating_policy,
        &saturating_request,
        &MintParams {
            issued_at_epoch_ms: u64::MAX - 10,
            credential_instance_id: "jti-3".to_string(),
        },
    )
    .outcome
    .expect("allowed request should mint");
    assert_eq!(saturating_claims.exp_epoch_ms(), u64::MAX);
}

#[test]
fn claims_serialize_with_exact_identity_contract_keys() {
    let decision = decision_for_tenant("alpha");
    let policy = allow_subject(decision.workload_identity().subject());
    let request =
        IdentityMintRequest::for_decision(&decision, AUDIENCE, Duration::from_millis(250));
    let claims = authorize_mint(
        &policy,
        &request,
        &MintParams {
            issued_at_epoch_ms: 1_000,
            credential_instance_id: "jti-claims".to_string(),
        },
    )
    .outcome
    .expect("allowed request should mint");
    let value = serde_json::to_value(&claims).expect("claims should serialize");
    let object = value
        .as_object()
        .expect("claims should serialize as object");
    let keys = object.keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(
        keys,
        BTreeSet::from([
            "aud".to_string(),
            "exp".to_string(),
            "jti".to_string(),
            "nimbus_decision_id".to_string(),
            "nimbus_invocation_id".to_string(),
            "nimbus_machine_id".to_string(),
            "nimbus_node_id".to_string(),
            "nimbus_sandbox_id".to_string(),
            "nimbus_workload_audit_projection".to_string(),
            "nimbus_workload_subject".to_string(),
            "sub".to_string(),
        ])
    );
    assert_eq!(value["sub"], value["nimbus_workload_subject"]);
    assert_eq!(value["aud"], AUDIENCE);
    assert_eq!(value["exp"], 1_250);
    assert_eq!(value["jti"], "jti-claims");
    assert_eq!(value["nimbus_decision_id"], decision.id().as_str());
    assert_eq!(
        value["nimbus_workload_audit_projection"],
        decision.workload_identity().audit_projection()
    );
    assert!(value["nimbus_node_id"].is_null());
    assert!(value["nimbus_machine_id"].is_null());
    assert!(value["nimbus_sandbox_id"].is_null());
    assert!(value["nimbus_invocation_id"].is_null());
}

#[test]
fn deny_by_default_policies_deny_valid_admitted_identity() {
    let decision = decision_for_tenant("alpha");
    let empty_policy = ProviderAuthPolicy::try_new(Vec::new()).expect("empty policy is valid");

    for policy in [ProviderAuthPolicy::deny_all(), empty_policy] {
        assert!(matches!(
            authorize(&policy, &decision).outcome,
            Err(IdentityMintError::NoMatchingSubjectRule { .. })
        ));
    }
}

#[test]
fn audit_events_are_unskippable_and_never_carry_secret_material() {
    let decision = decision_for_tenant_with_location("alpha");
    let subject = decision.workload_identity().subject();
    let policy = allow_subject(subject);
    let minted = authorize(&policy, &decision);
    assert!(minted.outcome.is_ok());
    assert_audit_has_no_secret_material(&minted.audit, "top-secret-token");

    let deny_subject = authorize(&ProviderAuthPolicy::deny_all(), &decision);
    assert!(matches!(
        deny_subject.outcome,
        Err(IdentityMintError::NoMatchingSubjectRule { .. })
    ));
    assert_audit_has_no_secret_material(&deny_subject.audit, "top-secret-token");

    let wrong_audience =
        IdentityMintRequest::for_decision(&decision, "provider://other", Duration::from_secs(30));
    let deny_audience = authorize_mint(&policy, &wrong_audience, &params("jti-deny-audience"));
    assert!(matches!(
        deny_audience.outcome,
        Err(IdentityMintError::AudienceNotAllowed { .. })
    ));
    assert_audit_has_no_secret_material(&deny_audience.audit, "top-secret-token");

    let zero_ttl = IdentityMintRequest::for_decision(&decision, AUDIENCE, Duration::ZERO);
    let deny_ttl = authorize_mint(&policy, &zero_ttl, &params("jti-deny-ttl"));
    assert_eq!(deny_ttl.outcome, Err(IdentityMintError::TtlInvalid));
    assert_audit_has_no_secret_material(&deny_ttl.audit, "top-secret-token");

    let invalid_params =
        IdentityMintRequest::for_decision(&decision, AUDIENCE, Duration::from_secs(30));
    let deny_params = authorize_mint(
        &policy,
        &invalid_params,
        &MintParams {
            issued_at_epoch_ms: 1_000,
            credential_instance_id: String::new(),
        },
    );
    assert!(matches!(
        deny_params.outcome,
        Err(IdentityMintError::InvalidParams { .. })
    ));
    assert_audit_has_no_secret_material(&deny_params.audit, "top-secret-token");

    let credential = MintedCredential::new(CredentialKind::OidcJwt, "top-secret-token");
    let debug = format!("{credential:?}");
    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains("top-secret-token"));
    assert_eq!(credential.secret(), "top-secret-token");
}

#[test]
fn deny_all_issuer_returns_issuance_not_configured() {
    let decision = decision_for_tenant("alpha");
    let policy = allow_subject(decision.workload_identity().subject());
    let claims = authorize(&policy, &decision)
        .outcome
        .expect("allowed request should mint claims");

    assert!(matches!(
        DenyAllIssuer.mint(&claims),
        Err(IdentityIssueError::IssuanceNotConfigured)
    ));
}

fn assert_audit_has_no_secret_material(
    audit: &nimbus_workload_identity::IdentityAuditEvent,
    forbidden_secret: &str,
) {
    let value = serde_json::to_value(audit).expect("audit should serialize");
    assert_no_secret_keys(&value);
    let serialized = serde_json::to_string(&value).expect("audit should serialize to JSON");
    assert!(
        !serialized.contains(forbidden_secret),
        "audit event leaked secret material: {serialized}"
    );
}

fn assert_no_secret_keys(value: &Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                assert!(
                    !matches!(
                        key.as_str(),
                        "secret"
                            | "token"
                            | "credential"
                            | "secret_value"
                            | "token_value"
                            | "credential_value"
                    ),
                    "audit event contains a secret-bearing key: {key}"
                );
                assert_no_secret_keys(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                assert_no_secret_keys(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}
