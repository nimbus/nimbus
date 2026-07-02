//! Shared host-bridge egress authorization.
//!
//! Every adapter host bridge that implements [`nimbus_runtime::EgressGateway`]
//! funnels through [`authorize_runtime_egress`] so the per-tenant PDP
//! (`nimbus-egress`) decision, the readiness gate, the tenant-label guard, the
//! custom-client / UDP fail-closed, and faithful proxy-enforcement propagation
//! are identical across every substrate and adapter. "Three substrates, one
//! decision" — Convex, Cloud Functions, and any future runtime adapter resolve
//! the same verdict from the same code, instead of each re-encoding (or
//! forgetting) the policy translation. (audit M13 — Cloud Functions egress
//! parity.)

use nimbus_egress::{
    EgressAuthorization as PolicyEgressAuthorization, EgressProtocol as PolicyEgressProtocol,
    EgressRequest as PolicyEgressRequest,
};
use nimbus_runtime::{
    EgressAuthorization as RuntimeEgressAuthorization, EgressProtocol as RuntimeEgressProtocol,
    EgressRequest as RuntimeEgressRequest,
};
use nimbus_tenant::TenantIsolationDecision;

/// Readiness latch for a tenant's egress enforcement plane.
///
/// A host bridge is only allowed to authorize workload traffic once the
/// nimbus-proxy PEP policy generation for its admitted decision is installed.
/// Until then every request fails closed, so a workload can never egress
/// against a half-installed policy. The latch is bound to a specific
/// [`TenantIsolationDecision`] id so a readiness token minted for one decision
/// can never silently authorize traffic for another.
///
/// **Production producer status (audit M14 — stated decision: keep the seam).**
/// [`Self::not_ready_for_decision`] is the seam for NEG's cross-substrate
/// generation-gated readiness (NEG4): a future producer, tied to the nimbus-proxy
/// PEP generation-install signal, will mint it so container/microVM workloads
/// cannot start before the tenant generation's enforcement state is installed.
/// Today every host bridge constructs [`Self::ready_for_decision`], which is
/// correct — *not* fail-open — because the in-process isolate gateway is
/// synchronous and its decision is admitted before the runtime executes, so there
/// is no install gap to gate. The seam is kept (not removed and not left silently
/// `#[cfg(test)]`-inert) so that producer can be wired without re-introducing a
/// contract; the not-ready path is already fully enforced (see
/// [`Self::ensure_ready`]) and is exercised by tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EgressGatewayEnforcementReadiness {
    decision_id: String,
    ready: bool,
    reason: Option<String>,
}

impl EgressGatewayEnforcementReadiness {
    /// The enforcement plane is installed and ready for `decision`.
    pub fn ready_for_decision(decision: &TenantIsolationDecision) -> Self {
        Self {
            decision_id: decision.id().as_str().to_owned(),
            ready: true,
            reason: None,
        }
    }

    /// The enforcement plane for `decision` is not yet ready; `reason` explains
    /// why so workload traffic fails closed with an actionable diagnostic.
    pub fn not_ready_for_decision(
        decision: &TenantIsolationDecision,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            decision_id: decision.id().as_str().to_owned(),
            ready: false,
            reason: Some(reason.into()),
        }
    }

    fn ensure_ready(&self, decision: &TenantIsolationDecision) -> std::result::Result<(), String> {
        if self.decision_id != decision.id().as_str() {
            return Err(format!(
                "egress gateway readiness belongs to decision {}, not active decision {}",
                self.decision_id,
                decision.id().as_str()
            ));
        }
        if !self.ready {
            return Err(format!(
                "egress gateway enforcement state is not ready for decision {}{}",
                self.decision_id,
                self.reason
                    .as_deref()
                    .map(|reason| format!(": {reason}"))
                    .unwrap_or_default()
            ));
        }
        Ok(())
    }
}

/// Authorize a runtime egress request against the tenant's admitted decision.
///
/// This is the single shared bridging path between the `nimbus-runtime` fetch
/// surface and the `nimbus-egress` PDP. It enforces, in order: the readiness
/// latch, the tenant-label guard, the custom-client fail-closed, the
/// unsupported-protocol (UDP) fail-closed, and finally the per-tenant policy
/// verdict — faithfully propagating whether the matched rule still requires
/// PEP-mediated L7 enforcement (credential injection / DLP) so the runtime
/// fetch hook can fail it closed on substrates with no proxy route (the
/// isolate). The bridge propagates; it never re-encodes the L7 fail-closed.
pub fn authorize_runtime_egress(
    decision: &TenantIsolationDecision,
    readiness: &EgressGatewayEnforcementReadiness,
    request: &RuntimeEgressRequest,
) -> RuntimeEgressAuthorization {
    if let Err(reason) = readiness.ensure_ready(decision) {
        return RuntimeEgressAuthorization::deny(reason);
    }
    match request.tenant_label.as_deref() {
        Some(tenant_label) if tenant_label != decision.tenant_id().as_str() => {
            return RuntimeEgressAuthorization::deny(format!(
                "egress gateway request tenant `{tenant_label}` does not match admitted tenant `{}`",
                decision.tenant_id()
            ));
        }
        Some(_) => {}
        None => {
            return RuntimeEgressAuthorization::deny(format!(
                "egress gateway request tenant label is absent; runtime egress requires a tenant label matching admitted tenant `{}`",
                decision.tenant_id()
            ));
        }
    }
    if request.uses_custom_client {
        return RuntimeEgressAuthorization::deny(
            "custom fetch clients must route through the Nimbus egress PEP",
        );
    }

    let Some(policy_request) = policy_request_from_runtime_request(request) else {
        return RuntimeEgressAuthorization::deny(format!(
            "egress gateway does not authorize {:?} traffic for {:?}",
            request.protocol, request.substrate
        ));
    };
    runtime_authorization_from_policy(decision.network().authorize_sandbox_egress(&policy_request))
}

fn policy_request_from_runtime_request(
    request: &RuntimeEgressRequest,
) -> Option<PolicyEgressRequest> {
    let protocol = match request.protocol {
        RuntimeEgressProtocol::Http => PolicyEgressProtocol::Http,
        RuntimeEgressProtocol::Https => PolicyEgressProtocol::Https,
        RuntimeEgressProtocol::Tcp => PolicyEgressProtocol::Tcp,
        RuntimeEgressProtocol::Udp => return None,
    };
    let mut policy_request = PolicyEgressRequest::new(protocol, request.host.clone(), request.port);
    if matches!(
        request.protocol,
        RuntimeEgressProtocol::Http | RuntimeEgressProtocol::Https
    ) && let (Some(method), Some(path)) =
        (request.method.as_deref(), request.path_and_query.as_deref())
    {
        policy_request = policy_request.with_http(method, path);
    }
    Some(policy_request)
}

fn runtime_authorization_from_policy(
    authorization: PolicyEgressAuthorization,
) -> RuntimeEgressAuthorization {
    if !authorization.is_allowed() {
        return RuntimeEgressAuthorization::deny(authorization.reason());
    }
    // Map the PDP verdict faithfully — including whether the matched rule needs
    // PEP-mediated L7 (credential injection / DLP). The fail-closed for substrates
    // with no proxy route (the isolate `fetch`) lives at the single runtime fetch
    // hook, the consumption seam every adapter funnels through, so no host bridge
    // re-encodes the rule. (audit H4.)
    let mut runtime_authorization = RuntimeEgressAuthorization::allow(authorization.reason())
        .requiring_proxy_enforcement(authorization.requires_proxy_enforcement());
    if let Some(rule) = authorization.matched_rule() {
        runtime_authorization = runtime_authorization.with_matched_rule(rule);
    }
    runtime_authorization
}

#[cfg(test)]
mod tests {
    use nimbus_core::{PrincipalContext, TenantId};
    use nimbus_egress::{EgressPolicy, EgressProtocol, EgressRule};
    use nimbus_runtime::{EgressSubstrate, RuntimePolicy};
    use nimbus_tenant::{
        RuntimeIsolationTier, TenantIsolationContext, TenantIsolationMode,
        TenantIsolationPolicyInput, TenantNetworkPolicyDecision, TenantStoragePolicyDecision,
        WorkloadAttributes,
    };

    use super::*;

    #[test]
    fn runtime_egress_absent_tenant_label_denies_before_policy() {
        let decision = decision_for_policy(
            "tenant-a",
            EgressPolicy::new([EgressRule::new(
                "api",
                EgressProtocol::Https,
                "api.internal",
                443,
            )]),
        );
        let readiness = EgressGatewayEnforcementReadiness::ready_for_decision(&decision);

        let authorization = authorize_runtime_egress(
            &decision,
            &readiness,
            &runtime_request(None, "api.internal"),
        );

        assert!(!authorization.is_allowed());
        assert!(
            authorization.reason().contains("tenant label is absent"),
            "absent tenant label must fail closed before policy authorization: {}",
            authorization.reason()
        );
    }

    #[test]
    fn runtime_egress_mismatched_tenant_label_denies_before_policy() {
        let decision = decision_for_policy(
            "tenant-a",
            EgressPolicy::new([EgressRule::new(
                "api",
                EgressProtocol::Https,
                "api.internal",
                443,
            )]),
        );
        let readiness = EgressGatewayEnforcementReadiness::ready_for_decision(&decision);

        let authorization = authorize_runtime_egress(
            &decision,
            &readiness,
            &runtime_request(Some("tenant-b"), "api.internal"),
        );

        assert!(!authorization.is_allowed());
        assert!(
            authorization
                .reason()
                .contains("does not match admitted tenant"),
            "mismatched tenant label must fail closed before policy authorization: {}",
            authorization.reason()
        );
    }

    #[test]
    fn runtime_egress_matching_tenant_label_preserves_policy_verdicts() {
        let decision = decision_for_policy(
            "tenant-a",
            EgressPolicy::new([EgressRule::new(
                "api",
                EgressProtocol::Https,
                "api.internal",
                443,
            )]),
        );
        let readiness = EgressGatewayEnforcementReadiness::ready_for_decision(&decision);

        let allowed = authorize_runtime_egress(
            &decision,
            &readiness,
            &runtime_request(Some("tenant-a"), "api.internal"),
        );
        assert!(
            allowed.is_allowed(),
            "matching tenant label should preserve policy allow verdict: {}",
            allowed.reason()
        );
        assert_eq!(allowed.matched_rule(), Some("api"));

        let denied = authorize_runtime_egress(
            &decision,
            &readiness,
            &runtime_request(Some("tenant-a"), "blocked.internal"),
        );
        assert!(!denied.is_allowed());
        assert!(
            denied.reason().contains("default deny"),
            "matching tenant label should still preserve policy deny verdict: {}",
            denied.reason()
        );
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

    fn runtime_request(tenant_label: Option<&str>, host: &str) -> RuntimeEgressRequest {
        RuntimeEgressRequest {
            substrate: EgressSubstrate::Isolate,
            protocol: RuntimeEgressProtocol::Https,
            method: Some("GET".to_string()),
            url: Some(format!("https://{host}/v1/messages")),
            host: host.to_string(),
            port: 443,
            path_and_query: Some("/v1/messages".to_string()),
            tenant_label: tenant_label.map(str::to_string),
            session_id: Some("session-egress-bridge-test".to_string()),
            invocation_id: Some(1),
            uses_custom_client: false,
        }
    }
}
