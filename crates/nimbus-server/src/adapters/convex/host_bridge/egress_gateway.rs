use nimbus_runtime::{
    EgressAuthorization as RuntimeEgressAuthorization, EgressGateway,
    EgressRequest as RuntimeEgressRequest,
};

// The readiness latch and the runtime<->PDP egress bridging live in
// `nimbus-bridge` so every adapter host bridge (Convex, Cloud Functions, and
// any future runtime adapter) resolves the identical verdict from one code
// path — "three substrates, one decision" — instead of re-encoding the policy
// translation per adapter. (audit M13.)
pub(in crate::adapters::convex) use nimbus_bridge::egress::EgressGatewayEnforcementReadiness;

use super::bridge::ConvexHostBridge;

impl EgressGateway for ConvexHostBridge {
    fn authorize(&self, request: &RuntimeEgressRequest) -> RuntimeEgressAuthorization {
        nimbus_bridge::egress::authorize_runtime_egress(
            self.decision(),
            self.egress_readiness(),
            request,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus_core::{PrincipalContext, TenantId};
    use nimbus_egress::{EgressCredentialInjection, EgressPolicy, EgressProtocol, EgressRule};
    use nimbus_engine::Engine;
    use nimbus_runtime::{
        EgressProtocol as RuntimeEgressProtocol, EgressRequest as RuntimeEgressRequest,
        EgressSubstrate, InvocationKind, RuntimePolicy,
    };
    use nimbus_services::ServiceInstanceBindingRegistry;
    use nimbus_tenant::{
        RuntimeIsolationTier, TenantIsolationContext, TenantIsolationDecision, TenantIsolationMode,
        TenantIsolationPolicyInput, TenantNetworkPolicyDecision, TenantStoragePolicyDecision,
        WorkloadAttributes,
    };
    use tempfile::{TempDir, tempdir};

    use crate::adapters::convex::{
        ConvexHostBridge, ConvexHostBridgeInvocation, ConvexHostBridgeScope, ConvexRegistry,
    };
    use nimbus_services::EmptyServiceInstanceCatalog;

    use super::*;

    #[test]
    fn egress_gateway_cross_substrate_parity_denies_and_allows_same_policy() {
        let (_tempdir, bridge) = bridge_for_policy(
            "tenant-parity",
            EgressPolicy::new([EgressRule::new(
                "api-internal",
                EgressProtocol::Https,
                "api.internal",
                443,
            )
            .with_methods(["GET"])
            .with_path_prefixes(["/v1"])]),
            None,
        );

        let isolate_deny = bridge.authorize(&runtime_request(
            "tenant-parity",
            EgressSubstrate::Isolate,
            "evil.example",
            "/v1/steal",
        ));
        let container_deny = bridge
            .decision()
            .network()
            .authorize_sandbox_egress(&container_request("evil.example", "/v1/steal"));
        assert!(!isolate_deny.is_allowed());
        assert!(!container_deny.is_allowed());
        assert_eq!(isolate_deny.reason(), container_deny.reason());

        let isolate_allow = bridge.authorize(&runtime_request(
            "tenant-parity",
            EgressSubstrate::Isolate,
            "api.internal",
            "/v1/messages",
        ));
        let container_allow = bridge
            .decision()
            .network()
            .authorize_sandbox_egress(&container_request("api.internal", "/v1/messages"));
        assert!(isolate_allow.is_allowed());
        assert!(container_allow.is_allowed());
        assert_eq!(
            isolate_allow.matched_rule(),
            container_allow.matched_rule(),
            "isolate and container substrates must resolve the same matched rule"
        );
    }

    #[test]
    fn egress_gateway_propagates_proxy_enforcement_requirement() {
        // A rule whose enforcement depends on the nimbus-proxy PEP (credential
        // injection or DLP) must be flagged so the single runtime fetch hook can
        // fail it closed on a substrate with no proxy route (the isolate). The
        // bridge's job is faithful propagation, not per-adapter enforcement — the
        // fail-closed lives at the consumption seam, not here. (audit H4.)
        let (_tempdir, bridge) = bridge_for_policy(
            "tenant-l7",
            EgressPolicy::new([
                EgressRule::new("plain", EgressProtocol::Https, "plain.internal", 443)
                    .with_methods(["GET"])
                    .with_path_prefixes(["/v1"]),
                EgressRule::new(
                    "credentialed",
                    EgressProtocol::Https,
                    "secret.internal",
                    443,
                )
                .with_methods(["GET"])
                .with_path_prefixes(["/v1"])
                .with_credential_injection(EgressCredentialInjection::new(
                    "github_pat",
                    "authorization",
                )),
            ]),
            None,
        );

        let plain = bridge.authorize(&runtime_request(
            "tenant-l7",
            EgressSubstrate::Isolate,
            "plain.internal",
            "/v1/ok",
        ));
        assert!(
            plain.is_allowed() && !plain.requires_proxy_enforcement(),
            "a plain allow rule carries no proxy-enforcement requirement, got: {}",
            plain.reason()
        );

        let credentialed = bridge.authorize(&runtime_request(
            "tenant-l7",
            EgressSubstrate::Isolate,
            "secret.internal",
            "/v1/ok",
        ));
        assert!(
            credentialed.is_allowed() && credentialed.requires_proxy_enforcement(),
            "the bridge must propagate the credential rule's proxy-enforcement \
             requirement so the runtime hook can fail closed (allowed={}, requires_proxy={})",
            credentialed.is_allowed(),
            credentialed.requires_proxy_enforcement()
        );
    }

    #[test]
    fn egress_gateway_per_tenant_policies_are_isolated() {
        let (_first_tempdir, first) = bridge_for_policy(
            "tenant-alpha",
            EgressPolicy::new([EgressRule::new(
                "alpha-api",
                EgressProtocol::Https,
                "alpha.example",
                443,
            )]),
            None,
        );
        let (_second_tempdir, second) = bridge_for_policy(
            "tenant-beta",
            EgressPolicy::new([EgressRule::new(
                "beta-api",
                EgressProtocol::Https,
                "beta.example",
                443,
            )]),
            None,
        );

        assert!(
            first
                .authorize(&runtime_request(
                    "tenant-alpha",
                    EgressSubstrate::Isolate,
                    "alpha.example",
                    "/"
                ))
                .is_allowed()
        );
        let denied_by_other_tenant = second.authorize(&runtime_request(
            "tenant-beta",
            EgressSubstrate::Isolate,
            "alpha.example",
            "/",
        ));
        assert!(!denied_by_other_tenant.is_allowed());
        assert!(
            denied_by_other_tenant.reason().contains("default deny"),
            "other tenant's policy must not bleed across decisions: {}",
            denied_by_other_tenant.reason()
        );

        let mismatched_label = RuntimeEgressRequest {
            tenant_label: Some("tenant-alpha".to_string()),
            ..runtime_request("tenant-beta", EgressSubstrate::Isolate, "beta.example", "/")
        };
        let label_denial = second.authorize(&mismatched_label);
        assert!(!label_denial.is_allowed());
        assert!(
            label_denial
                .reason()
                .contains("does not match admitted tenant"),
            "tenant-label mismatch should fail before policy authorization: {}",
            label_denial.reason()
        );
    }

    #[test]
    fn egress_gateway_no_policy_tenant_default_deny() {
        let (_tempdir, bridge) = bridge_for_decision(default_decision("tenant-no-policy"), None);

        let denial = bridge.authorize(&runtime_request(
            "tenant-no-policy",
            EgressSubstrate::Isolate,
            "api.internal",
            "/v1/messages",
        ));
        assert!(!denial.is_allowed());
        assert!(
            denial.reason().contains("default deny"),
            "no-policy tenants must deny by default: {}",
            denial.reason()
        );
    }

    #[test]
    fn egress_gateway_runtime_request_without_tenant_label_fails_closed() {
        let (_tempdir, bridge) = bridge_for_policy(
            "tenant-missing-label",
            EgressPolicy::new([EgressRule::new(
                "api",
                EgressProtocol::Https,
                "api.internal",
                443,
            )]),
            None,
        );

        let unlabeled = RuntimeEgressRequest {
            tenant_label: None,
            ..runtime_request(
                "tenant-missing-label",
                EgressSubstrate::Isolate,
                "api.internal",
                "/",
            )
        };
        let denial = bridge.authorize(&unlabeled);
        assert!(!denial.is_allowed());
        assert!(
            denial.reason().contains("tenant label is absent"),
            "runtime egress without an admitted tenant label must fail closed: {}",
            denial.reason()
        );
    }

    #[test]
    fn egress_gateway_readiness_gate_denies_workload_traffic_until_ready() {
        let decision = decision_for_policy(
            "tenant-readiness",
            EgressPolicy::new([EgressRule::new(
                "ready-api",
                EgressProtocol::Https,
                "api.internal",
                443,
            )]),
        );
        let not_ready = EgressGatewayEnforcementReadiness::not_ready_for_decision(
            &decision,
            "PEP policy generation has not been installed",
        );
        let (_blocked_tempdir, blocked) = bridge_for_decision(decision.clone(), Some(not_ready));
        let denial = blocked.authorize(&runtime_request(
            "tenant-readiness",
            EgressSubstrate::Isolate,
            "api.internal",
            "/",
        ));
        assert!(!denial.is_allowed());
        assert!(
            denial.reason().contains("not ready"),
            "workload traffic must not start until egress readiness is active: {}",
            denial.reason()
        );

        let (_ready_tempdir, ready) = bridge_for_decision(decision, None);
        assert!(
            ready
                .authorize(&runtime_request(
                    "tenant-readiness",
                    EgressSubstrate::Isolate,
                    "api.internal",
                    "/",
                ))
                .is_allowed()
        );
    }

    #[test]
    fn egress_gateway_custom_client_and_udp_fail_closed() {
        let (_tempdir, bridge) = bridge_for_policy(
            "tenant-custom-client",
            EgressPolicy::new([EgressRule::new(
                "api",
                EgressProtocol::Https,
                "api.internal",
                443,
            )]),
            None,
        );
        let custom_client_denial = bridge.authorize(&RuntimeEgressRequest {
            uses_custom_client: true,
            ..runtime_request(
                "tenant-custom-client",
                EgressSubstrate::Isolate,
                "api.internal",
                "/",
            )
        });
        assert!(!custom_client_denial.is_allowed());
        assert!(
            custom_client_denial
                .reason()
                .contains("custom fetch clients"),
            "custom client proxy settings must not inherit policy allow: {}",
            custom_client_denial.reason()
        );

        let udp_denial = bridge.authorize(&RuntimeEgressRequest {
            protocol: RuntimeEgressProtocol::Udp,
            ..runtime_request(
                "tenant-custom-client",
                EgressSubstrate::Isolate,
                "api.internal",
                "/",
            )
        });
        assert!(!udp_denial.is_allowed());
        assert!(
            udp_denial.reason().contains("does not authorize"),
            "unsupported UDP must fail closed: {}",
            udp_denial.reason()
        );
    }

    fn bridge_for_policy(
        tenant: &str,
        policy: EgressPolicy,
        readiness: Option<EgressGatewayEnforcementReadiness>,
    ) -> (TempDir, ConvexHostBridge) {
        bridge_for_decision(decision_for_policy(tenant, policy), readiness)
    }

    fn bridge_for_decision(
        decision: TenantIsolationDecision,
        readiness: Option<EgressGatewayEnforcementReadiness>,
    ) -> (TempDir, ConvexHostBridge) {
        let tempdir = tempdir().expect("egress gateway tempdir should build");
        let engine = Arc::new(Engine::new(tempdir.path()).expect("engine should build"));
        engine
            .create_tenant(decision.tenant_id().clone())
            .expect("tenant should be created");
        let registry = Arc::new(ConvexRegistry::empty());
        let runtime_service_registry = Arc::new(ServiceInstanceBindingRegistry::new(Arc::new(
            EmptyServiceInstanceCatalog,
        )));
        let mut scope = ConvexHostBridgeScope::new_for_test(
            engine,
            registry,
            decision,
            runtime_service_registry,
        );
        if let Some(readiness) = readiness {
            scope = scope.with_egress_readiness(readiness);
        }
        let bridge = ConvexHostBridge::build(
            scope,
            ConvexHostBridgeInvocation::new(
                None,
                Default::default(),
                PrincipalContext::anonymous(),
                None,
                InvocationKind::Query,
                "egress_gateway_test",
            ),
        )
        .expect("egress gateway bridge should build");
        (tempdir, bridge)
    }

    fn decision_for_policy(tenant: &str, policy: EgressPolicy) -> TenantIsolationDecision {
        let context = TenantIsolationContext::application(
            TenantId::new(tenant).expect("tenant id should build"),
            PrincipalContext::anonymous(),
            "egress_gateway_test",
        );
        let runtime_policy = RuntimePolicy::default();
        let network = TenantNetworkPolicyDecision::new([])
            .with_sandbox_egress(policy)
            .expect("egress policy should compile");
        context
            .admit_decision(
                TenantIsolationPolicyInput::new(WorkloadAttributes::runtime_function(
                    "egress_gateway_test",
                    RuntimeIsolationTier::InProcessUntrusted,
                ))
                .with_runtime_policy(
                    &context,
                    &runtime_policy,
                    RuntimeIsolationTier::InProcessUntrusted,
                    TenantIsolationMode::LocalDevelopment,
                )
                .with_network(network)
                .with_storage(TenantStoragePolicyDecision::namespace(
                    context.tenant_id().as_str(),
                )),
            )
            .expect("tenant isolation decision should build")
    }

    fn default_decision(tenant: &str) -> TenantIsolationDecision {
        let context = TenantIsolationContext::application(
            TenantId::new(tenant).expect("tenant id should build"),
            PrincipalContext::anonymous(),
            "egress_gateway_test",
        );
        let runtime_policy = RuntimePolicy::default();
        context
            .admit_decision(
                TenantIsolationPolicyInput::new(WorkloadAttributes::runtime_function(
                    "egress_gateway_test",
                    RuntimeIsolationTier::InProcessUntrusted,
                ))
                .with_runtime_policy(
                    &context,
                    &runtime_policy,
                    RuntimeIsolationTier::InProcessUntrusted,
                    TenantIsolationMode::LocalDevelopment,
                )
                .with_storage(TenantStoragePolicyDecision::namespace(
                    context.tenant_id().as_str(),
                )),
            )
            .expect("tenant isolation decision should build")
    }

    fn runtime_request(
        tenant: &str,
        substrate: EgressSubstrate,
        host: &str,
        path: &str,
    ) -> RuntimeEgressRequest {
        RuntimeEgressRequest {
            substrate,
            protocol: RuntimeEgressProtocol::Https,
            method: Some("GET".to_string()),
            url: Some(format!("https://{host}{path}")),
            host: host.to_string(),
            port: 443,
            path_and_query: Some(path.to_string()),
            tenant_label: Some(tenant.to_string()),
            session_id: Some("session-egress-gateway-test".to_string()),
            invocation_id: Some(1),
            uses_custom_client: false,
        }
    }

    fn container_request(host: &str, path: &str) -> nimbus_egress::EgressRequest {
        nimbus_egress::EgressRequest::new(EgressProtocol::Https, host, 443).with_http("GET", path)
    }
}
