use nimbus_egress::CompiledEgressPolicy;

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
pub struct EgressProxyReadiness {
    pub ready: bool,
    pub policy_generation: Option<PolicyGeneration>,
}

#[derive(Debug, Clone)]
pub(crate) struct LastKnownGoodPolicy {
    pub(crate) policy_generation: PolicyGeneration,
    pub(crate) policy: CompiledEgressPolicy,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EgressProxyPolicyState {
    last_known_good: Option<LastKnownGoodPolicy>,
}

impl EgressProxyPolicyState {
    pub(crate) fn with_policy(policy: CompiledEgressPolicy) -> Self {
        Self {
            last_known_good: Some(LastKnownGoodPolicy {
                policy_generation: PolicyGeneration::initial(),
                policy,
            }),
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
            policy,
        });
        next_generation
    }

    pub(crate) fn readiness(&self) -> EgressProxyReadiness {
        EgressProxyReadiness {
            ready: self.last_known_good.is_some(),
            policy_generation: self
                .last_known_good
                .as_ref()
                .map(|policy| policy.policy_generation),
        }
    }
}
