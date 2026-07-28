use std::num::NonZeroU64;

use nimbus_egress::{CompiledEgressPolicy, LayeredEgressPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PolicyGeneration(u64);

impl PolicyGeneration {
    pub(crate) fn initial() -> Self {
        Self(1)
    }

    pub(crate) fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

/// Durable caller identity for one policy-reload effect attempt.
///
/// The PEP does not allocate these values. Its composition owner persists both
/// generations before invoking the effect and reuses the exact tuple while
/// reconciling an ambiguous acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PolicyReloadAttempt {
    desired_generation: NonZeroU64,
    attempt_generation: NonZeroU64,
}

impl PolicyReloadAttempt {
    pub const fn new(desired_generation: NonZeroU64, attempt_generation: NonZeroU64) -> Self {
        Self {
            desired_generation,
            attempt_generation,
        }
    }

    pub const fn desired_generation(self) -> NonZeroU64 {
        self.desired_generation
    }

    pub const fn attempt_generation(self) -> NonZeroU64 {
        self.attempt_generation
    }
}

/// Exact process-local provider acknowledgement for one durable reload attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyReloadReceipt {
    attempt: PolicyReloadAttempt,
    policy_generation: PolicyGeneration,
}

impl PolicyReloadReceipt {
    pub const fn attempt(self) -> PolicyReloadAttempt {
        self.attempt
    }

    pub const fn policy_generation(self) -> PolicyGeneration {
        self.policy_generation
    }
}

/// Exact observation of the policy currently active in one running PEP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyReloadObservation {
    /// The exact attempt and expected policy are active.
    Exact(PolicyReloadReceipt),
    /// The PEP has a policy but no durable reload-attempt identity.
    Untagged,
    /// A different durable attempt is active.
    Different(PolicyReloadReceipt),
    /// The expected attempt identity was reused with different policy bytes.
    ConflictingPolicy(PolicyReloadReceipt),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadPepReadiness {
    pub ready: bool,
    /// False after the first durable decision-log append failure. This is
    /// sticky for the lifetime of the PEP, so readiness fail-closes until the
    /// process restarts with a healthy audit sink.
    pub audit_healthy: bool,
    pub policy_generation: Option<PolicyGeneration>,
}

#[derive(Debug, Clone)]
pub(crate) struct LastKnownGoodPolicy {
    pub(crate) policy_generation: PolicyGeneration,
    pub(crate) reload_attempt: Option<PolicyReloadAttempt>,
    /// The layered (ceiling-composed) policy the request path authorizes
    /// against. With no ceiling configured this is `sandbox_only` — proven
    /// byte-identical to the bare sandbox policy — so empty-ceiling parity
    /// holds in the REAL request path, not just nimbus-egress unit tests.
    pub(crate) policy: LayeredEgressPolicy,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EgressProxyPolicyState {
    last_known_good: Option<LastKnownGoodPolicy>,
    /// Node-global allow-ceiling captured at PEP start (policy-hardening owns
    /// the content; hot-reload of the ceiling itself is out of scope here).
    /// Sandbox-policy reloads recompose against this captured ceiling.
    global_ceiling: Option<CompiledEgressPolicy>,
}

impl EgressProxyPolicyState {
    pub(crate) fn with_policy(policy: CompiledEgressPolicy) -> Self {
        Self {
            last_known_good: Some(LastKnownGoodPolicy {
                policy_generation: PolicyGeneration::initial(),
                reload_attempt: None,
                policy: LayeredEgressPolicy::sandbox_only(policy),
            }),
            global_ceiling: None,
        }
    }

    /// Capture the node-global allow-ceiling and recompose the active layered
    /// policy (generation unchanged — the sandbox policy did not change).
    pub(crate) fn set_global_ceiling(&mut self, ceiling: Option<CompiledEgressPolicy>) {
        self.global_ceiling = ceiling;
        if let Some(current) = self.last_known_good.take() {
            let sandbox = current.policy.sandbox().clone();
            self.last_known_good = Some(LastKnownGoodPolicy {
                policy_generation: current.policy_generation,
                reload_attempt: current.reload_attempt,
                policy: self.compose(sandbox),
            });
        }
    }

    fn compose(&self, sandbox: CompiledEgressPolicy) -> LayeredEgressPolicy {
        match &self.global_ceiling {
            Some(ceiling) => LayeredEgressPolicy::with_global_ceiling(ceiling.clone(), sandbox),
            None => LayeredEgressPolicy::sandbox_only(sandbox),
        }
    }

    pub(crate) fn active(&self) -> Option<&LastKnownGoodPolicy> {
        self.last_known_good.as_ref()
    }

    pub(crate) fn observe_reload(
        &self,
        policy: &CompiledEgressPolicy,
        attempt: PolicyReloadAttempt,
    ) -> PolicyReloadObservation {
        let Some(current) = self.last_known_good.as_ref() else {
            return PolicyReloadObservation::Untagged;
        };
        let Some(active_attempt) = current.reload_attempt else {
            return PolicyReloadObservation::Untagged;
        };
        let receipt = PolicyReloadReceipt {
            attempt: active_attempt,
            policy_generation: current.policy_generation,
        };
        if active_attempt != attempt {
            return PolicyReloadObservation::Different(receipt);
        }
        if current.policy.sandbox() != policy {
            return PolicyReloadObservation::ConflictingPolicy(receipt);
        }
        PolicyReloadObservation::Exact(receipt)
    }

    pub(crate) fn reload_for_attempt(
        &mut self,
        policy: CompiledEgressPolicy,
        attempt: PolicyReloadAttempt,
    ) -> std::result::Result<PolicyReloadReceipt, String> {
        match self.observe_reload(&policy, attempt) {
            PolicyReloadObservation::Exact(receipt) => return Ok(receipt),
            PolicyReloadObservation::ConflictingPolicy(receipt) => {
                return Err(format!(
                    "egress policy reload attempt {:?} is already active with different policy \
                     bytes at provider generation {}",
                    receipt.attempt(),
                    receipt.policy_generation().get()
                ));
            }
            PolicyReloadObservation::Different(active)
                if active.attempt().desired_generation() >= attempt.desired_generation()
                    || active.attempt().attempt_generation() >= attempt.attempt_generation() =>
            {
                return Err(format!(
                    "egress policy reload attempt {attempt:?} is stale relative to active attempt \
                     {:?}",
                    active.attempt()
                ));
            }
            PolicyReloadObservation::Untagged | PolicyReloadObservation::Different(_) => {}
        }

        let policy_generation = self
            .last_known_good
            .as_ref()
            .map(|current| current.policy_generation.next())
            .unwrap_or_else(PolicyGeneration::initial);
        self.last_known_good = Some(LastKnownGoodPolicy {
            policy_generation,
            reload_attempt: Some(attempt),
            policy: self.compose(policy),
        });
        Ok(PolicyReloadReceipt {
            attempt,
            policy_generation,
        })
    }

    pub(crate) fn readiness(&self, audit_healthy: bool) -> WorkloadPepReadiness {
        let policy_generation = self
            .last_known_good
            .as_ref()
            .map(|policy| policy.policy_generation);
        WorkloadPepReadiness {
            ready: policy_generation.is_some() && audit_healthy,
            audit_healthy,
            policy_generation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_egress::{EgressPolicy, EgressProtocol, EgressRule};

    fn attempt(desired: u64, provider: u64) -> PolicyReloadAttempt {
        PolicyReloadAttempt::new(
            NonZeroU64::new(desired).expect("desired generation should be nonzero"),
            NonZeroU64::new(provider).expect("attempt generation should be nonzero"),
        )
    }

    fn policy(name: &str, port: u16) -> CompiledEgressPolicy {
        EgressPolicy::new([EgressRule::new(
            name,
            EgressProtocol::Https,
            "example.com",
            port,
        )])
        .compile()
        .expect("fixture policy should compile")
    }

    #[test]
    fn readiness_fails_closed_when_audit_health_is_false() {
        let state = EgressProxyPolicyState::with_policy(CompiledEgressPolicy::deny_all());

        let readiness = state.readiness(false);

        assert!(
            !readiness.ready,
            "an unhealthy durable audit sink must make the PEP not ready"
        );
        assert!(
            !readiness.audit_healthy,
            "readiness should expose sticky audit health"
        );
        assert_eq!(
            readiness.policy_generation,
            Some(PolicyGeneration::initial()),
            "audit health must not erase the last-known policy generation"
        );
    }

    #[test]
    fn exact_reload_attempt_replay_returns_one_provider_generation() {
        let mut state = EgressProxyPolicyState::with_policy(CompiledEgressPolicy::deny_all());
        let desired = policy("exact-replay", 443);
        let attempt = attempt(2, 1);

        let first = state
            .reload_for_attempt(desired.clone(), attempt)
            .expect("first attempt should apply");
        let replay = state
            .reload_for_attempt(desired.clone(), attempt)
            .expect("exact replay should be idempotent");

        assert_eq!(first, replay);
        assert_eq!(first.policy_generation().get(), 2);
        assert_eq!(
            state.observe_reload(&desired, attempt),
            PolicyReloadObservation::Exact(first)
        );
    }

    #[test]
    fn attempt_identity_cannot_be_reused_for_different_policy() {
        let mut state = EgressProxyPolicyState::with_policy(CompiledEgressPolicy::deny_all());
        let attempt = attempt(2, 1);
        let first = policy("first-policy", 443);
        let substituted = policy("substituted-policy", 8443);
        state
            .reload_for_attempt(first, attempt)
            .expect("first attempt should apply");

        let error = state
            .reload_for_attempt(substituted.clone(), attempt)
            .expect_err("same attempt with different policy must fail closed");

        assert!(error.contains("different policy bytes"), "{error}");
        assert!(matches!(
            state.observe_reload(&substituted, attempt),
            PolicyReloadObservation::ConflictingPolicy(_)
        ));
    }

    #[test]
    fn stale_reload_attempt_cannot_replace_newer_provider_evidence() {
        let mut state = EgressProxyPolicyState::with_policy(CompiledEgressPolicy::deny_all());
        let current = attempt(3, 2);
        state
            .reload_for_attempt(policy("current-policy", 443), current)
            .expect("current attempt should apply");

        let error = state
            .reload_for_attempt(policy("stale-policy", 8443), attempt(2, 1))
            .expect_err("stale attempt must not replace current provider state");

        assert!(
            error.contains("stale relative to active attempt"),
            "{error}"
        );
    }
}
