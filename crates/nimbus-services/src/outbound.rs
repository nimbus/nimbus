//! CB5: outbound egress unification — broker egress through one PDP + PEP.
//!
//! A workload's outbound operations — `fetch` AND WebSocket-out — flow through
//! the SAME egress decision path: the pure `nimbus-egress` PDP
//! (`CompiledEgressPolicy::authorize`), with the `nimbus-proxy` PEP as the
//! enforcement point. There is NOT a separate WebSocket allowlist beside the
//! fetch one — a single `authorize_outbound` decides both, so a host allowed
//! (or denied) for fetch is identically allowed (or denied) for ws-out.
//!
//! Fail-closed on enforcement: when the matched rule
//! `requires_proxy_enforcement` (credential injection / L7 DLP), the outbound
//! op is permitted only if the workload's PEP is ready
//! (`nimbus_proxy::WorkloadPepReadiness`) — a policy that needs the PEP but has
//! no ready PEP path denies, never leaks. This is the broker's binding of the
//! K11P/EE per-workload PEP to the isolate substrate's outbound.

use nimbus_egress::{CompiledEgressPolicy, EgressProtocol, EgressRequest};
use nimbus_proxy::WorkloadPepReadiness;

/// A workload outbound operation. Both kinds authorize through one PDP call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundOp {
    /// An HTTP(S) `fetch`.
    Fetch,
    /// A WebSocket-out connect (`new WebSocket(...)`), authorized as its
    /// HTTP-upgrade target on the same decision path as fetch.
    WebSocket,
}

/// The unified outbound egress decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundDecision {
    pub allowed: bool,
    pub reason: String,
    /// The matched rule required PEP enforcement (credential/DLP).
    pub requires_enforcement: bool,
}

impl OutboundDecision {
    fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: reason.into(),
            requires_enforcement: false,
        }
    }
}

/// The broker's outbound egress gate: one compiled PDP, consulted identically
/// for fetch and ws-out, with PEP-readiness fail-closing enforcement-required
/// traffic.
pub struct OutboundEgress {
    policy: CompiledEgressPolicy,
}

impl OutboundEgress {
    pub fn new(policy: CompiledEgressPolicy) -> Self {
        Self { policy }
    }

    /// Build the egress request for an outbound op. WebSocket-out targets its
    /// upgrade host over HTTP(S) — the same request shape fetch produces, so
    /// both hit the identical PDP rules.
    fn request(op: &OutboundOp, host: &str, port: u16, secure: bool) -> EgressRequest {
        let protocol = if secure {
            EgressProtocol::Https
        } else {
            EgressProtocol::Http
        };
        // ws-out and fetch produce the SAME EgressRequest for a given host —
        // that identity is the unification.
        let _ = op;
        EgressRequest::new(protocol, host, port)
    }

    /// Authorize an outbound op through the unified PDP.
    ///
    /// Both `OutboundOp::Fetch` and `OutboundOp::WebSocket` call the SAME
    /// `policy.authorize`. When the matched rule requires PEP enforcement, the
    /// op is allowed only if `pep` is ready — fail-closed.
    pub fn authorize_outbound(
        &self,
        op: OutboundOp,
        host: &str,
        port: u16,
        secure: bool,
        pep: WorkloadPepReadiness,
    ) -> OutboundDecision {
        let request = Self::request(&op, host, port, secure);
        let authorization = self.policy.authorize(&request);
        if !authorization.is_allowed() {
            return OutboundDecision::deny(authorization.reason().to_owned());
        }
        let requires_enforcement = authorization.requires_proxy_enforcement();
        if requires_enforcement && !pep.ready {
            // Fail-closed: enforcement is required but no ready PEP path.
            return OutboundDecision::deny(format!(
                "egress to {host}:{port} requires PEP enforcement but the workload PEP is not ready"
            ));
        }
        OutboundDecision {
            allowed: true,
            reason: authorization.reason().to_owned(),
            requires_enforcement,
        }
    }

    /// Convenience: authorize a WebSocket-out on the same egress path as fetch.
    pub fn authorize_ws_out(
        &self,
        host: &str,
        port: u16,
        secure: bool,
        pep: WorkloadPepReadiness,
    ) -> OutboundDecision {
        self.authorize_outbound(OutboundOp::WebSocket, host, port, secure, pep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_egress::{EgressPolicy, EgressRule};

    fn ready() -> WorkloadPepReadiness {
        WorkloadPepReadiness {
            ready: true,
            audit_healthy: true,
            policy_generation: None,
        }
    }

    fn not_ready() -> WorkloadPepReadiness {
        WorkloadPepReadiness {
            ready: false,
            audit_healthy: true,
            policy_generation: None,
        }
    }

    fn audit_unhealthy() -> WorkloadPepReadiness {
        WorkloadPepReadiness {
            ready: false,
            audit_healthy: false,
            policy_generation: None,
        }
    }

    fn allow_policy(rules: Vec<EgressRule>) -> CompiledEgressPolicy {
        EgressPolicy::new(rules).compile().expect("compile")
    }

    #[test]
    fn fetch_and_ws_out_share_one_decision_path() {
        // One rule allows api.example; the SAME policy decides fetch and ws-out
        // identically. This is the unification: no separate ws allowlist.
        let policy = allow_policy(vec![EgressRule::new(
            "api",
            EgressProtocol::Https,
            "api.example",
            443,
        )]);
        let egress = OutboundEgress::new(policy);

        let f = egress.authorize_outbound(OutboundOp::Fetch, "api.example", 443, true, ready());
        let w = egress.authorize_ws_out("api.example", 443, true, ready());
        assert!(
            f.allowed && w.allowed,
            "allowed host: both fetch and ws-out pass"
        );

        let fd = egress.authorize_outbound(OutboundOp::Fetch, "evil.example", 443, true, ready());
        let wd = egress.authorize_ws_out("evil.example", 443, true, ready());
        assert!(
            !fd.allowed && !wd.allowed,
            "denied host: fetch AND ws-out both denied by the same PDP"
        );
    }

    #[test]
    fn enforcement_required_without_ready_pep_fails_closed() {
        // A rule that requires proxy enforcement (credential injection).
        let rule = EgressRule::new("api", EgressProtocol::Https, "api.example", 443)
            .with_credential_injection(nimbus_egress::EgressCredentialInjection::new(
                "stripe",
                "Authorization",
            ));
        let egress = OutboundEgress::new(allow_policy(vec![rule]));

        let allowed =
            egress.authorize_outbound(OutboundOp::Fetch, "api.example", 443, true, ready());
        assert!(allowed.allowed && allowed.requires_enforcement);

        let denied =
            egress.authorize_outbound(OutboundOp::Fetch, "api.example", 443, true, not_ready());
        assert!(
            !denied.allowed,
            "enforcement-required egress with no ready PEP must fail closed: {}",
            denied.reason
        );
        assert!(denied.reason.contains("not ready"));
    }

    #[test]
    fn enforcement_required_with_unhealthy_audit_sink_fails_closed() {
        let rule = EgressRule::new("api", EgressProtocol::Https, "api.example", 443)
            .with_credential_injection(nimbus_egress::EgressCredentialInjection::new(
                "stripe",
                "Authorization",
            ));
        let egress = OutboundEgress::new(allow_policy(vec![rule]));

        let denied = egress.authorize_outbound(
            OutboundOp::Fetch,
            "api.example",
            443,
            true,
            audit_unhealthy(),
        );

        assert!(
            !denied.allowed,
            "audit-unhealthy readiness must fail closed for enforcement-required egress"
        );
        assert!(denied.reason.contains("not ready"));
    }

    /// CB7: cross-substrate egress parity. One policy denying `evil.example`
    /// blocks it across every substrate that consults this PDP. The broker's
    /// two outbound op kinds (isolate fetch + isolate WS) are proven here
    /// directly; the container and microVM substrates enforce the SAME
    /// `CompiledEgressPolicy` through the sandbox's per-workload PEP (the EE
    /// WorkloadPep), so a single deny rule is node-wide across all four.
    #[test]
    fn cross_substrate_egress_parity_one_deny_blocks_all() {
        let policy = allow_policy(vec![EgressRule::new(
            "api",
            EgressProtocol::Https,
            "api.example",
            443,
        )]);
        let egress = OutboundEgress::new(policy.clone());

        // Isolate fetch + isolate WS: denied to evil.example.
        assert!(
            !egress
                .authorize_outbound(OutboundOp::Fetch, "evil.example", 443, true, ready())
                .allowed
        );
        assert!(
            !egress
                .authorize_ws_out("evil.example", 443, true, ready())
                .allowed
        );

        // Container + microVM ride the SAME compiled policy via the sandbox
        // PEP: the identical PDP call denies evil.example. (The sandbox PEP is
        // exercised in nimbus-sandbox's own egress tests; here we assert the
        // policy object the broker uses IS the shared decision core.)
        let direct = policy.authorize(&EgressRequest::new(
            EgressProtocol::Https,
            "evil.example",
            443,
        ));
        assert!(
            !direct.is_allowed(),
            "the shared PDP the container/microVM PEP consults denies evil.example identically"
        );

        // Parity sanity: the allowed host passes on every path.
        assert!(
            egress
                .authorize_outbound(OutboundOp::Fetch, "api.example", 443, true, ready())
                .allowed
        );
        assert!(
            policy
                .authorize(&EgressRequest::new(
                    EgressProtocol::Https,
                    "api.example",
                    443
                ))
                .is_allowed()
        );
    }
}
