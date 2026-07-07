use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use nimbus_core::TenantId;
use nimbus_crypto::{
    FileBackedIdentitySigner, IdentityPublicKey, IdentitySignature, IdentitySigner,
    IdentitySignerKind, OpenMode, SigningError, SigningResult,
};
use nimbus_runtime::{RuntimeGrants, RuntimeLimits, RuntimePolicy};
use nimbus_tenant::{
    RuntimeIsolationTier, TenantIsolationContext, TenantIsolationDecision, TenantIsolationMode,
    TenantIsolationPolicyInput, WorkloadAttributes, WorkloadLocation,
};
use nimbus_workload_identity::{
    CredentialKind, CredentialMintError, DenyAllIssuer, IdentityAuditOutcome, IdentityIssueError,
    IdentityIssuer, IdentityMintError, IdentityMintRequest, IdentityTrustConfig, LocalDevIssuer,
    MintParams, MintedCredential, NodeIdentityRecord, PolicyValidationError, ProviderAuthPolicy,
    ProviderAuthRule, SubjectMatch, TrustConfigError, authorize_mint, mint_credential,
};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde_json::Value;

const AUDIENCE: &str = "provider://oidc";

fn decision_for_tenant(tenant: &str) -> TenantIsolationDecision {
    decision_for_tenant_with_identity_grants(tenant, ["service:test"])
}

fn decision_for_tenant_with_identity_grants(
    tenant: &str,
    identity_grants: impl IntoIterator<Item = impl Into<String>>,
) -> TenantIsolationDecision {
    let context = TenantIsolationContext::operator(
        TenantId::new(tenant).expect("tenant id should parse"),
        "identity.test",
    );
    context
        .admit_decision(policy_input_with_identity_grants(
            &context,
            WorkloadAttributes::service("worker"),
            identity_grants,
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
        .admit_decision(policy_input_with_identity_grants(
            &context,
            WorkloadAttributes::service("worker")
                .with_sandbox_id("sandbox-a")
                .with_invocation_id("invoke-a"),
            ["service:test"],
        ))
        .expect("test decision should admit")
}

fn policy_input_with_identity_grants(
    context: &TenantIsolationContext,
    attributes: WorkloadAttributes,
    identity_grants: impl IntoIterator<Item = impl Into<String>>,
) -> TenantIsolationPolicyInput {
    let runtime_policy = RuntimePolicy::new(RuntimeLimits {
        grants: RuntimeGrants {
            identity: identity_grants.into_iter().map(Into::into).collect(),
            ..RuntimeGrants::application_node_production_in_process()
        },
        ..RuntimeLimits::application_node22()
    });
    TenantIsolationPolicyInput::new(attributes).with_runtime_policy(
        context,
        &runtime_policy,
        RuntimeIsolationTier::InProcessUntrusted,
        TenantIsolationMode::Production,
    )
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
fn mint_denies_without_identity_grant() {
    let decision = decision_for_tenant_with_identity_grants("alpha", Vec::<String>::new());
    let subject = decision.workload_identity().subject();
    let policy = allow_subject(subject.clone());

    let authorization = authorize(&policy, &decision);

    assert_eq!(
        authorization.outcome,
        Err(IdentityMintError::IdentityGrantMissing)
    );
    assert!(authorization.audit.identity_grants().is_empty());
    assert!(matches!(
        authorization.audit.outcome(),
        IdentityAuditOutcome::Denied { reason }
            if reason == "workload has no identity grant; identity minting requires an explicit identity grant"
                && !reason.contains(&subject)
                && !reason.contains(AUDIENCE)
    ));
}

#[test]
fn grant_check_precedes_policy_matching() {
    let decision = decision_for_tenant_with_identity_grants("alpha", Vec::<String>::new());
    let policy = ProviderAuthPolicy::try_new(vec![ProviderAuthRule::new(
        SubjectMatch::Exact("nimbus-workload:v1/tenant/beta/workload/service/worker".to_string()),
        ["provider://other"],
        Duration::from_secs(60),
    )])
    .expect("policy should validate");

    let authorization = authorize(&policy, &decision);

    assert_eq!(
        authorization.outcome,
        Err(IdentityMintError::IdentityGrantMissing)
    );
}

#[test]
fn mint_succeeds_with_identity_grant_and_records_grants_in_audit() {
    let decision =
        decision_for_tenant_with_identity_grants("alpha", ["service:z", "service:a", "service:z"]);
    let policy = allow_subject(decision.workload_identity().subject());

    let authorization = authorize(&policy, &decision);

    assert!(authorization.outcome.is_ok());
    assert_eq!(
        authorization.audit.identity_grants(),
        ["service:a".to_string(), "service:z".to_string()].as_slice()
    );
    assert!(matches!(
        authorization.audit.outcome(),
        IdentityAuditOutcome::Minted
    ));
}

#[test]
fn audit_json_includes_identity_grants_on_mint_and_deny_without_secret_keys() {
    let mint_decision = decision_for_tenant_with_identity_grants("alpha", ["service:test"]);
    let mint_policy = allow_subject(mint_decision.workload_identity().subject());
    let minted = authorize(&mint_policy, &mint_decision);
    assert!(minted.outcome.is_ok());
    assert_audit_json_identity_grants(&minted.audit, &["service:test"]);

    let deny_decision = decision_for_tenant_with_identity_grants("beta", Vec::<String>::new());
    let deny_policy = allow_subject(deny_decision.workload_identity().subject());
    let denied = authorize(&deny_policy, &deny_decision);
    assert_eq!(denied.outcome, Err(IdentityMintError::IdentityGrantMissing));
    assert_audit_json_identity_grants(&denied.audit, &[]);
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
            "nimbus_issued_at_ms".to_string(),
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
    assert_eq!(claims.iat_epoch_ms(), 1_000);
    assert_eq!(value["nimbus_issued_at_ms"], 1_000);
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

#[test]
fn local_dev_issuer_mints_expected_jwt_and_omits_null_placement_claims() {
    let decision = decision_for_tenant("alpha");
    let policy = allow_subject(decision.workload_identity().subject());
    let request = IdentityMintRequest::for_decision(&decision, AUDIENCE, Duration::from_secs(30));
    let params = MintParams {
        issued_at_epoch_ms: 1_234,
        credential_instance_id: "jti-e2e".to_string(),
    };
    let signer = TestIdentitySigner::new("e2e");
    let issuer = local_dev_issuer(&signer);

    let mint = mint_credential(&policy, &request, &params, &issuer);

    assert!(matches!(mint.audit.outcome(), IdentityAuditOutcome::Minted));
    assert_eq!(mint.audit.exp_epoch_ms(), Some(31_234));
    assert_eq!(mint.audit.credential_instance_id(), Some("jti-e2e"));
    let credential = mint.outcome.expect("credential should mint");
    assert_eq!(credential.kind(), CredentialKind::OidcJwt);
    let token = credential.secret();
    let segments = jwt_segments(token);

    let header = decode_jwt_json_segment(segments[0]);
    assert_eq!(header["alg"], "EdDSA");
    assert_eq!(header["typ"], "JWT");
    assert_eq!(header["kid"], signer.signer.public_key().fingerprint());

    let payload = decode_jwt_json_segment(segments[1]);
    assert_eq!(payload["iss"], "identity.test");
    assert_eq!(payload["sub"], decision.workload_identity().subject());
    assert_eq!(payload["aud"], AUDIENCE);
    assert_eq!(payload["iat"], 1);
    assert_eq!(payload["exp"], 31);
    assert_eq!(
        payload["exp"].as_u64().expect("exp should be numeric")
            - payload["iat"].as_u64().expect("iat should be numeric"),
        30
    );
    assert_eq!(payload["jti"], "jti-e2e");
    assert_eq!(payload["nimbus_decision_id"], decision.id().as_str());
    assert_eq!(
        payload["nimbus_workload_subject"],
        decision.workload_identity().subject()
    );
    assert_eq!(
        payload["nimbus_workload_audit_projection"],
        decision.workload_identity().audit_projection()
    );
    let payload = payload
        .as_object()
        .expect("JWT payload should decode as an object");
    for omitted in [
        "nimbus_node_id",
        "nimbus_machine_id",
        "nimbus_sandbox_id",
        "nimbus_invocation_id",
        "nimbus_issued_at_ms",
    ] {
        assert!(
            !payload.contains_key(omitted),
            "JWT payload should omit {omitted}: {payload:?}"
        );
    }
}

#[test]
fn minted_jwt_signature_verifies_independently_with_ring_and_rejects_tampered_payload() {
    let decision = decision_for_tenant("alpha");
    let policy = allow_subject(decision.workload_identity().subject());
    let request = IdentityMintRequest::for_decision(&decision, AUDIENCE, Duration::from_secs(30));
    let signer = TestIdentitySigner::new("signature");
    let issuer = local_dev_issuer(&signer);
    let mint = mint_credential(&policy, &request, &params("jti-signature"), &issuer);
    let credential = mint.outcome.expect("credential should mint");
    let segments = jwt_segments(credential.secret());
    let signing_input = format!("{}.{}", segments[0], segments[1]);
    let signature = URL_SAFE_NO_PAD
        .decode(segments[2])
        .expect("signature segment should decode");
    let public_key = signer.signer.public_key();
    let verifier = UnparsedPublicKey::new(&ED25519, public_key.as_bytes());

    verifier
        .verify(signing_input.as_bytes(), &signature)
        .expect("ring should independently verify the JWT signature");

    let mut tampered_payload = decode_jwt_json_segment(segments[1]);
    tampered_payload["sub"] = Value::String("nimbus-workload:v1/tenant/alpha/tampered".to_string());
    let tampered_payload = URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&tampered_payload).expect("tampered payload should encode"));
    let tampered_input = format!("{}.{}", segments[0], tampered_payload);

    assert!(
        verifier
            .verify(tampered_input.as_bytes(), &signature)
            .is_err(),
        "tampered payload must not verify against the original signature"
    );
}

#[test]
fn mint_credential_denies_wrong_audience_without_token_and_audits_denial() {
    let decision = decision_for_tenant("alpha");
    let policy = allow_subject(decision.workload_identity().subject());
    let request =
        IdentityMintRequest::for_decision(&decision, "provider://other", Duration::from_secs(30));

    let mint = mint_credential(&policy, &request, &params("jti-wrong-aud"), &DenyAllIssuer);

    assert!(matches!(
        mint.outcome,
        Err(CredentialMintError::Authorization(
            IdentityMintError::AudienceNotAllowed { .. }
        ))
    ));
    assert!(matches!(
        mint.audit.outcome(),
        IdentityAuditOutcome::Denied { reason }
            if reason.contains("does not allow audience `provider://other`")
    ));
    assert_eq!(mint.audit.exp_epoch_ms(), None);
    assert_eq!(mint.audit.credential_instance_id(), None);
}

#[test]
fn local_dev_issuer_is_unconstructible_under_production_trust() {
    let public_key = IdentityPublicKey::from_ed25519_bytes([0x33; 32]);
    let record = NodeIdentityRecord::local_dev(&public_key);
    let trust =
        IdentityTrustConfig::production("identity.test").expect("production trust should build");
    let signer: Arc<dyn IdentitySigner> = Arc::new(FailingSigner::new(
        public_key,
        "top-secret-production-key-marker",
    ));

    let result = LocalDevIssuer::new(trust, &record, signer);

    assert!(matches!(
        result,
        Err(TrustConfigError::SourceNotAdmitted { .. })
    ));
}

#[test]
fn issuance_failure_returns_error_and_denied_audit_without_secret_material() {
    let decision = decision_for_tenant("alpha");
    let policy = allow_subject(decision.workload_identity().subject());
    let request = IdentityMintRequest::for_decision(&decision, AUDIENCE, Duration::from_secs(30));
    let public_key = IdentityPublicKey::from_ed25519_bytes([0x44; 32]);
    let leaked_marker = "top-secret-signing-key-marker";
    let record = NodeIdentityRecord::local_dev(&public_key);
    let trust = IdentityTrustConfig::local_dev("identity.test").expect("local trust should build");
    let signer: Arc<dyn IdentitySigner> = Arc::new(FailingSigner::new(public_key, leaked_marker));
    let issuer = LocalDevIssuer::new(trust, &record, signer).expect("local issuer should build");

    let mint = mint_credential(&policy, &request, &params("jti-failed-issuance"), &issuer);

    assert!(matches!(
        &mint.outcome,
        Err(CredentialMintError::Issuance(IdentityIssueError::Failed(message)))
            if message == "signer failed to produce identity signature"
    ));
    assert!(matches!(
        mint.audit.outcome(),
        IdentityAuditOutcome::Denied { reason }
            if reason == "issuance failed: identity issuance failed: signer failed to produce identity signature"
    ));
    assert_eq!(mint.audit.exp_epoch_ms(), Some(31_000));
    assert_eq!(
        mint.audit.credential_instance_id(),
        Some("jti-failed-issuance")
    );
    assert_audit_has_no_secret_material(&mint.audit, leaked_marker);
}

#[test]
fn minted_credential_debug_redacts_jwt_and_secret_round_trips() {
    let decision = decision_for_tenant("alpha");
    let policy = allow_subject(decision.workload_identity().subject());
    let request = IdentityMintRequest::for_decision(&decision, AUDIENCE, Duration::from_secs(30));
    let signer = TestIdentitySigner::new("debug");
    let issuer = local_dev_issuer(&signer);
    let mint = mint_credential(&policy, &request, &params("jti-debug"), &issuer);
    let credential = mint.outcome.expect("credential should mint");
    let token = credential.secret().to_string();

    let debug = format!("{credential:?}");

    assert!(debug.contains("[REDACTED]"));
    assert!(!debug.contains(&token));
    assert_eq!(credential.secret(), token);
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

fn assert_audit_json_identity_grants(
    audit: &nimbus_workload_identity::IdentityAuditEvent,
    expected: &[&str],
) {
    let value = serde_json::to_value(audit).expect("audit should serialize");
    let object = value
        .as_object()
        .expect("audit should serialize as an object");
    assert!(
        object.contains_key("identity_grants"),
        "audit JSON must include identity_grants: {value}"
    );
    let grants = value["identity_grants"]
        .as_array()
        .expect("identity_grants should serialize as an array");
    let actual = grants
        .iter()
        .map(|grant| {
            grant
                .as_str()
                .expect("identity grant should serialize as a string")
        })
        .collect::<Vec<_>>();
    assert_eq!(actual.as_slice(), expected);
    assert_no_secret_keys(&value);
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

struct TestIdentitySigner {
    signer: Arc<FileBackedIdentitySigner>,
    key_path: PathBuf,
}

impl TestIdentitySigner {
    fn new(label: &str) -> Self {
        let key_path = unique_key_path(label);
        remove_identity_key_files(&key_path);
        let signer = FileBackedIdentitySigner::open(&key_path, OpenMode::GenerateIfAbsent)
            .expect("test identity signer should open");
        Self {
            signer: Arc::new(signer),
            key_path,
        }
    }

    fn erased(&self) -> Arc<dyn IdentitySigner> {
        let signer: Arc<dyn IdentitySigner> = self.signer.clone();
        signer
    }
}

impl Drop for TestIdentitySigner {
    fn drop(&mut self) {
        remove_identity_key_files(&self.key_path);
    }
}

fn local_dev_issuer(signer: &TestIdentitySigner) -> LocalDevIssuer {
    let record = NodeIdentityRecord::local_dev(&signer.signer.public_key());
    let trust = IdentityTrustConfig::local_dev("identity.test").expect("local trust should build");
    LocalDevIssuer::new(trust, &record, signer.erased()).expect("local issuer should build")
}

fn jwt_segments(token: &str) -> Vec<&str> {
    let segments = token.split('.').collect::<Vec<_>>();
    assert_eq!(segments.len(), 3, "JWT should have three compact segments");
    assert!(
        segments.iter().all(|segment| !segment.is_empty()),
        "JWT segments should be non-empty"
    );
    segments
}

fn decode_jwt_json_segment(segment: &str) -> Value {
    let bytes = URL_SAFE_NO_PAD
        .decode(segment)
        .expect("JWT segment should use base64url no-pad");
    serde_json::from_slice(&bytes).expect("JWT segment should decode as JSON")
}

fn unique_key_path(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "nimbus-workload-identity-{label}-{}-{nanos}.key",
        process::id()
    ))
}

fn remove_identity_key_files(path: &Path) {
    for path in [
        path.to_path_buf(),
        append_suffix(path, ".lock"),
        append_suffix(path, ".rotating"),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to remove {}: {error}", path.display()),
        }
    }
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_owned();
    value.push(suffix);
    PathBuf::from(value)
}

struct FailingSigner {
    public_key: IdentityPublicKey,
    leaked_marker: String,
}

impl FailingSigner {
    fn new(public_key: IdentityPublicKey, leaked_marker: impl Into<String>) -> Self {
        Self {
            public_key,
            leaked_marker: leaked_marker.into(),
        }
    }
}

impl IdentitySigner for FailingSigner {
    fn sign(&self, _message: &[u8]) -> SigningResult<IdentitySignature> {
        Err(SigningError::MalformedInMemoryKey {
            key_id: self.leaked_marker.clone(),
        })
    }

    fn verify(&self, _message: &[u8], _signature: &IdentitySignature) -> SigningResult<()> {
        Err(SigningError::MalformedInMemoryKey {
            key_id: self.leaked_marker.clone(),
        })
    }

    fn public_key(&self) -> IdentityPublicKey {
        self.public_key
    }

    fn kind(&self) -> IdentitySignerKind {
        IdentitySignerKind::FileBacked {
            path: "[redacted-test-path]".to_string(),
            fingerprint: self.public_key.fingerprint(),
        }
    }
}
