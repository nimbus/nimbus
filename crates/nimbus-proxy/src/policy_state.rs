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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadPepReadiness {
    pub ready: bool,
    pub policy_generation: Option<PolicyGeneration>,
}

#[derive(Debug, Clone)]
pub(crate) struct LastKnownGoodPolicy {
    pub(crate) policy_generation: PolicyGeneration,
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

    pub(crate) fn reload(&mut self, policy: CompiledEgressPolicy) -> PolicyGeneration {
        let next_generation = self
            .last_known_good
            .as_ref()
            .map(|current| current.policy_generation.next())
            .unwrap_or_else(PolicyGeneration::initial);
        self.last_known_good = Some(LastKnownGoodPolicy {
            policy_generation: next_generation,
            policy: self.compose(policy),
        });
        next_generation
    }

    pub(crate) fn readiness(&self) -> WorkloadPepReadiness {
        WorkloadPepReadiness {
            ready: self.last_known_good.is_some(),
            policy_generation: self
                .last_known_good
                .as_ref()
                .map(|policy| policy.policy_generation),
        }
    }
}
