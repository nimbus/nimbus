use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

use crate::tenant_isolation::{
    TenantImageAdmissionSource, TenantImageVerificationEvidence, TenantImageVerificationProvider,
};

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
        - cache
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
        - prod/cache/password
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

struct NoopImageVerifier;

impl TenantImageVerificationProvider for NoopImageVerifier {
    fn verify_registry_image(
        &self,
        _image_reference: &str,
    ) -> Result<TenantImageVerificationEvidence> {
        Ok(TenantImageVerificationEvidence::new())
    }
}

fn parse_policy(body: &str) -> OperatorPolicyDocument {
    serde_yaml::from_str(body).expect("policy fixture should parse")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeExternalPolicyResponse {
    Allow,
    Deny,
    MalformedOutput,
    Timeout,
    Unavailable,
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
            FakeExternalPolicyResponse::Deny => Ok(OperatorExternalPolicyDecision::deny(
                self.name,
                self.version,
                "fixture policy denied",
            )),
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
            FakeExternalPolicyResponse::Unavailable => {
                Err(OperatorExternalPolicyBackendError::unavailable(
                    "fixture policy backend unavailable",
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
    let backend = FakeExternalPolicyBackend::fake_opa(FakeExternalPolicyResponse::Allow);

    let evaluation = policy
        .evaluate_with_external_policy(Some(&backend))
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
    assert_eq!(evidence.reason, "fixture policy allowed");
    let rendered = evaluation.render_explain_text();
    assert!(rendered.contains("external_policy: allow via fake-opa@v0-test"));
    assert!(rendered.contains("trace: external policy: allow via fake-opa@v0-test"));
}

#[test]
fn external_policy_backend_deny_fails_closed() {
    let policy = parse_policy(VALID_POLICY);
    let backend = FakeExternalPolicyBackend::fake_cedar(FakeExternalPolicyResponse::Deny);

    let error = policy
        .evaluate_with_external_policy(Some(&backend))
        .expect_err("external policy deny should fail closed");

    assert_eq!(backend.call_count(), 1);
    assert!(
        error.to_string().contains("fake-cedar@v0-test")
            && error.to_string().contains("denied workload")
            && error.to_string().contains("fixture policy denied"),
        "deny error should identify backend and reason: {error}"
    );
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
        let backend = FakeExternalPolicyBackend::fake_opa(response);

        let error = policy
            .evaluate_with_external_policy(Some(&backend))
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
fn built_in_hard_deny_precedes_external_policy_backend_allow() {
    let policy = parse_policy(INVALID_IMAGE);
    let backend = FakeExternalPolicyBackend::fake_opa(FakeExternalPolicyResponse::Allow);

    let error = policy
        .evaluate_with_external_policy(Some(&backend))
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
    assert!(rendered.contains("services added: cache"));
    assert!(rendered.contains("network endpoints added: cache/redis"));
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
    assert!(rendered.contains("volumes removed: cache"));
    assert!(rendered.contains("image allowed registries added: registry-b.example.com"));
    assert!(rendered.contains("image allowed registries removed: registry-a.example.com"));
    assert!(rendered.contains("image SBOM requirement changed: false -> true"));
    assert!(rendered.contains("secret handles changed: count 1 -> 1"));
    assert!(rendered.contains("quotas changed:"));
    assert!(rendered.contains("audit redactions added: query_params"));
    assert!(
        !rendered.contains("prod/db/password") && !rendered.contains("prod/cache/password"),
        "policy diff should not leak raw secret handles: {rendered}"
    );
}
