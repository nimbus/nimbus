use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use nimbus_core::Result;
use nimbus_runtime::{
    EgressAuthorization, EgressGateway, EgressRequest, HostBridge, HostBridgeFuture,
    HostCallCancellation, HostCallEnvelope, HostCallPayload, HostCallRequest, NimbusRuntimeError,
    RuntimePolicy,
};
use serde_json::Value;

use crate::runtime_api;
use nimbus_bridge::capabilities::RuntimeServiceCapabilityHost;
use nimbus_bridge::egress::{EgressGatewayEnforcementReadiness, authorize_runtime_egress};
use nimbus_bridge::host_calls::{
    RuntimeAsyncHostCallTrace, execute_async_host_call, execute_host_call,
    execute_host_call_cancellable,
};
use nimbus_bridge::{RuntimeHostContext, RuntimeHostInvocation, RuntimeHostScope, abi};
use nimbus_tenant::TenantIsolationDecision;

#[derive(Clone)]
pub struct CloudFunctionsHostBridge {
    context: RuntimeHostContext,
    runtime_policy: Arc<RuntimePolicy>,
    decision: TenantIsolationDecision,
    egress_readiness: EgressGatewayEnforcementReadiness,
}

impl CloudFunctionsHostBridge {
    pub fn build(scope: RuntimeHostScope, invocation: RuntimeHostInvocation) -> Result<Self> {
        let runtime_policy = scope.runtime_policy().clone();
        // Retain the admitted decision so the bridge can answer the runtime's
        // per-tenant egress questions through the shared PDP path, exactly like
        // the Convex host bridge. Without this a Cloud Functions handler's
        // `fetch` would be governed only by coarse deno_permissions and never by
        // the tenant's nimbus-egress policy. (audit M13.)
        let decision = scope.decision().clone();
        let egress_readiness = EgressGatewayEnforcementReadiness::ready_for_decision(&decision);
        let context = RuntimeHostContext::build(scope, invocation)?;
        Ok(Self {
            context,
            runtime_policy,
            decision,
            egress_readiness,
        })
    }

    pub fn commit_mutation_execution_unit(&self) -> Result<()> {
        self.context.commit_mutation_execution_unit()
    }

    pub fn service_capabilities(&self) -> Option<&dyn RuntimeServiceCapabilityHost> {
        None
    }

    async fn dispatch_host_call_async(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        let operation = envelope.operation().as_str();
        match envelope.payload {
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => {
                abi::document_calls::dispatch_document_host_call_async(
                    &self.context,
                    payload,
                    cancellation,
                )
                .await
            }
            HostCallPayload::RuntimeExtensionCall(payload) => {
                runtime_api::dispatch_runtime_extension_call_async(
                    &self.context,
                    payload,
                    cancellation,
                )
                .await
            }
            _ => unsupported_cloud_functions_host_operation(operation),
        }
    }

    fn dispatch_host_call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        let operation = envelope.operation().as_str();
        match envelope.payload {
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => {
                abi::document_calls::dispatch_document_host_call_cancellable(
                    &self.context,
                    payload,
                    cancellation,
                )
            }
            HostCallPayload::RuntimeExtensionCall(payload) => {
                runtime_api::dispatch_runtime_extension_call_cancellable(
                    &self.context,
                    payload,
                    cancellation,
                )
            }
            _ => unsupported_cloud_functions_host_operation(operation),
        }
    }

    fn dispatch_host_call(
        &self,
        request: HostCallRequest,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let envelope = HostCallEnvelope::try_from(request)?;
        let operation = envelope.operation().as_str();
        match envelope.payload {
            payload @ (HostCallPayload::DocumentGet(_)
            | HostCallPayload::DocumentInsert(_)
            | HostCallPayload::DocumentPatch(_)
            | HostCallPayload::DocumentDelete(_)) => {
                abi::document_calls::dispatch_document_host_call(&self.context, payload)
            }
            HostCallPayload::RuntimeExtensionCall(payload) => {
                runtime_api::dispatch_runtime_extension_call(&self.context, payload)
            }
            _ => unsupported_cloud_functions_host_operation(operation),
        }
    }
}

impl HostBridge for CloudFunctionsHostBridge {
    fn call(&self, request: HostCallRequest) -> std::result::Result<Value, NimbusRuntimeError> {
        let operation = request.operation.as_str();
        execute_host_call(self.runtime_policy.metrics().as_ref(), operation, || {
            self.dispatch_host_call(request)
        })
    }

    fn call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NimbusRuntimeError> {
        let operation = request.operation.as_str();
        execute_host_call_cancellable(
            self.runtime_policy.metrics().as_ref(),
            operation,
            cancellation,
            || self.dispatch_host_call_cancellable(request, cancellation),
        )
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let bridge = self.clone();
        static NEXT_ASYNC_HOST_CALL_ID: AtomicU64 = AtomicU64::new(1);
        let trace = RuntimeAsyncHostCallTrace::new(
            tracing::debug_span!(
                "cloud_functions_runtime_async_host_call",
                tenant = %bridge.context.tenant_id(),
                server_request_id = ?bridge.context.server_request_id(),
                host_call_session_id = %bridge.context.host_call_session_id(),
                operation = %request.operation.as_str(),
                host_call_id = NEXT_ASYNC_HOST_CALL_ID.fetch_add(1, Ordering::Relaxed),
            ),
            "cloud functions runtime async host call",
        );
        let metrics = bridge.runtime_policy.metrics();
        let operation = request.operation.as_str();
        Box::pin(execute_async_host_call(
            trace,
            metrics,
            operation,
            cancellation.clone(),
            async move {
                bridge
                    .dispatch_host_call_async(request, &cancellation)
                    .await
            },
        ))
    }
}

impl EgressGateway for CloudFunctionsHostBridge {
    fn authorize(&self, request: &EgressRequest) -> EgressAuthorization {
        // Cloud Functions handlers run on the isolate substrate. Route their
        // `fetch` through the same shared bridging path the Convex host bridge
        // uses so the per-tenant nimbus-egress PDP — not coarse net permissions
        // — decides, and proxy-enforced (credential injection / DLP) allows are
        // propagated for the runtime fetch hook to fail closed. (audit M13.)
        authorize_runtime_egress(&self.decision, &self.egress_readiness, request)
    }
}

fn unsupported_cloud_functions_host_operation(
    operation: &str,
) -> std::result::Result<Value, NimbusRuntimeError> {
    Err(NimbusRuntimeError::Contract(format!(
        "cloud functions runtime host does not support operation `{operation}`"
    )))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus_core::{PrincipalContext, TenantId};
    use nimbus_egress::{EgressCredentialInjection, EgressPolicy, EgressProtocol, EgressRule};
    use nimbus_engine::Engine;
    use nimbus_runtime::{
        EgressGateway, EgressProtocol as RuntimeEgressProtocol,
        EgressRequest as RuntimeEgressRequest, EgressSubstrate, InvocationKind, RuntimePolicy,
    };
    use nimbus_tenant::{
        RuntimeIsolationTier, TenantIsolationContext, TenantIsolationDecision, TenantIsolationMode,
        TenantIsolationPolicyInput, TenantNetworkPolicyDecision, TenantStoragePolicyDecision,
        WorkloadAttributes,
    };
    use tempfile::{TempDir, tempdir};

    use nimbus_bridge::egress::EgressGatewayEnforcementReadiness;
    use nimbus_bridge::{RuntimeHostInvocation, RuntimeHostScope};

    use super::{CloudFunctionsHostBridge, unsupported_cloud_functions_host_operation};

    #[test]
    fn cloud_functions_service_lookup_stays_refusal_only() {
        let error = unsupported_cloud_functions_host_operation("ctx_service_lookup")
            .expect_err("cloud functions must refuse injected service lookup capability");

        assert!(
            error
                .to_string()
                .contains("does not support operation `ctx_service_lookup`"),
            "service lookup refusal should name the unsupported operation: {error}"
        );
    }

    // A Cloud Functions handler runs on the isolate substrate. Its `fetch` must
    // be governed by the same per-tenant nimbus-egress PDP as the container
    // plane — not by coarse deno_permissions. These tests mirror the Convex
    // host-bridge parity tests so "three substrates, one decision" holds for the
    // Cloud Functions adapter too. (audit M13.)

    #[test]
    fn cloud_functions_egress_denies_and_allows_same_policy() {
        let (_tempdir, bridge) = bridge_for_policy(
            "tenant-cf-parity",
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

        // Denied by a policy that does not allow the host: the isolate verdict
        // and the container-plane PDP verdict are byte-for-byte the same.
        let isolate_deny = bridge.authorize(&runtime_request(
            "tenant-cf-parity",
            EgressSubstrate::Isolate,
            "evil.example",
            "/v1/steal",
        ));
        let container_deny = bridge
            .decision
            .network()
            .authorize_sandbox_egress(&container_request("evil.example", "/v1/steal"));
        assert!(
            !isolate_deny.is_allowed(),
            "cloud functions fetch to a non-allowed host must be denied: {}",
            isolate_deny.reason()
        );
        assert!(!container_deny.is_allowed());
        assert_eq!(isolate_deny.reason(), container_deny.reason());

        // Allowed by a policy that allows the host, resolving the same rule the
        // container plane resolves.
        let isolate_allow = bridge.authorize(&runtime_request(
            "tenant-cf-parity",
            EgressSubstrate::Isolate,
            "api.internal",
            "/v1/messages",
        ));
        let container_allow = bridge
            .decision
            .network()
            .authorize_sandbox_egress(&container_request("api.internal", "/v1/messages"));
        assert!(
            isolate_allow.is_allowed(),
            "cloud functions fetch to an allowed host must be allowed: {}",
            isolate_allow.reason()
        );
        assert!(container_allow.is_allowed());
        assert_eq!(
            isolate_allow.matched_rule(),
            container_allow.matched_rule(),
            "isolate and container substrates must resolve the same matched rule"
        );
    }

    #[test]
    fn cloud_functions_egress_propagates_proxy_enforcement_requirement() {
        // A rule whose enforcement depends on the nimbus-proxy PEP (credential
        // injection / DLP) must be flagged so the single runtime fetch hook can
        // fail it closed on the isolate, which has no proxy route. The bridge
        // propagates the requirement; it does not re-encode the fail-closed.
        let (_tempdir, bridge) = bridge_for_policy(
            "tenant-cf-l7",
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
            "tenant-cf-l7",
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
            "tenant-cf-l7",
            EgressSubstrate::Isolate,
            "secret.internal",
            "/v1/ok",
        ));
        assert!(
            credentialed.is_allowed() && credentialed.requires_proxy_enforcement(),
            "the cloud functions bridge must propagate the credential rule's \
             proxy-enforcement requirement so the runtime hook fails closed on the \
             isolate (allowed={}, requires_proxy={})",
            credentialed.is_allowed(),
            credentialed.requires_proxy_enforcement()
        );
    }

    #[test]
    fn cloud_functions_egress_no_policy_tenant_default_deny() {
        let (_tempdir, bridge) = bridge_for_decision(default_decision("tenant-cf-no-policy"), None);

        let denial = bridge.authorize(&runtime_request(
            "tenant-cf-no-policy",
            EgressSubstrate::Isolate,
            "api.internal",
            "/v1/messages",
        ));
        assert!(!denial.is_allowed());
        assert!(
            denial.reason().contains("default deny"),
            "no-policy cloud functions tenants must deny fetch by default: {}",
            denial.reason()
        );
    }

    #[test]
    fn cloud_functions_egress_without_tenant_label_fails_closed() {
        let (_tempdir, bridge) = bridge_for_policy(
            "tenant-cf-missing-label",
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
                "tenant-cf-missing-label",
                EgressSubstrate::Isolate,
                "api.internal",
                "/",
            )
        };
        let denial = bridge.authorize(&unlabeled);
        assert!(!denial.is_allowed());
        assert!(
            denial.reason().contains("tenant label is absent"),
            "cloud functions runtime egress without a tenant label must fail closed: {}",
            denial.reason()
        );
    }

    #[test]
    fn cloud_functions_egress_per_tenant_policies_are_isolated() {
        let (_alpha_tempdir, alpha) = bridge_for_policy(
            "tenant-cf-alpha",
            EgressPolicy::new([EgressRule::new(
                "alpha-api",
                EgressProtocol::Https,
                "alpha.example",
                443,
            )]),
            None,
        );
        let (_beta_tempdir, beta) = bridge_for_policy(
            "tenant-cf-beta",
            EgressPolicy::new([EgressRule::new(
                "beta-api",
                EgressProtocol::Https,
                "beta.example",
                443,
            )]),
            None,
        );

        assert!(
            alpha
                .authorize(&runtime_request(
                    "tenant-cf-alpha",
                    EgressSubstrate::Isolate,
                    "alpha.example",
                    "/"
                ))
                .is_allowed()
        );
        let denied_by_other_tenant = beta.authorize(&runtime_request(
            "tenant-cf-beta",
            EgressSubstrate::Isolate,
            "alpha.example",
            "/",
        ));
        assert!(!denied_by_other_tenant.is_allowed());
        assert!(
            denied_by_other_tenant.reason().contains("default deny"),
            "one tenant's egress policy must not bleed into another's bridge: {}",
            denied_by_other_tenant.reason()
        );

        let mismatched_label = RuntimeEgressRequest {
            tenant_label: Some("tenant-cf-alpha".to_string()),
            ..runtime_request(
                "tenant-cf-beta",
                EgressSubstrate::Isolate,
                "beta.example",
                "/",
            )
        };
        let label_denial = beta.authorize(&mismatched_label);
        assert!(!label_denial.is_allowed());
        assert!(
            label_denial
                .reason()
                .contains("does not match admitted tenant"),
            "tenant-label mismatch must fail before policy authorization: {}",
            label_denial.reason()
        );
    }

    #[test]
    fn cloud_functions_egress_readiness_gate_denies_until_ready() {
        let decision = decision_for_policy(
            "tenant-cf-readiness",
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
            "tenant-cf-readiness",
            EgressSubstrate::Isolate,
            "api.internal",
            "/",
        ));
        assert!(!denial.is_allowed());
        assert!(
            denial.reason().contains("not ready"),
            "cloud functions traffic must not start until egress enforcement is ready: {}",
            denial.reason()
        );

        let (_ready_tempdir, ready) = bridge_for_decision(decision, None);
        assert!(
            ready
                .authorize(&runtime_request(
                    "tenant-cf-readiness",
                    EgressSubstrate::Isolate,
                    "api.internal",
                    "/"
                ))
                .is_allowed(),
            "the same tenant authorizes once enforcement is ready"
        );
    }

    #[test]
    fn cloud_functions_egress_custom_client_and_udp_fail_closed() {
        let (_tempdir, bridge) = bridge_for_policy(
            "tenant-cf-custom-client",
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
                "tenant-cf-custom-client",
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
            "a custom fetch client must not inherit the policy allow: {}",
            custom_client_denial.reason()
        );

        let udp_denial = bridge.authorize(&RuntimeEgressRequest {
            protocol: RuntimeEgressProtocol::Udp,
            ..runtime_request(
                "tenant-cf-custom-client",
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
    ) -> (TempDir, CloudFunctionsHostBridge) {
        bridge_for_decision(decision_for_policy(tenant, policy), readiness)
    }

    fn bridge_for_decision(
        decision: TenantIsolationDecision,
        readiness: Option<EgressGatewayEnforcementReadiness>,
    ) -> (TempDir, CloudFunctionsHostBridge) {
        let tempdir = tempdir().expect("egress gateway tempdir should build");
        let engine = Arc::new(Engine::new(tempdir.path()).expect("engine should build"));
        engine
            .create_tenant(decision.tenant_id().clone())
            .expect("tenant should be created");
        let scope =
            RuntimeHostScope::new(engine, Arc::new(RuntimePolicy::default()), decision.clone());
        let mut bridge = CloudFunctionsHostBridge::build(
            scope,
            RuntimeHostInvocation::new(
                PrincipalContext::anonymous(),
                None,
                InvocationKind::Mutation,
                "egress_gateway_test",
            ),
        )
        .expect("cloud functions egress bridge should build");
        if let Some(readiness) = readiness {
            bridge.egress_readiness = readiness;
        }
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
