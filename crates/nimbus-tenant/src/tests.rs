use nimbus_core::{PrincipalContext, TenantId};
use nimbus_runtime::{
    RuntimeBackendKind, RuntimeBundle, RuntimeLimits, RuntimePolicy, RuntimeProfile,
};
use nimbus_sandbox::{
    PublishedEndpointProtocol, SandboxBackendKind, SandboxOwnerSpec, SandboxProcessSpec,
    SandboxResourceCharge, SandboxRootSpec, SandboxSpec,
};

use super::*;

fn sparse_spec(tenant: &str, name: &str, backend: SandboxBackendKind) -> SandboxSpec {
    SandboxSpec::new(
        TenantId::new(tenant).expect("tenant id should parse"),
        SandboxOwnerSpec::service(name),
        backend,
        SandboxRootSpec::rootfs(""),
        SandboxProcessSpec::new(Vec::<String>::new()),
    )
}

fn tenant_labeled_bundle(tenant: &str) -> RuntimeBundle {
    RuntimeBundle::for_tenant(
        "bundle.mjs",
        "0000000000000000000000000000000000000000000000000000000000000000",
        tenant,
    )
    .expect("test runtime bundle should build")
}

fn test_application_context() -> TenantIsolationContext {
    TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        PrincipalContext::anonymous(),
        "test",
    )
}

fn tenant_decision_input(
    context: &TenantIsolationContext,
    policy: &RuntimePolicy,
) -> TenantIsolationPolicyInput {
    TenantIsolationPolicyInput::new(
            WorkloadAttributes::runtime_function(
                "messages:send",
                RuntimeIsolationTier::InProcessUntrusted,
            )
            .with_invocation_id("invoke-1"),
        )
        .with_runtime_policy(
            context,
            policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        )
        .with_services(TenantServiceGrantPolicyDecision::new(["db"]))
        .with_network(TenantNetworkPolicyDecision::new([
            TenantNetworkEndpointDecision::new(
                "db",
                "postgres",
                PublishedEndpointProtocol::Tcp,
                "127.0.0.1",
                15432,
            )
            .with_guest_port(5432),
        ]))
        .with_storage(TenantStoragePolicyDecision::namespace("tenant-a"))
        .with_volumes(TenantVolumePolicyDecision::new(["cache"])) // 002-auth-caching-policy: volume fixture name, not auth cache
        .with_image(TenantImagePolicyDecision::digest_pinned(
            "registry.example.com/app@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ))
        .with_secrets(TenantSecretPolicyDecision::handles(["prod/db/password"]))
        .with_quotas(
            TenantQuotaPolicyDecision::default()
                .with_runtime_budget(policy.tenant_budget())
                .with_sandbox_charge(SandboxResourceCharge {
                    active_sandboxes: 1,
                    vcpus: 1,
                    memory_bytes: 512 * 1024 * 1024,
                    disk_bytes: 10 * 1024 * 1024 * 1024,
                    log_bytes: 64 * 1024 * 1024,
                }),
        )
}

fn principal_with_tenant_claim(claim: &'static str, tenant: &str) -> PrincipalContext {
    PrincipalContext {
        authenticated: true,
        claims: serde_json::Map::from_iter([(
            claim.to_string(),
            serde_json::Value::String(tenant.to_string()),
        )]),
        verified_claims: serde_json::Map::new(),
    }
}

fn production_untrusted_route(policy: &RuntimePolicy) -> RuntimeIsolationRoute {
    let context = test_application_context();
    match context.admit_runtime_policy(
        policy,
        RuntimeIsolationTier::InProcessUntrusted,
        TenantIsolationMode::Production,
    ) {
        RuntimePolicyAdmission::Route(route) => route,
        RuntimePolicyAdmission::AdmitInProcess => {
            panic!("policy should have produced a runtime isolation route")
        }
    }
}

fn runtime_policy_decision(
    policy: &RuntimePolicy,
    tier: RuntimeIsolationTier,
    mode: TenantIsolationMode,
) -> TenantRuntimePolicyDecision {
    let context = test_application_context();
    let admission = context.admit_runtime_policy(policy, tier, mode);
    TenantRuntimePolicyDecision::from_runtime_policy(policy, tier, mode, admission)
}

#[test]
fn runtime_efficiency_plan_classifies_web_and_node_after_admission_without_changing_axes() {
    for (limits, expected_profile) in [
        (
            RuntimeLimits::application_web_standard(),
            RuntimeProfile::WebLean,
        ),
        (
            RuntimeLimits::application_node22(),
            RuntimeProfile::NodeFull,
        ),
    ] {
        let normalized = limits.normalized();
        let policy = RuntimePolicy::new(normalized.clone());
        let decision = runtime_policy_decision(
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        );
        let plan = decision.runtime_efficiency_plan(&normalized);

        assert_eq!(plan.profile(), Some(expected_profile));
        assert_eq!(
            plan.state(),
            RuntimeEfficiencyPlanState::FlagOffCurrentBehavior,
            "PIR1 is classification-only; current behavior remains effective"
        );
        assert_eq!(plan.effective_pool_kind(), normalized.runtime_pool_kind);
        assert_eq!(plan.effective_execution_model(), normalized.execution_model);
        assert_eq!(policy.limits(), &normalized);
        assert_eq!(
            decision.admission(),
            &TenantRuntimePolicyAdmission::AdmitInProcess
        );
    }
}

#[test]
fn runtime_efficiency_plan_never_downgrades_escalated_or_unsupported_surfaces() {
    let service_limits = RuntimeLimits::application_node22_service_microvm().normalized();
    let service_policy = RuntimePolicy::new(service_limits.clone());
    let service_decision = runtime_policy_decision(
        &service_policy,
        RuntimeIsolationTier::MicroVmService,
        TenantIsolationMode::Production,
    );
    let service_plan = service_decision.runtime_efficiency_plan(&service_limits);
    assert_eq!(service_plan.profile(), Some(RuntimeProfile::NodeFull));
    assert_eq!(
        service_plan.state(),
        RuntimeEfficiencyPlanState::EscalatedOrRouted,
        "a microVM/service tier remains outside in-process efficiency selection"
    );
    assert!(
        service_decision
            .grants()
            .net_listen
            .contains(&"[::]".to_string()),
        "classification must not strip service/microVM grants"
    );

    let routed_limits = RuntimeLimits::application_node22_local_development().normalized();
    let routed_policy = RuntimePolicy::new(routed_limits.clone());
    let routed_decision = runtime_policy_decision(
        &routed_policy,
        RuntimeIsolationTier::InProcessUntrusted,
        TenantIsolationMode::Production,
    );
    let routed_plan = routed_decision.runtime_efficiency_plan(&routed_limits);
    assert!(matches!(
        routed_decision.admission(),
        TenantRuntimePolicyAdmission::Route { .. }
    ));
    assert_eq!(routed_plan.profile(), Some(RuntimeProfile::NodeFull));
    assert_eq!(
        routed_plan.state(),
        RuntimeEfficiencyPlanState::EscalatedOrRouted,
        "a production route/rejection cannot be re-admitted by profile classification"
    );

    let bun_limits = RuntimeLimits::application_bun_jsc().normalized();
    let bun_policy = RuntimePolicy::new(bun_limits.clone());
    let bun_decision = runtime_policy_decision(
        &bun_policy,
        RuntimeIsolationTier::InProcessUntrusted,
        TenantIsolationMode::Production,
    );
    let bun_plan = bun_decision.runtime_efficiency_plan(&bun_limits);
    assert_eq!(bun_plan.profile(), None);
    assert_eq!(
        bun_plan.state(),
        RuntimeEfficiencyPlanState::UnsupportedSurface,
        "PIR1 does not collapse Bun/JSC into a V8 WebLean profile"
    );
}

#[test]
fn tenant_isolation_decision_has_stable_decision_id_and_audit_safe_redaction() {
    let principal = PrincipalContext {
        authenticated: true,
        claims: serde_json::Map::from_iter([
            (
                "tenant_id".to_string(),
                serde_json::Value::String("tenant-a".to_string()),
            ),
            (
                "email".to_string(),
                serde_json::Value::String("operator@example.com".to_string()),
            ),
        ]),
        verified_claims: serde_json::Map::new(),
    };
    let context = TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        principal,
        "convex.runtime",
    )
    .with_deployment_generation(7);
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let input = tenant_decision_input(&context, &policy);

    let decision = context
        .admit_decision(input.clone())
        .expect("decision should admit matching tenant authority");
    let same_decision = context
        .admit_decision(input)
        .expect("same decision inputs should admit again");

    assert_eq!(
        decision.id(),
        same_decision.id(),
        "decision IDs must be stable for identical admitted inputs"
    );
    assert_eq!(decision.tenant_id().as_str(), "tenant-a");
    assert_eq!(decision.workload().name(), "messages:send");
    assert_eq!(decision.storage().namespace_name(), "tenant-a");
    assert_eq!(decision.network().endpoints().len(), 1);
    assert!(matches!(
        decision.runtime().admission(),
        TenantRuntimePolicyAdmission::AdmitInProcess
    ));

    let audit = decision.to_audit_record();
    let serialized = serde_json::to_string(&audit).expect("audit record should serialize to JSON");
    assert!(
        serialized.contains(decision.id().as_str()),
        "audit record should carry the decision ID: {serialized}"
    );
    assert!(
        serialized.contains("\"secret_handle_count\":1"),
        "audit record should expose secret counts without handles: {serialized}"
    );
    assert!(
        !serialized.contains("prod/db/password"),
        "audit record must not leak raw secret handles: {serialized}"
    );
    assert!(
        !serialized.contains("operator@example.com"),
        "audit record must not leak principal claims: {serialized}"
    );
    assert!(
        decision
            .audit_redactions()
            .redacted_fields()
            .contains(&"secret_handles".to_string()),
        "decision should advertise secret-handle redaction"
    );
    assert!(
        decision
            .audit_redactions()
            .redacted_fields()
            .contains(&"principal_claims".to_string()),
        "decision should advertise principal-claim redaction"
    );

    let changed_workload = TenantIsolationPolicyInput::new(
        WorkloadAttributes::runtime_function(
            "messages:list",
            RuntimeIsolationTier::InProcessUntrusted,
        )
        .with_invocation_id("invoke-1"),
    )
    .with_runtime_policy(
        &context,
        &policy,
        RuntimeIsolationTier::InProcessUntrusted,
        TenantIsolationMode::Production,
    );
    let changed_decision = context
        .admit_decision(changed_workload)
        .expect("changed workload should still admit");
    assert_ne!(
        decision.id(),
        changed_decision.id(),
        "decision ID must change when workload attributes change"
    );
}

#[test]
fn workload_identity_splits_subject_from_audit_projection() {
    let principal = principal_with_tenant_claim("tenant_id", "tenant-a");
    let context = TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        principal,
        "convex.runtime",
    )
    .with_deployment_generation(7)
    .with_workload_location(
        WorkloadLocation::new()
            .with_node_id("node-a")
            .with_machine_id("default"),
    );
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let decision = context
        .admit_decision(tenant_decision_input(&context, &policy))
        .expect("decision should admit matching tenant authority");

    let identity = decision.workload_identity();

    assert_eq!(identity.tenant_id(), "tenant-a");
    assert_eq!(identity.deployment_generation(), Some(7));
    assert_eq!(identity.node_id(), Some("node-a"));
    assert_eq!(identity.machine_id(), Some("default"));
    assert_eq!(
        identity.subject(),
        "nimbus-workload:v1/tenant/tenant-a/deployment/7/surface/convex.runtime/kind/runtime_function/name/messages%3Asend/runtime-tier/in_process_untrusted/runtime-backend/v8/sandbox-backend/none"
    );
    assert_eq!(
        identity.audit_projection(),
        "nimbus-workload-audit:v1/tenant/tenant-a/deployment/7/surface/convex.runtime/kind/runtime_function/name/messages%3Asend/runtime-tier/in_process_untrusted/runtime-backend/v8/sandbox-backend/none/node/node-a/machine/default/sandbox/none/invocation/invoke-1"
    );
    assert_eq!(
        identity.spiffe_path(),
        "/nimbus/workload/v1/tenant/tenant-a/deployment/7/surface/convex.runtime/kind/runtime_function/name/messages%3Asend/runtime-tier/in_process_untrusted/runtime-backend/v8/sandbox-backend/none"
    );
    assert_eq!(
        identity.audit_projection_path(),
        "/nimbus/workload-audit/v1/tenant/tenant-a/deployment/7/surface/convex.runtime/kind/runtime_function/name/messages%3Asend/runtime-tier/in_process_untrusted/runtime-backend/v8/sandbox-backend/none/node/node-a/machine/default/sandbox/none/invocation/invoke-1"
    );
    assert_eq!(
        identity
            .spiffe_id("nimbus.local")
            .expect("trust domain should be valid"),
        "spiffe://nimbus.local/nimbus/workload/v1/tenant/tenant-a/deployment/7/surface/convex.runtime/kind/runtime_function/name/messages%3Asend/runtime-tier/in_process_untrusted/runtime-backend/v8/sandbox-backend/none"
    );

    let audit_json =
        serde_json::to_string(&decision.to_audit_record()).expect("audit record should serialize");
    assert!(
        audit_json.contains("\"workload_subject\""),
        "audit record should expose the canonical workload identity: {audit_json}"
    );
    assert!(
        audit_json.contains("\"workload_audit_projection\""),
        "audit record should expose the full placement/invocation projection: {audit_json}"
    );
    assert!(
        audit_json.contains("messages%3Asend"),
        "audit record should use the stable escaped workload name: {audit_json}"
    );
}

#[test]
fn workload_identity_labels_wasmtime_runtime_backend() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_wasm_component());
    let input = TenantIsolationPolicyInput::new(WorkloadAttributes::runtime_function(
        "agent:tick",
        RuntimeIsolationTier::InProcessUntrusted,
    ))
    .with_runtime_policy(
        &context,
        &policy,
        RuntimeIsolationTier::InProcessUntrusted,
        TenantIsolationMode::LocalDevelopment,
    );
    let decision = context
        .admit_decision(input)
        .expect("local development Wasmtime policy should admit for identity derivation");

    assert_eq!(
        decision.runtime().backend_kind(),
        RuntimeBackendKind::Wasmtime
    );
    assert_eq!(
        decision.workload_identity().subject(),
        "nimbus-workload:v1/tenant/tenant-a/deployment/none/surface/test/kind/runtime_function/name/agent%3Atick/runtime-tier/in_process_untrusted/runtime-backend/wasmtime/sandbox-backend/none"
    );
}

#[test]
fn workload_identity_distinguishes_sandbox_backend_and_location() {
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let context_a = test_application_context()
        .with_deployment_generation(9)
        .with_workload_location(
            WorkloadLocation::new()
                .with_node_id("node-a")
                .with_machine_id("machine-a"),
        );
    let context_b = test_application_context()
        .with_deployment_generation(9)
        .with_workload_location(
            WorkloadLocation::new()
                .with_node_id("node-b")
                .with_machine_id("machine-b"),
        );
    let input = TenantIsolationPolicyInput::new(
        WorkloadAttributes::service("db:primary")
            .with_sandbox_id("sandbox-1")
            .with_sandbox_backend(SandboxBackendKind::Krun),
    )
    .with_runtime_policy(
        &context_a,
        &policy,
        RuntimeIsolationTier::MicroVmService,
        TenantIsolationMode::Production,
    );

    let decision_a = context_a
        .admit_decision(input.clone())
        .expect("first location should admit");
    let decision_b = context_b
        .admit_decision(input)
        .expect("second location should admit");

    assert_ne!(
        decision_a.id(),
        decision_b.id(),
        "location must participate in the immutable decision fingerprint"
    );
    assert_eq!(
        decision_a.workload_identity().subject(),
        "nimbus-workload:v1/tenant/tenant-a/deployment/9/surface/test/kind/service/name/db%3Aprimary/runtime-tier/none/runtime-backend/none/sandbox-backend/krun"
    );
    assert_eq!(
        decision_b.workload_identity().subject(),
        "nimbus-workload:v1/tenant/tenant-a/deployment/9/surface/test/kind/service/name/db%3Aprimary/runtime-tier/none/runtime-backend/none/sandbox-backend/krun"
    );
    assert_ne!(
        decision_a.workload_identity().audit_projection(),
        decision_b.workload_identity().audit_projection(),
        "audit projection must retain placement for evidence correlation"
    );
}

#[test]
fn workload_identity_rejects_invalid_spiffe_trust_domains() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let decision = context
        .admit_decision(tenant_decision_input(&context, &policy))
        .expect("decision should admit");
    let identity = decision.workload_identity();

    for trust_domain in [
        "",
        "spiffe://nimbus.local",
        "nimbus.local/path",
        "nimbus local",
    ] {
        let error = identity
            .spiffe_id(trust_domain)
            .expect_err("invalid trust domain should be rejected");
        assert!(
            error.to_string().contains("SPIFFE trust domain"),
            "error should name the invalid trust-domain field: {error}"
        );
    }
}

#[test]
fn tenant_isolation_decision_clones_inputs_so_policy_cannot_widen_after_admission() {
    let context = test_application_context().with_deployment_generation(11);
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let mut input = tenant_decision_input(&context, &policy);

    let decision = context
        .admit_decision(input.clone())
        .expect("decision should admit initial policy input");

    input.services.services.push("other-tenant-db".to_string());
    input
        .network
        .endpoints
        .push(TenantNetworkEndpointDecision::new(
            "other-tenant-db",
            "postgres",
            PublishedEndpointProtocol::Tcp,
            "127.0.0.1",
            25432,
        ));
    input
        .volumes
        .named_volumes
        .push("other-tenant-cache".to_string()); // 002-auth-caching-policy: volume fixture name, not auth cache
    input.runtime.grants.run.push("npm".to_string());

    assert_eq!(
        decision.services().services(),
        &["db".to_string()],
        "admitted service grants should be immutable snapshots"
    );
    assert_eq!(
        decision.network().endpoints().len(),
        1,
        "admitted endpoint grants should be immutable snapshots"
    );
    assert_eq!(
        decision.volumes().named_volumes(),
        &["cache".to_string()], // 002-auth-caching-policy: volume fixture name, not auth cache
        "admitted volume grants should be immutable snapshots"
    );
    assert!(
        decision.runtime().grants().run.is_empty(),
        "admitted runtime grants should not widen after input mutation"
    );

    let tenant_b = TenantId::new("tenant-b").expect("tenant id should parse");
    let error = decision
        .ensure_tenant_matches(&tenant_b, "lower seam forged tenant")
        .expect_err("decision must remain tenant-bound");
    assert!(
        error.to_string().contains("authorized tenant tenant-a"),
        "error should name the admitted tenant: {error}"
    );

    let error = decision
        .ensure_deployment_generation_matches(12, "stale runtime invocation")
        .expect_err("decision must remain deployment-bound");
    assert!(
        error
            .to_string()
            .contains("authorized deployment generation 11"),
        "error should name the admitted deployment generation: {error}"
    );
}

#[test]
fn tenant_isolation_decision_issues_narrow_service_and_storage_access() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let decision = context
        .admit_decision(tenant_decision_input(&context, &policy))
        .expect("decision should admit");

    let service = decision
        .service_access("db", "host bridge service lookup")
        .expect("admitted service should receive a narrow access decision");
    assert_eq!(service.decision_id(), decision.id());
    assert_eq!(service.tenant_id().as_str(), "tenant-a");
    assert_eq!(service.service_name(), "db");

    let storage = decision.storage_access();
    assert_eq!(storage.decision_id(), decision.id());
    assert_eq!(storage.tenant_id().as_str(), "tenant-a");
    assert_eq!(storage.namespace_name(), "tenant-a");

    let tenant_b = TenantId::new("tenant-b").expect("tenant id should parse");
    let error = storage
        .ensure_tenant_matches(&tenant_b, "runtime storage host operation")
        .expect_err("storage projection must reject a forged lower-seam tenant");
    assert!(
        error.to_string().contains("authorized tenant tenant-a"),
        "error should name the admitted tenant: {error}"
    );
}

#[test]
fn tenant_isolation_decision_rejects_unadmitted_service_grants() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let decision = context
        .admit_decision(tenant_decision_input(&context, &policy))
        .expect("decision should admit");

    let error = decision
        .service_access("other-tenant-db", "host bridge service lookup")
        .expect_err("service access must be limited to the admitted grant set");
    assert!(
        error.to_string().contains("permission denied"),
        "error should map to permission denial: {error}"
    );
    assert!(
        error
            .to_string()
            .contains("did not authorize service `other-tenant-db`"),
        "error should name the rejected service: {error}"
    );
}

#[test]
fn tenant_isolation_decision_rejects_mismatched_application_claims() {
    let context = TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        principal_with_tenant_claim("tenant_id", "tenant-b"),
        "convex.runtime",
    );
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let input = tenant_decision_input(&context, &policy);

    let error = context
        .admit_decision(input)
        .expect_err("mismatched application claims must not receive a decision");

    assert!(
        error.to_string().contains("permission denied"),
        "error should map to permission denial: {error}"
    );
    assert!(
        error.to_string().contains("tenant `tenant-b`"),
        "error should name the claimed tenant: {error}"
    );
}

#[test]
fn tenant_isolation_mode_defaults_to_production() {
    assert_eq!(
        TenantIsolationMode::default(),
        TenantIsolationMode::Production
    );
}

#[test]
fn tenant_isolation_decision_rejects_mismatched_sandbox_launch() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let decision = context
        .admit_decision(tenant_decision_input(&context, &policy))
        .expect("decision should admit");
    let service = decision
        .service_access("db", "sandbox-backed service launch")
        .expect("db service should be admitted");
    let spec = sparse_spec("tenant-b", "db", SandboxBackendKind::Krun);

    let error = service
        .ensure_sandbox_spec_matches(&spec, SandboxBackendKind::Krun)
        .expect_err("service projection must reject a forged launch tenant");
    assert!(
        error.to_string().contains("authorized tenant tenant-a"),
        "error should name the admitted tenant: {error}"
    );
    assert!(
        error.to_string().contains("referenced tenant tenant-b"),
        "error should name the forged tenant: {error}"
    );
}

#[test]
fn tenant_isolation_decision_rejects_mismatched_runtime_bundle_before_invocation() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let decision = context
        .admit_decision(tenant_decision_input(&context, &policy))
        .expect("decision should admit");
    let bundle = tenant_labeled_bundle("tenant-b");

    let error = decision
        .ensure_runtime_bundle_matches(&bundle, "runtime bundle")
        .expect_err("decision must reject a forged runtime bundle tenant");
    assert!(
        error.to_string().contains("authorized tenant tenant-a"),
        "error should name the authorized tenant: {error}"
    );
    assert!(
        error.to_string().contains("referenced tenant tenant-b"),
        "error should name the rejected tenant: {error}"
    );
}

#[test]
fn tenant_isolation_decision_rejects_mismatched_service_before_launch() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let decision = context
        .admit_decision(tenant_decision_input(&context, &policy))
        .expect("decision should admit");
    let service = decision
        .service_access("db", "sandbox-backed service launch")
        .expect("db service should be admitted");
    let spec = sparse_spec("tenant-a", "cache", SandboxBackendKind::Krun); // 002-auth-caching-policy: service fixture name, not auth cache

    let error = service
        .ensure_sandbox_spec_matches(&spec, SandboxBackendKind::Krun)
        .expect_err("mismatched service name must be rejected before sandbox launch");
    assert!(
        error.to_string().contains("authorized service db"),
        "error should name the rejected service: {error}"
    );
}

#[test]
fn tenant_isolation_decision_rejects_mismatched_backend_before_launch() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let decision = context
        .admit_decision(tenant_decision_input(&context, &policy))
        .expect("decision should admit");
    let service = decision
        .service_access("db", "sandbox-backed service launch")
        .expect("db service should be admitted");
    let spec = sparse_spec("tenant-a", "db", SandboxBackendKind::Container);

    let error = service
        .ensure_sandbox_spec_matches(&spec, SandboxBackendKind::Krun)
        .expect_err("mismatched backend must be rejected before sandbox launch");
    assert!(
        error.to_string().contains("requested backend Container"),
        "error should name the rejected backend: {error}"
    );
}

#[test]
fn tenant_context_rejects_mismatched_runtime_bundle_before_invocation() {
    let context = TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        PrincipalContext::anonymous(),
        "test",
    );
    let bundle = tenant_labeled_bundle("tenant-b");

    let error = context
        .ensure_runtime_bundle_matches(&bundle, "runtime bundle")
        .expect_err("mismatched runtime bundle tenant must be rejected before invocation");
    assert!(
        error.to_string().contains("authorized tenant tenant-a"),
        "error should name the authorized tenant: {error}"
    );
    assert!(
        error.to_string().contains("referenced tenant tenant-b"),
        "error should name the rejected tenant: {error}"
    );
}

#[test]
fn application_context_rejects_mismatched_principal_tenant_claim() {
    let context = TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        principal_with_tenant_claim("tenant_id", "tenant-b"),
        "test",
    );

    let error = context
        .admit_if_principal_claim_absent_or_matching("convex route tenant")
        .expect_err("mismatched application tenant claim must be rejected");
    assert!(
        error.to_string().contains("permission denied"),
        "error should map to permission denial: {error}"
    );
    assert!(
        error.to_string().contains("authorizes tenant `tenant-b`"),
        "error should name the authorized tenant claim: {error}"
    );
    assert!(
        error.to_string().contains("targeted tenant `tenant-a`"),
        "error should name the rejected target tenant: {error}"
    );
}

#[test]
fn application_context_allows_matching_verified_principal_tenant_claim() {
    let mut principal = principal_with_tenant_claim("tenant_id", "tenant-b");
    principal.verified_claims = serde_json::Map::from_iter([(
        "tenantId".to_string(),
        serde_json::Value::String("tenant-a".to_string()),
    )]);
    let context = TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        principal,
        "test",
    );

    context
        .admit_if_principal_claim_absent_or_matching("convex route tenant")
        .expect("verified tenant claim should take precedence and authorize access");
}

#[test]
fn application_context_can_require_tenant_claim_for_control_plane_routes() {
    let context = TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        PrincipalContext {
            authenticated: true,
            claims: serde_json::Map::new(),
            verified_claims: serde_json::Map::new(),
        },
        "test",
    );

    context
        .admit_if_principal_claim_absent_or_matching("convex route tenant")
        .expect("generic adapter routes may accept principals without tenant claims");
    let error = context
        .require_matching_principal_claim("service lifecycle route")
        .expect_err("service control routes must require a tenant claim");
    assert!(
        error.to_string().contains("has no tenant claim"),
        "error should explain the missing tenant claim: {error}"
    );
    assert!(
        error.to_string().contains("targeted tenant `tenant-a`"),
        "error should name the targeted tenant: {error}"
    );
}

#[test]
fn tenant_context_rejects_mismatched_deployment_before_invocation() {
    let context = TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        PrincipalContext::anonymous(),
        "test",
    )
    .with_deployment_generation(7);

    let error = context
        .ensure_deployment_generation_matches(8, "runtime invocation")
        .expect_err("mismatched deployment generation must be rejected");
    assert!(
        error
            .to_string()
            .contains("authorized deployment generation 7"),
        "error should name the authorized deployment generation: {error}"
    );
    assert!(
        error
            .to_string()
            .contains("referenced deployment generation 8"),
        "error should name the rejected deployment generation: {error}"
    );
}

#[test]
fn reauthorized_application_context_preserves_tenant_and_deployment_generation() {
    let context = TenantIsolationContext::application(
        TenantId::new("tenant-a").expect("tenant id should parse"),
        PrincipalContext::anonymous(),
        "convex websocket route",
    )
    .with_deployment_generation(42)
    .with_workload_location(
        WorkloadLocation::new()
            .with_node_id("node-source")
            .with_machine_id("machine-source"),
    );

    let derived =
        context.reauthorize_application(PrincipalContext::system(), "convex subscription");

    derived
        .ensure_tenant_matches(
            &TenantId::new("tenant-a").expect("tenant id should parse"),
            "derived context tenant",
        )
        .expect("derived context should preserve tenant identity");
    derived
        .ensure_deployment_generation_matches(42, "derived context deployment")
        .expect("derived context should preserve deployment generation");
    let error = derived
        .ensure_deployment_generation_matches(43, "stale subscription runtime")
        .expect_err("derived context must still reject stale deployment generations");
    assert!(
        error
            .to_string()
            .contains("authorized deployment generation 42"),
        "error should name the preserved deployment generation: {error}"
    );

    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());
    let decision = derived
        .admit_decision(tenant_decision_input(&derived, &policy))
        .expect("derived context should still admit matching tenant");
    let identity = decision.workload_identity();
    assert_eq!(identity.node_id(), Some("node-source"));
    assert_eq!(identity.machine_id(), Some("machine-source"));
}

#[test]
fn production_untrusted_runtime_admission_allows_web_standard_application_policy() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_web_standard());

    assert_eq!(
        context.admit_runtime_policy(
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        ),
        RuntimePolicyAdmission::AdmitInProcess,
        "web-standard application grants should be production-admissible"
    );
}

#[test]
fn production_untrusted_runtime_admission_allows_bun_jsc_fresh_discard_policy() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_bun_jsc());

    assert_eq!(
        context.admit_runtime_policy(
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        ),
        RuntimePolicyAdmission::AdmitInProcess,
        "Bun/JSC fresh/discard policy is the proven in-process admission profile"
    );
}

#[test]
fn production_untrusted_runtime_admission_allows_production_node_profile() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_node22());

    assert_eq!(
        context.admit_runtime_policy(
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::Production,
        ),
        RuntimePolicyAdmission::AdmitInProcess,
        "production Node profile should be in-process admissible because it has no broad host grants"
    );
}

#[test]
fn production_untrusted_runtime_admission_rejects_local_development_node_grants() {
    let policy = RuntimePolicy::new(RuntimeLimits::application_node22_local_development());

    let route = production_untrusted_route(&policy);

    assert!(
        route.reason().contains("generic localhost"),
        "route should explain loopback authority: {route:?}"
    );
    assert_eq!(
        route.recommended_tier(),
        RuntimeIsolationTier::MicroVmService,
        "route should name the canonical routing fallback"
    );
}

#[test]
fn production_untrusted_runtime_admission_rejects_node_tls_disable_env_grant() {
    let policy = RuntimePolicy::new(RuntimeLimits {
        grants: nimbus_runtime::RuntimeGrants {
            env_read: vec![
                "NODE_ENV".to_string(),
                "NODE_TLS_REJECT_UNAUTHORIZED".to_string(),
            ],
            ..nimbus_runtime::RuntimeGrants::application_node_production_in_process()
        },
        ..RuntimeLimits::application_node22()
    });

    let route = production_untrusted_route(&policy);

    assert_eq!(
        route.recommended_tier(),
        RuntimeIsolationTier::InProcessTrustedOnly
    );
    assert!(
        route.reason().contains("NODE_TLS_REJECT_UNAUTHORIZED"),
        "route should explain the ambient TLS-disable env authority: {route:?}"
    );
}

#[test]
fn production_untrusted_runtime_admission_routes_package_manager_run_to_microvm() {
    let policy = RuntimePolicy::new(RuntimeLimits {
        grants: nimbus_runtime::RuntimeGrants {
            run: vec!["npm".to_string()],
            ..nimbus_runtime::RuntimeGrants::application_web_standard()
        },
        ..RuntimeLimits::application_node22()
    });

    let route = production_untrusted_route(&policy);

    assert_eq!(
        route.recommended_tier(),
        RuntimeIsolationTier::MicroVmService
    );
    assert!(
        route.reason().contains("run grants `npm`"),
        "route should name the package-manager subprocess authority: {route:?}"
    );
}

#[test]
fn production_untrusted_runtime_admission_routes_native_addon_package_loading_to_microvm() {
    let policy = RuntimePolicy::new(RuntimeLimits {
        grants: nimbus_runtime::RuntimeGrants {
            read: vec![
                "$generated_root".to_string(),
                "$app_root".to_string(),
                "$cache_root".to_string(), // 002-auth-caching-policy: filesystem grant name, not auth cache
            ],
            ..nimbus_runtime::RuntimeGrants::application_web_standard()
        },
        ..RuntimeLimits::application_web_standard()
    });

    let route = production_untrusted_route(&policy);

    assert_eq!(
        route.recommended_tier(),
        RuntimeIsolationTier::MicroVmService
    );
    assert!(
        route.reason().contains("broad filesystem/package-loading"),
        "route should name the native-addon/package-loading authority: {route:?}"
    );
}

#[test]
fn production_untrusted_runtime_admission_routes_trusted_grants_to_trusted_tier() {
    let policy = RuntimePolicy::new(RuntimeLimits {
        grants: nimbus_runtime::RuntimeGrants {
            env_write: vec!["DEBUG".to_string()],
            ..nimbus_runtime::RuntimeGrants::application_web_standard()
        },
        ..RuntimeLimits::application_web_standard()
    });

    let route = production_untrusted_route(&policy);

    assert!(
        route.reason().contains("env_write"),
        "route should explain the rejected grant family: {route:?}"
    );
    assert_eq!(
        route.recommended_tier(),
        RuntimeIsolationTier::InProcessTrustedOnly,
        "route should name the trusted-only routing fallback"
    );
}

#[test]
fn production_admission_only_validates_in_process_untrusted_tier() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_node22_service_microvm());

    assert_eq!(
        context.admit_runtime_policy(
            &policy,
            RuntimeIsolationTier::MicroVmService,
            TenantIsolationMode::Production,
        ),
        RuntimePolicyAdmission::AdmitInProcess,
        "microVM service routing owns OS isolation outside the in-process gate"
    );
}

#[test]
fn local_development_runtime_admission_preserves_node_compatibility_policy() {
    let context = test_application_context();
    let policy = RuntimePolicy::new(RuntimeLimits::application_node22_local_development());

    assert_eq!(
        context.admit_runtime_policy(
            &policy,
            RuntimeIsolationTier::InProcessUntrusted,
            TenantIsolationMode::LocalDevelopment,
        ),
        RuntimePolicyAdmission::AdmitInProcess,
        "local development mode should preserve Node compatibility localhost grants"
    );
}
