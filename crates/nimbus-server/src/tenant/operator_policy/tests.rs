use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;
use std::time::Duration;

use crate::tenant::{
    TenantImageAdmissionSource, TenantImageVerificationEvidence, TenantImageVerificationProvider,
    TenantImageVerificationRequest, TenantRuntimePolicyAdmission,
};
use nimbus_sandbox::PublishedEndpointProtocol;

use super::*;

const VALID_POLICY: &str = include_str!("../../../tests/fixtures/policy/valid-enterprise.yaml");
const INVALID_WILDCARD: &str = include_str!("../../../tests/fixtures/policy/invalid-wildcard.yaml");
const INVALID_PORT: &str = include_str!("../../../tests/fixtures/policy/invalid-port.yaml");
const INVALID_SECRET: &str = include_str!("../../../tests/fixtures/policy/invalid-secret.yaml");
const INVALID_IMAGE: &str = include_str!("../../../tests/fixtures/policy/invalid-image.yaml");
const UNKNOWN_FIELD: &str = include_str!("../../../tests/fixtures/policy/unknown-field.yaml");
const NODE_ROUTE: &str = include_str!("../../../tests/fixtures/policy/node-route.yaml");
const DIFF_FROM: &str = include_str!("../../../tests/fixtures/policy/diff-from.yaml");
const DIFF_TO: &str = include_str!("../../../tests/fixtures/policy/diff-to.yaml");
const REGISTRY_WIDE_IMAGE_POLICY: &str = r#"
schema_version: 1
tenant: tenant-a
workloads:
  - kind: runtime_function
    name: "images:launch"
    image:
      allowed_registries:
        - registry.example.com
"#;
const SBOM_REQUIRED_IMAGE_POLICY: &str = r#"
schema_version: 1
tenant: tenant-a
workloads:
  - kind: runtime_function
    name: "images:launch"
    image:
      reference: "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
      sbom_required: true
"#;
const INVALID_EGRESS_POLICY: &str = r#"
schema_version: 1
tenant: tenant-a
workloads:
  - kind: sandbox_service
    name: "worker"
    sandbox:
      sandbox_id: "worker-1"
      backend: container
    network:
      egress:
        allow:
          - name: all
            protocol: https
            host: "*.example.com"
            port: 443
"#;
const EGRESS_DIFF_FROM: &str = r#"
schema_version: 1
tenant: tenant-a
workloads:
  - kind: sandbox_service
    name: "worker"
    sandbox:
      sandbox_id: "worker-1"
      backend: container
    network:
      egress:
        allow:
          - name: stripe
            protocol: https
            host: api.stripe.com
            port: 443
            methods:
              - POST
            path_prefixes:
              - /v1/
"#;
const EGRESS_DIFF_TO: &str = r#"
schema_version: 1
tenant: tenant-a
workloads:
  - kind: sandbox_service
    name: "worker"
    sandbox:
      sandbox_id: "worker-1"
      backend: container
    network:
      egress:
        allow:
          - name: stripe
            protocol: https
            host: api.stripe.com
            port: 443
            methods:
              - POST
            path_prefixes:
              - /v1/
          - name: github
            protocol: https
            host: api.github.com
            port: 443
            methods:
              - GET
            path_prefixes:
              - /repos/
"#;
const INVALID_CUSTOM_STORAGE_NAMESPACE: &str = r#"
schema_version: 1
tenant: tenant-a
defaults:
  storage_namespace: shared
workloads:
  - kind: runtime_function
    name: "messages:send"
"#;
const AUTHORITY_DIFF_FROM: &str = r#"
schema_version: 1
tenant: tenant-a
workloads:
  - kind: runtime_function
    name: "messages:send"
    volumes:
      named:
        - cache # 002-auth-caching-policy: named volume fixture, not auth cache
    image:
      allowed_registries:
        - registry-a.example.com
    secrets:
      handles:
        - prod/db/password
    quotas:
      sandbox_charge:
        active_sandboxes: 1
        vcpus: 1
        memory_bytes: 536870912
        disk_bytes: 10737418240
        log_bytes: 67108864
"#;
const AUTHORITY_DIFF_TO: &str = r#"
schema_version: 1
tenant: tenant-a
workloads:
  - kind: runtime_function
    name: "messages:send"
    volumes:
      named:
        - data
    image:
      allowed_registries:
        - registry-b.example.com
      sbom_required: true
    secrets:
      handles:
        - prod/cache/password # 002-auth-caching-policy: redacted secret-handle fixture, not auth cache
    quotas:
      sandbox_charge:
        active_sandboxes: 1
        vcpus: 1
        memory_bytes: 1073741824
        disk_bytes: 10737418240
        log_bytes: 67108864
    audit:
      redacted_fields:
        - principal_claims
        - bearer_claims
        - secret_handles
        - raw_credentials
        - query_params
"#;
const PROVE_RISK_POLICY: &str = r#"
schema_version: 1
tenant: tenant-a
workloads:
  - kind: runtime_function
    name: "risky:send"
    runtime:
      tier: in_process_untrusted
    services:
      allow:
        - db
    network:
      endpoints:
        - service: db
          name: postgres
          protocol: tcp
          host: 10.0.0.20
          host_port: 15432
          guest_port: 5432
      egress:
        allow:
          - name: github
            protocol: https
            host: api.github.com
            port: 443
    secrets:
      handles:
        - tenant-b/db/password
        - prod/api/key
"#;
const PROVE_ACCEPTED_RISK_POLICY: &str = r#"
schema_version: 1
tenant: tenant-a
accepted_risks:
  - advisory_id: broad_egress:runtime_function/risky:send:github
    approved_by: security-review
    reason: GitHub metadata endpoint accepted during bootstrap
workloads:
  - kind: runtime_function
    name: "risky:send"
    runtime:
      tier: in_process_untrusted
    services:
      allow:
        - db
    network:
      endpoints:
        - service: db
          name: postgres
          protocol: tcp
          host: 10.0.0.20
          host_port: 15432
          guest_port: 5432
      egress:
        allow:
          - name: github
            protocol: https
            host: api.github.com
            port: 443
    secrets:
      handles:
        - tenant-b/db/password
        - prod/api/key
"#;

struct NoopImageVerifier;

impl TenantImageVerificationProvider for NoopImageVerifier {
    fn verify_registry_image(
        &self,
        _request: &TenantImageVerificationRequest,
    ) -> Result<TenantImageVerificationEvidence> {
        Ok(TenantImageVerificationEvidence::new())
    }
}

struct SbomImageVerifier;

impl TenantImageVerificationProvider for SbomImageVerifier {
    fn verify_registry_image(
        &self,
        _request: &TenantImageVerificationRequest,
    ) -> Result<TenantImageVerificationEvidence> {
        Ok(TenantImageVerificationEvidence::new().with_sbom())
    }
}

fn parse_policy(body: &str) -> OperatorPolicyDocument {
    serde_yaml::from_str(body).expect("policy fixture should parse")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeExternalPolicyResponse {
    Allow,
    AllowSensitiveReason,
    Deny,
    DenySensitiveReason,
    MalformedOutput,
    Timeout,
    SleepPastEngineTimeout,
    Unavailable,
    UnavailableSensitiveReason,
}

struct FakeExternalPolicyBackend {
    response: FakeExternalPolicyResponse,
    name: &'static str,
    version: &'static str,
    calls: AtomicUsize,
    last_request: Mutex<Option<OperatorExternalPolicyRequest>>,
}

impl FakeExternalPolicyBackend {
    fn new(
        response: FakeExternalPolicyResponse,
        name: &'static str,
        version: &'static str,
    ) -> Self {
        Self {
            response,
            name,
            version,
            calls: AtomicUsize::new(0),
            last_request: Mutex::new(None),
        }
    }

    fn fake_opa(response: FakeExternalPolicyResponse) -> Self {
        Self::new(response, "fake-opa", "v0-test")
    }

    fn fake_cedar(response: FakeExternalPolicyResponse) -> Self {
        Self::new(response, "fake-cedar", "v0-test")
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn last_request(&self) -> OperatorExternalPolicyRequest {
        self.last_request
            .lock()
            .expect("request lock should not be poisoned")
            .clone()
            .expect("backend should have received a request")
    }
}

impl OperatorExternalPolicyBackend for FakeExternalPolicyBackend {
    fn evaluate(
        &self,
        request: &OperatorExternalPolicyRequest,
    ) -> OperatorExternalPolicyBackendResult<OperatorExternalPolicyDecision> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.last_request
            .lock()
            .expect("request lock should not be poisoned")
            .replace(request.clone());
        match self.response {
            FakeExternalPolicyResponse::Allow => Ok(OperatorExternalPolicyDecision::allow(
                self.name,
                self.version,
                "fixture policy allowed",
            )),
            FakeExternalPolicyResponse::AllowSensitiveReason => {
                Ok(OperatorExternalPolicyDecision::allow(
                    self.name,
                    self.version,
                    "Authorization: Bearer do-not-log-token",
                ))
            }
            FakeExternalPolicyResponse::Deny => Ok(OperatorExternalPolicyDecision::deny(
                self.name,
                self.version,
                "fixture policy denied",
            )),
            FakeExternalPolicyResponse::DenySensitiveReason => {
                Ok(OperatorExternalPolicyDecision::deny(
                    self.name,
                    self.version,
                    "https://policy.local/deny?token=do-not-log-token",
                ))
            }
            FakeExternalPolicyResponse::MalformedOutput => {
                Ok(OperatorExternalPolicyDecision::allow(
                    self.name,
                    "",
                    "fixture policy returned malformed output",
                ))
            }
            FakeExternalPolicyResponse::Timeout => Err(
                OperatorExternalPolicyBackendError::timeout("fixture policy deadline exceeded"),
            ),
            FakeExternalPolicyResponse::SleepPastEngineTimeout => {
                thread::sleep(Duration::from_millis(200));
                Ok(OperatorExternalPolicyDecision::allow(
                    self.name,
                    self.version,
                    "fixture policy allowed too late",
                ))
            }
            FakeExternalPolicyResponse::Unavailable => {
                Err(OperatorExternalPolicyBackendError::unavailable(
                    "fixture policy backend unavailable",
                ))
            }
            FakeExternalPolicyResponse::UnavailableSensitiveReason => {
                Err(OperatorExternalPolicyBackendError::unavailable(
                    "Authorization: Bearer do-not-log-token",
                ))
            }
        }
    }
}

#[test]
fn valid_policy_fixture_compiles_to_tenant_isolation_decision() {
    let policy = parse_policy(VALID_POLICY);

    policy
        .validate()
        .expect("public validate should compile policy");
    let evaluation = policy.evaluate().expect("policy should evaluate");

    assert_eq!(evaluation.tenant_id, "tenant-a");
    assert_eq!(evaluation.decision_count, 1);
    let decision = &evaluation.decisions[0];
    assert_eq!(decision.workload_key, "runtime_function/messages:send");
    assert_eq!(decision.storage_namespace, "tenant-a");
    assert_eq!(decision.services, vec!["db".to_string()]);
    assert_eq!(decision.network_endpoints.len(), 1);
    assert_eq!(decision.sandbox_egress.len(), 1);
    assert!(
        decision.sandbox_egress[0].contains("stripe-api"),
        "egress summary should name the rule: {:?}",
        decision.sandbox_egress
    );
    assert_eq!(
        decision.runtime_admission,
        TenantRuntimePolicyAdmission::AdmitInProcess
    );
    assert!(
        decision.decision_id.starts_with("tid_"),
        "decision id should come from TenantIsolationDecision"
    );
    assert!(
        decision
            .decision
            .to_audit_record()
            .workload_stable_id
            .contains("messages%3Asend"),
        "compiled decision should produce normal tenant-isolation audit evidence"
    );

    let rendered = evaluation.render_explain_text();
    assert!(rendered.contains("runtime_function/messages:send"));
    assert!(rendered.contains("runtime_admission: admit_in_process"));
    assert!(rendered.contains("storage_namespace: tenant-a"));
    assert!(rendered.contains("sandbox_egress: stripe-api"));
}

#[test]
fn external_policy_backend_allow_records_decision_evidence() {
    let policy = parse_policy(VALID_POLICY);
    let backend = Arc::new(FakeExternalPolicyBackend::fake_opa(
        FakeExternalPolicyResponse::Allow,
    ));
    let engine = OperatorExternalPolicyEngine::from_arc(backend.clone())
        .with_policy_bundle_hash(
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        .expect("policy bundle hash should be valid");

    let evaluation = policy
        .evaluate_with_external_policy(Some(&engine))
        .expect("allowing external policy should admit");

    assert_eq!(backend.call_count(), 1);
    let request = backend.last_request();
    assert_eq!(request.policy_name.as_deref(), Some("enterprise-baseline"));
    assert_eq!(request.tenant_id, "tenant-a");
    assert_eq!(request.workload_key, "runtime_function/messages:send");
    assert_eq!(request.workload_kind, "runtime_function");
    assert_eq!(request.workload_name, "messages:send");
    assert_eq!(request.runtime_tier, "in_process_untrusted");
    assert_eq!(request.runtime_admission, "admit_in_process");
    assert_eq!(request.secret_handle_count, 1);
    assert_eq!(
        request.policy_bundle_hash.as_deref(),
        Some("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );
    assert!(
        request.input_digest.starts_with("sha256:"),
        "external request should carry a stable digest: {}",
        request.input_digest
    );
    assert_eq!(request.timeout_millis, 2_000);
    assert!(
        !serde_json::to_string(&request)
            .expect("external request should serialize")
            .contains("prod/db/password"),
        "external policy requests must not carry raw secret handles"
    );

    let decision = &evaluation.decisions[0];
    let evidence = decision
        .external_policy
        .as_ref()
        .expect("allowing backend should attach evidence");
    assert_eq!(evidence.backend.name, "fake-opa");
    assert_eq!(evidence.backend.version, "v0-test");
    assert_eq!(evidence.outcome, OperatorExternalPolicyOutcome::Allow);
    assert_eq!(evidence.reason_code, "external_policy_allowed");
    assert_eq!(evidence.reason, "fixture policy allowed");
    assert_eq!(
        evidence.policy_bundle_hash.as_deref(),
        Some("sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
    );
    assert_eq!(evidence.input_digest, request.input_digest);
    assert_eq!(evidence.timeout_millis, 2_000);
    let rendered = evaluation.render_explain_text();
    assert!(rendered.contains("external_policy: allow via fake-opa@v0-test"));
    assert!(rendered.contains("trace: external policy: allow via fake-opa@v0-test"));
}

#[test]
fn external_policy_backend_deny_fails_closed() {
    let policy = parse_policy(VALID_POLICY);
    let backend = Arc::new(FakeExternalPolicyBackend::fake_cedar(
        FakeExternalPolicyResponse::Deny,
    ));
    let engine = OperatorExternalPolicyEngine::from_arc(backend.clone());

    let error = policy
        .evaluate_with_external_policy(Some(&engine))
        .expect_err("external policy deny should fail closed");

    assert_eq!(backend.call_count(), 1);
    assert!(
        error.to_string().contains("fake-cedar@v0-test")
            && error.to_string().contains("denied workload")
            && error.to_string().contains("external_policy_denied")
            && error.to_string().contains("fixture policy denied"),
        "deny error should identify backend and reason: {error}"
    );
}

#[test]
fn external_policy_evidence_and_errors_redact_sensitive_backend_text() {
    let policy = parse_policy(VALID_POLICY);
    let allow_backend = Arc::new(FakeExternalPolicyBackend::fake_opa(
        FakeExternalPolicyResponse::AllowSensitiveReason,
    ));
    let allow_engine = OperatorExternalPolicyEngine::from_arc(allow_backend);
    let evaluation = policy
        .evaluate_with_external_policy(Some(&allow_engine))
        .expect("allowing external policy should admit");
    let evidence = evaluation.decisions[0]
        .external_policy
        .as_ref()
        .expect("allowing backend should attach evidence");
    assert_eq!(evidence.reason_code, "external_policy_allowed");
    assert_eq!(evidence.reason, "[redacted evidence text]");

    let deny_backend = Arc::new(FakeExternalPolicyBackend::fake_opa(
        FakeExternalPolicyResponse::DenySensitiveReason,
    ));
    let deny_engine = OperatorExternalPolicyEngine::from_arc(deny_backend);
    let deny_error = policy
        .evaluate_with_external_policy(Some(&deny_engine))
        .expect_err("external policy deny should fail closed");
    assert!(deny_error.to_string().contains("[redacted evidence text]"));
    assert!(!deny_error.to_string().contains("do-not-log-token"));

    let unavailable_backend = Arc::new(FakeExternalPolicyBackend::fake_opa(
        FakeExternalPolicyResponse::UnavailableSensitiveReason,
    ));
    let unavailable_engine = OperatorExternalPolicyEngine::from_arc(unavailable_backend);
    let unavailable_error = policy
        .evaluate_with_external_policy(Some(&unavailable_engine))
        .expect_err("external policy backend failure should fail closed");
    assert!(
        unavailable_error
            .to_string()
            .contains("[redacted evidence text]")
    );
    assert!(!unavailable_error.to_string().contains("do-not-log-token"));
}

#[test]
fn external_policy_backend_errors_fail_closed() {
    for (response, expected) in [
        (
            FakeExternalPolicyResponse::MalformedOutput,
            "malformed_output",
        ),
        (FakeExternalPolicyResponse::Timeout, "timeout"),
        (FakeExternalPolicyResponse::Unavailable, "unavailable"),
    ] {
        let policy = parse_policy(VALID_POLICY);
        let backend = Arc::new(FakeExternalPolicyBackend::fake_opa(response));
        let engine = OperatorExternalPolicyEngine::from_arc(backend.clone());

        let error = policy
            .evaluate_with_external_policy(Some(&engine))
            .expect_err("external policy backend failure should fail closed");

        assert_eq!(backend.call_count(), 1);
        assert!(
            error.to_string().contains("failed closed")
                && error.to_string().contains(expected)
                && error.to_string().contains("runtime_function/messages:send"),
            "backend error should be fail-closed and actionable: {error}"
        );
    }
}

#[test]
fn external_policy_engine_timeout_fails_closed_without_waiting_for_backend() {
    let policy = parse_policy(VALID_POLICY);
    let backend = Arc::new(FakeExternalPolicyBackend::fake_opa(
        FakeExternalPolicyResponse::SleepPastEngineTimeout,
    ));
    let engine = OperatorExternalPolicyEngine::from_arc(backend.clone())
        .with_timeout(Duration::from_millis(25))
        .expect("timeout should be valid");

    let started = std::time::Instant::now();
    let error = policy
        .evaluate_with_external_policy(Some(&engine))
        .expect_err("engine timeout should fail closed");

    assert_eq!(backend.call_count(), 1);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "engine timeout should bound admission latency"
    );
    assert!(
        error.to_string().contains("failed closed")
            && error.to_string().contains("timeout")
            && error.to_string().contains("25ms"),
        "timeout error should be fail-closed and actionable: {error}"
    );
}

#[test]
fn built_in_hard_deny_precedes_external_policy_backend_allow() {
    let policy = parse_policy(INVALID_IMAGE);
    let backend = Arc::new(FakeExternalPolicyBackend::fake_opa(
        FakeExternalPolicyResponse::Allow,
    ));
    let engine = OperatorExternalPolicyEngine::from_arc(backend.clone());

    let error = policy
        .evaluate_with_external_policy(Some(&engine))
        .expect_err("built-in image policy should reject before external allow");

    assert_eq!(
        backend.call_count(),
        0,
        "external policy must not be consulted after a built-in hard deny"
    );
    assert!(
        error
            .to_string()
            .contains("image.digest_required=false is unsafe"),
        "built-in hard-deny reason should be preserved: {error}"
    );
}

#[test]
fn denied_egress_draft_proposes_minimal_rule_without_mutating_policy() {
    let policy = parse_policy(EGRESS_DIFF_FROM);
    let event = OperatorDeniedEgressEvent {
        tenant_id: "tenant-a".to_string(),
        workload_kind: "sandbox_service".to_string(),
        workload_name: "worker".to_string(),
        protocol: PublishedEndpointProtocol::Https,
        host: "API.GitHub.com".to_string(),
        port: 443,
        method: Some("get".to_string()),
        path: Some("/repos/nimbus/nimbus?token=do-not-log".to_string()),
        reason: "sandbox egress default deny".to_string(),
    };

    let draft = policy
        .draft_from_denied_egress(event)
        .expect("external denied egress should produce a draft");

    assert_eq!(draft.kind, OperatorPolicyDraftKind::SandboxEgressAllow);
    assert_eq!(draft.status, OperatorPolicyDraftStatus::ReviewRequired);
    assert!(draft.requires_explicit_approval);
    assert!(!draft.auto_apply);
    assert_eq!(draft.workload_key, "sandbox_service/worker");
    assert_eq!(draft.suggested_egress_rule.name, "api-github-com-https-443");
    assert_eq!(draft.suggested_egress_rule.host, "api.github.com");
    assert_eq!(draft.suggested_egress_rule.port, 443);
    assert_eq!(draft.suggested_egress_rule.methods, vec!["GET".to_string()]);
    assert_eq!(
        draft.suggested_egress_rule.path_prefixes,
        vec!["/repos/nimbus/nimbus".to_string()]
    );
    assert!(
        !serde_yaml::to_string(&draft)
            .expect("draft should serialize")
            .contains("do-not-log"),
        "query parameters from denial evidence must not leak into draft policy"
    );
    assert_eq!(
        policy.workloads[0].network.egress.allow.len(),
        1,
        "draft generation must not mutate the source policy"
    );
}

#[test]
fn denied_egress_draft_requires_approval_before_apply() {
    let policy = parse_policy(EGRESS_DIFF_FROM);
    let draft = policy
        .draft_from_denied_egress(OperatorDeniedEgressEvent {
            tenant_id: "tenant-a".to_string(),
            workload_kind: "sandbox_service".to_string(),
            workload_name: "worker".to_string(),
            protocol: PublishedEndpointProtocol::Https,
            host: "api.github.com".to_string(),
            port: 443,
            method: Some("GET".to_string()),
            path: Some("/repos/".to_string()),
            reason: "sandbox egress default deny".to_string(),
        })
        .expect("denied egress should produce a draft");

    let error = draft
        .apply_to(&policy, None)
        .expect_err("draft apply should require approval");
    assert!(
        error.to_string().contains("requires explicit approval"),
        "approval error should be clear: {error}"
    );

    let approval =
        OperatorPolicyDraftApproval::new("security-reviewer", "approved GitHub metadata egress")
            .expect("approval should be valid");
    let updated = draft
        .apply_to(&policy, Some(&approval))
        .expect("approved draft should apply to a cloned policy");

    assert_eq!(
        policy.workloads[0].network.egress.allow.len(),
        1,
        "approved apply must still leave source policy untouched"
    );
    assert_eq!(updated.workloads[0].network.egress.allow.len(), 2);
    let evaluation = updated.evaluate().expect("applied policy should evaluate");
    assert!(
        evaluation.decisions[0]
            .sandbox_egress
            .iter()
            .any(|summary| summary.contains("api-github-com-https-443")),
        "approved draft should produce real policy authority: {:?}",
        evaluation.decisions[0].sandbox_egress
    );
}

#[test]
fn denied_egress_draft_rejects_mismatched_tenant_and_unknown_workload() {
    let policy = parse_policy(EGRESS_DIFF_FROM);
    let mismatched_tenant = OperatorDeniedEgressEvent {
        tenant_id: "tenant-b".to_string(),
        workload_kind: "sandbox_service".to_string(),
        workload_name: "worker".to_string(),
        protocol: PublishedEndpointProtocol::Https,
        host: "api.github.com".to_string(),
        port: 443,
        method: Some("GET".to_string()),
        path: Some("/repos/".to_string()),
        reason: "sandbox egress default deny".to_string(),
    };
    let error = policy
        .draft_from_denied_egress(mismatched_tenant)
        .expect_err("tenant mismatch should reject");
    assert!(
        error.to_string().contains("does not match policy tenant"),
        "tenant mismatch should be explicit: {error}"
    );

    let unknown_workload = OperatorDeniedEgressEvent {
        tenant_id: "tenant-a".to_string(),
        workload_kind: "sandbox_service".to_string(),
        workload_name: "missing".to_string(),
        protocol: PublishedEndpointProtocol::Https,
        host: "api.github.com".to_string(),
        port: 443,
        method: Some("GET".to_string()),
        path: Some("/repos/".to_string()),
        reason: "sandbox egress default deny".to_string(),
    };
    let error = policy
        .draft_from_denied_egress(unknown_workload)
        .expect_err("unknown workload should reject");
    assert!(
        error.to_string().contains("is not present in policy"),
        "unknown workload should be explicit: {error}"
    );
}

#[test]
fn policy_prove_detects_enterprise_risk_advisories() {
    let policy = parse_policy(PROVE_RISK_POLICY);

    let report = policy.prove().expect("prove should evaluate policy");

    assert_eq!(report.checked_workloads, 1);
    assert_eq!(report.accepted_count, 0);
    assert_eq!(report.unaccepted_count, report.advisory_count);
    for kind in [
        OperatorPolicyAdvisoryKind::BroadEgress,
        OperatorPolicyAdvisoryKind::WriteBypass,
        OperatorPolicyAdvisoryKind::SecretExposure,
        OperatorPolicyAdvisoryKind::CrossTenantRegression,
    ] {
        assert!(
            report
                .advisories
                .iter()
                .any(|advisory| advisory.kind == kind),
            "expected advisory kind {kind:?}: {:?}",
            report.advisories
        );
    }
    let rendered = report.render_text();
    assert!(rendered.contains("Policy prove"));
    assert!(rendered.contains("broad_egress:runtime_function/risky:send:github"));
    assert!(rendered.contains("write_bypass:runtime_function/risky:send:db/postgres"));
    assert!(rendered.contains("secret_exposure:runtime_function/risky:send:in_process_untrusted"));
    assert!(rendered.contains("cross_tenant_regression:runtime_function/risky:send:tenant-b"));
    assert!(rendered.contains("unaccepted"));
}

#[test]
fn policy_prove_marks_accepted_risks_without_hiding_regressions() {
    let policy = parse_policy(PROVE_ACCEPTED_RISK_POLICY);

    let report = policy.prove().expect("prove should evaluate policy");

    let broad = report
        .advisories
        .iter()
        .find(|advisory| advisory.id == "broad_egress:runtime_function/risky:send:github")
        .expect("broad egress advisory should exist");
    assert!(
        broad.accepted_risk.is_some(),
        "accepted risk should attach to matching advisory"
    );
    assert_eq!(report.accepted_count, 1);
    assert!(
        report.unaccepted_count >= 3,
        "accepted risk should not suppress other advisories: {:?}",
        report.advisories
    );
    let rendered = report.render_text();
    assert!(rendered.contains("accepted_by: security-review"));
    assert!(rendered.contains("write_bypass"));
    assert!(rendered.contains("unaccepted"));
}

#[test]
fn policy_prove_rejects_malformed_accepted_risks() {
    let policy = parse_policy(
        r#"
schema_version: 1
tenant: tenant-a
accepted_risks:
  - advisory_id: broad_egress:runtime_function/foo:github
    approved_by: ""
    reason: missing reviewer
workloads:
  - kind: runtime_function
    name: "foo"
"#,
    );

    let error = policy
        .prove()
        .expect_err("malformed accepted risk should reject");

    assert!(
        error.to_string().contains("requires approved_by"),
        "accepted risk validation should be actionable: {error}"
    );
}

#[test]
fn registry_wide_image_policy_still_requires_digest_pinned_launches() {
    let policy = parse_policy(REGISTRY_WIDE_IMAGE_POLICY);

    let evaluation = policy.evaluate().expect("policy should evaluate");
    let image = evaluation.decisions[0].decision.image();

    let error = image
        .admit_image(
            TenantImageAdmissionSource::registry("registry.example.com/nimbus/api:latest"),
            &NoopImageVerifier,
        )
        .expect_err("registry-wide policy should still reject tag-only references");
    assert!(
        error
            .to_string()
            .contains("requires an immutable sha256 digest reference"),
        "tag-only image should be rejected by compiled digest policy: {error}"
    );

    image
            .admit_image(
                TenantImageAdmissionSource::registry(
                    "registry.example.com/nimbus/api@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                ),
                &NoopImageVerifier,
            )
            .expect("digest-pinned image from an allowed registry should pass");
}

#[test]
fn operator_image_policy_sbom_required_compiles_to_admission_hook() {
    let policy = parse_policy(SBOM_REQUIRED_IMAGE_POLICY);
    let evaluation = policy.evaluate().expect("policy should evaluate");
    let image = evaluation.decisions[0].decision.image();
    let source = TenantImageAdmissionSource::registry(
        "registry.example.com/nimbus/api@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );

    let error = image
        .admit_image(source.clone(), &NoopImageVerifier)
        .expect_err("SBOM-required operator policy should reject missing SBOM evidence");
    assert!(
        error.to_string().contains("requires SBOM evidence"),
        "missing SBOM evidence should be explicit: {error}"
    );

    image
        .admit_image(source, &SbomImageVerifier)
        .expect("SBOM evidence should satisfy compiled operator policy");
}

#[test]
fn policy_fixture_rejects_unknown_fields_at_parse_time() {
    let error = serde_yaml::from_str::<OperatorPolicyDocument>(UNKNOWN_FIELD)
        .expect_err("unknown fields should be rejected");
    assert!(
        error.to_string().contains("unknown field"),
        "serde should report strict parsing failure: {error}"
    );
}

#[test]
fn policy_fixture_rejects_wildcard_hosts_and_unsafe_image_defaults() {
    let policy = parse_policy(INVALID_WILDCARD);

    let error = policy
        .evaluate()
        .expect_err("wildcard policy should fail closed");

    assert!(
        error.to_string().contains("wildcard"),
        "error should name the unsafe wildcard: {error}"
    );
}

#[test]
fn policy_fixtures_reject_invalid_port_secret_and_image_shapes() {
    let cases = [
        (INVALID_PORT, "host_port must not be 0"),
        (INVALID_SECRET, "looks like inline secret material"),
        (INVALID_IMAGE, "image.digest_required=false is unsafe"),
        (INVALID_EGRESS_POLICY, "wildcards"),
        (
            INVALID_CUSTOM_STORAGE_NAMESPACE,
            "custom storage namespaces are deferred",
        ),
    ];

    for (body, expected) in cases {
        let policy = parse_policy(body);
        let error = match policy.evaluate() {
            Ok(_) => panic!("policy should reject `{expected}`"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains(expected),
            "error should contain `{expected}`: {error}"
        );
    }
}

#[test]
fn node_profile_routes_away_from_production_in_process_untrusted() {
    let policy = parse_policy(NODE_ROUTE);

    let evaluation = policy.evaluate().expect("policy should evaluate");

    let admission = &evaluation.decisions[0].runtime_admission;
    assert!(
        matches!(
            admission,
            TenantRuntimePolicyAdmission::Route {
                recommended_tier: RuntimeIsolationTier::MicroVmService,
                ..
            }
        ),
        "existing runtime admission should route broad Node grants: {admission:?}"
    );
    let rendered = evaluation.render_explain_text();
    assert!(rendered.contains("route_to_microvm_service"));
}

#[test]
fn policy_diff_reports_authority_deltas() {
    let from = parse_policy(DIFF_FROM);
    let to = parse_policy(DIFF_TO);

    let diff = OperatorPolicyDiff::between(&from, &to).expect("diff should evaluate");

    assert_eq!(diff.added_workloads.len(), 1);
    assert_eq!(diff.changed_workloads.len(), 1);
    let rendered = diff.render_text();
    assert!(rendered.contains("+ runtime_function/messages:list"));
    assert!(rendered.contains("~ runtime_function/messages:send"));
    assert!(rendered.contains("services added: cache")); // 002-auth-caching-policy: service fixture name, not auth cache
    assert!(rendered.contains("network endpoints added: cache/redis")); // 002-auth-caching-policy: endpoint fixture name, not auth cache
    assert!(rendered.contains("Lifecycle: recreate_required"));
}

#[test]
fn policy_diff_classifies_egress_only_changes_as_dynamic_reload() {
    let from = parse_policy(EGRESS_DIFF_FROM);
    let to = parse_policy(EGRESS_DIFF_TO);

    let diff = OperatorPolicyDiff::between(&from, &to).expect("diff should evaluate");

    assert_eq!(diff.lifecycle(), OperatorPolicyLifecycle::DynamicReload);
    assert_eq!(diff.changed_workloads.len(), 1);
    assert_eq!(
        diff.changed_workloads[0].lifecycle,
        OperatorPolicyLifecycle::DynamicReload
    );
    let rendered = diff.render_text();
    assert!(rendered.contains("Lifecycle: dynamic_reload"));
    assert!(rendered.contains("sandbox egress added: github"));
}

#[test]
fn policy_diff_keeps_krun_egress_changes_recreate_required() {
    let from = parse_policy(&EGRESS_DIFF_FROM.replace("backend: container", "backend: krun"));
    let to = parse_policy(&EGRESS_DIFF_TO.replace("backend: container", "backend: krun"));

    let diff = OperatorPolicyDiff::between(&from, &to).expect("diff should evaluate");

    assert_eq!(diff.lifecycle(), OperatorPolicyLifecycle::RecreateRequired);
    assert_eq!(diff.changed_workloads.len(), 1);
    assert_eq!(
        diff.changed_workloads[0].lifecycle,
        OperatorPolicyLifecycle::RecreateRequired
    );
    let rendered = diff.render_text();
    assert!(rendered.contains("Lifecycle: recreate_required"));
    assert!(rendered.contains("sandbox egress added: github"));
}

#[test]
fn policy_diff_classifies_no_authority_change_as_dynamic_reload() {
    let policy = parse_policy(EGRESS_DIFF_FROM);

    let diff = OperatorPolicyDiff::between(&policy, &policy).expect("diff should evaluate");

    assert_eq!(diff.lifecycle(), OperatorPolicyLifecycle::DynamicReload);
    assert!(diff.added_workloads.is_empty());
    assert!(diff.removed_workloads.is_empty());
    assert!(diff.changed_workloads.is_empty());
    let rendered = diff.render_text();
    assert!(rendered.contains("Lifecycle: dynamic_reload"));
    assert!(rendered.contains("No authority changes."));
}

#[test]
fn policy_reload_keeps_last_known_good_after_invalid_candidate() {
    let mut reload = OperatorPolicyReloadState::new(parse_policy(EGRESS_DIFF_FROM))
        .expect("initial policy should evaluate");
    let original_ids = reload
        .evaluation()
        .decisions
        .iter()
        .map(|decision| decision.decision_id.clone())
        .collect::<Vec<_>>();

    let applied = reload.reload(parse_policy(EGRESS_DIFF_TO));
    assert!(
        applied.applied,
        "valid egress change should update desired policy"
    );
    assert_eq!(
        applied.lifecycle,
        Some(OperatorPolicyLifecycle::DynamicReload)
    );

    let rejected = reload.reload(parse_policy(INVALID_EGRESS_POLICY));
    assert!(!rejected.applied, "invalid reload should be rejected");
    assert!(
        rejected
            .error
            .as_deref()
            .is_some_and(|error| error.contains("wildcards")),
        "invalid reload should expose validation reason: {:?}",
        rejected.error
    );
    assert_ne!(
        rejected.active_decision_ids, original_ids,
        "last-known-good should remain the previously applied candidate, not roll back to the original"
    );
    assert_eq!(
        reload.evaluation().decisions[0].sandbox_egress.len(),
        2,
        "rejected reload should keep the last valid egress policy active"
    );
}

#[test]
fn policy_diff_reports_every_compiled_authority_delta_without_secret_handle_leaks() {
    let from = parse_policy(AUTHORITY_DIFF_FROM);
    let to = parse_policy(AUTHORITY_DIFF_TO);

    let diff = OperatorPolicyDiff::between(&from, &to).expect("diff should evaluate");

    assert_eq!(diff.changed_workloads.len(), 1);
    let rendered = diff.render_text();
    assert!(rendered.contains("volumes added: data"));
    assert!(rendered.contains("volumes removed: cache")); // 002-auth-caching-policy: volume fixture name, not auth cache
    assert!(rendered.contains("image allowed registries added: registry-b.example.com"));
    assert!(rendered.contains("image allowed registries removed: registry-a.example.com"));
    assert!(rendered.contains("image SBOM requirement changed: false -> true"));
    assert!(rendered.contains("secret handles changed: count 1 -> 1"));
    assert!(rendered.contains("quotas changed:"));
    assert!(rendered.contains("audit redactions added: query_params"));
    assert!(
        !rendered.contains("prod/db/password") && !rendered.contains("prod/cache/password"), // 002-auth-caching-policy: redacted secret-handle fixture, not auth cache
        "policy diff should not leak raw secret handles: {rendered}"
    );
}
