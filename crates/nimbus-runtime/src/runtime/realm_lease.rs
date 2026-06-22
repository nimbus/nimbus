#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "NFR3 defines the realm lease state machine before NFR4 wires it into NodeFull realm execution"
    )
)]

use std::cell::RefCell;
use std::error::Error;
use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use crate::execution_plan::{RuntimePoolAuthorityKey, RuntimePoolAuthorityMissingReason};
use crate::limits::RuntimeProfile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RuntimeRealmLeaseGeneration(u64);

impl RuntimeRealmLeaseGeneration {
    const INITIAL: Self = Self(0);

    fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeRealmLeaseState {
    BlankSubstrate,
    ContractInstalled,
    RealmReady,
    BundleLoaded,
    Invoking,
    Draining,
    Clean,
    Condemned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeRealmLeaseOwnerClass {
    Tenant,
    Operator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeRealmLeaseOwner {
    class: RuntimeRealmLeaseOwnerClass,
    stable_key: String,
}

impl RuntimeRealmLeaseOwner {
    pub(crate) fn tenant(stable_key: impl Into<String>) -> Self {
        Self {
            class: RuntimeRealmLeaseOwnerClass::Tenant,
            stable_key: stable_key.into(),
        }
    }

    #[cfg(test)]
    fn operator(stable_key: impl Into<String>) -> Self {
        Self {
            class: RuntimeRealmLeaseOwnerClass::Operator,
            stable_key: stable_key.into(),
        }
    }

    pub(crate) const fn class(&self) -> RuntimeRealmLeaseOwnerClass {
        self.class
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeRealmLeaseContract {
    owner: RuntimeRealmLeaseOwner,
    authority_key: RuntimePoolAuthorityKey,
    generation: RuntimeRealmLeaseGeneration,
}

impl RuntimeRealmLeaseContract {
    pub(crate) const fn owner(&self) -> &RuntimeRealmLeaseOwner {
        &self.owner
    }

    pub(crate) const fn authority_key(&self) -> &RuntimePoolAuthorityKey {
        &self.authority_key
    }

    pub(crate) const fn generation(&self) -> RuntimeRealmLeaseGeneration {
        self.generation
    }

    fn runtime_profile(&self) -> Option<RuntimeProfile> {
        self.authority_key.runtime_profile()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeRealmLeaseCondemnationReason {
    Dirty,
    Panicked,
    TimedOut,
    ExternalPressure,
    Abandoned,
    AuthorityMismatch,
    GenerationMismatch,
    OwnerMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeRealmLeaseEvictionReason {
    IdleTtlExpired,
    MemoryBudgetExceeded,
    CodeCacheBudgetExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeRealmLeaseMetricDecision {
    CheckoutRejected,
    ReturnedClean,
    Condemned,
    Evicted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeRealmLeaseMetricReason {
    Condemned(RuntimeRealmLeaseCondemnationReason),
    Evicted(RuntimeRealmLeaseEvictionReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeRealmLeaseMetricLabels {
    profile: Option<RuntimeProfile>,
    owner_class: RuntimeRealmLeaseOwnerClass,
    reason: Option<RuntimeRealmLeaseMetricReason>,
    decision: RuntimeRealmLeaseMetricDecision,
}

impl RuntimeRealmLeaseMetricLabels {
    pub(crate) fn for_contract(
        contract: &RuntimeRealmLeaseContract,
        reason: Option<RuntimeRealmLeaseCondemnationReason>,
        decision: RuntimeRealmLeaseMetricDecision,
    ) -> Self {
        Self {
            profile: contract.runtime_profile(),
            owner_class: contract.owner().class(),
            reason: reason.map(RuntimeRealmLeaseMetricReason::Condemned),
            decision,
        }
    }

    pub(crate) fn for_checkout_rejection(
        profile: Option<RuntimeProfile>,
        owner_class: RuntimeRealmLeaseOwnerClass,
    ) -> Self {
        Self {
            profile,
            owner_class,
            reason: None,
            decision: RuntimeRealmLeaseMetricDecision::CheckoutRejected,
        }
    }

    pub(crate) fn for_eviction(
        profile: Option<RuntimeProfile>,
        owner_class: RuntimeRealmLeaseOwnerClass,
        reason: RuntimeRealmLeaseEvictionReason,
    ) -> Self {
        Self {
            profile,
            owner_class,
            reason: Some(RuntimeRealmLeaseMetricReason::Evicted(reason)),
            decision: RuntimeRealmLeaseMetricDecision::Evicted,
        }
    }

    #[cfg(test)]
    const fn profile(&self) -> Option<RuntimeProfile> {
        self.profile
    }

    #[cfg(test)]
    const fn owner_class(&self) -> RuntimeRealmLeaseOwnerClass {
        self.owner_class
    }

    #[cfg(test)]
    const fn reason(&self) -> Option<RuntimeRealmLeaseMetricReason> {
        self.reason
    }

    #[cfg(test)]
    const fn decision(&self) -> RuntimeRealmLeaseMetricDecision {
        self.decision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeRealmLeaseRetentionPolicy {
    pub(crate) max_active_leases_per_owner: usize,
    pub(crate) max_retained_substrates_per_owner: usize,
    pub(crate) max_idle_age: Option<Duration>,
    pub(crate) max_retained_memory_bytes: Option<usize>,
    pub(crate) max_retained_code_cache_bytes: Option<usize>,
}

impl Default for RuntimeRealmLeaseRetentionPolicy {
    fn default() -> Self {
        Self {
            max_active_leases_per_owner: usize::MAX,
            max_retained_substrates_per_owner: usize::MAX,
            max_idle_age: None,
            max_retained_memory_bytes: None,
            max_retained_code_cache_bytes: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct RuntimeRealmLeasePoolLoad {
    pub(crate) active_leases_for_owner: usize,
    pub(crate) retained_substrates_for_owner: usize,
    pub(crate) idle_age: Duration,
    pub(crate) retained_memory_bytes: usize,
    pub(crate) retained_code_cache_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeRealmLeaseError {
    AuthorityKeyMissing(RuntimePoolAuthorityMissingReason),
    LeaseAlreadyActive {
        generation: RuntimeRealmLeaseGeneration,
    },
    LeaseAlreadyFinished {
        generation: RuntimeRealmLeaseGeneration,
    },
    LeaseNotActive,
    SubstrateCondemned {
        reason: RuntimeRealmLeaseCondemnationReason,
    },
    OwnerMismatch {
        expected: RuntimeRealmLeaseOwner,
        actual: RuntimeRealmLeaseOwner,
    },
    AuthorityKeyMismatch,
    GenerationMismatch {
        expected: RuntimeRealmLeaseGeneration,
        actual: RuntimeRealmLeaseGeneration,
    },
    InvalidTransition {
        from: RuntimeRealmLeaseState,
        to: RuntimeRealmLeaseState,
    },
    OwnerCapExceeded {
        owner_class: RuntimeRealmLeaseOwnerClass,
        active_leases_for_owner: usize,
        retained_substrates_for_owner: usize,
        max_active_leases_per_owner: usize,
        max_retained_substrates_per_owner: usize,
    },
    EvictionRequired {
        reason: RuntimeRealmLeaseEvictionReason,
    },
}

impl fmt::Display for RuntimeRealmLeaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthorityKeyMissing(reason) => {
                write!(f, "realm lease authority key is missing: {reason:?}")
            }
            Self::LeaseAlreadyActive { generation } => {
                write!(f, "realm lease generation {generation:?} is already active")
            }
            Self::LeaseAlreadyFinished { generation } => {
                write!(
                    f,
                    "realm lease generation {generation:?} is already finished"
                )
            }
            Self::LeaseNotActive => write!(f, "realm lease is not active"),
            Self::SubstrateCondemned { reason } => {
                write!(f, "realm substrate is condemned: {reason:?}")
            }
            Self::OwnerMismatch { expected, actual } => {
                write!(
                    f,
                    "realm lease owner mismatch: expected {expected:?}, got {actual:?}"
                )
            }
            Self::AuthorityKeyMismatch => write!(f, "realm lease authority key mismatch"),
            Self::GenerationMismatch { expected, actual } => {
                write!(
                    f,
                    "realm lease generation mismatch: expected {expected:?}, got {actual:?}"
                )
            }
            Self::InvalidTransition { from, to } => {
                write!(f, "invalid realm lease transition: {from:?} -> {to:?}")
            }
            Self::OwnerCapExceeded {
                owner_class,
                active_leases_for_owner,
                retained_substrates_for_owner,
                max_active_leases_per_owner,
                max_retained_substrates_per_owner,
            } => write!(
                f,
                "realm lease owner cap exceeded for {owner_class:?}: active={active_leases_for_owner}/{max_active_leases_per_owner}, retained={retained_substrates_for_owner}/{max_retained_substrates_per_owner}"
            ),
            Self::EvictionRequired { reason } => {
                write!(f, "realm lease substrate requires eviction: {reason:?}")
            }
        }
    }
}

impl Error for RuntimeRealmLeaseError {}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeRealmLeaseController {
    inner: Rc<RefCell<RuntimeRealmLeaseInner>>,
}

impl RuntimeRealmLeaseController {
    pub(crate) fn new(policy: RuntimeRealmLeaseRetentionPolicy) -> Self {
        Self {
            inner: Rc::new(RefCell::new(RuntimeRealmLeaseInner::new(policy))),
        }
    }

    pub(crate) fn checkout(
        &self,
        owner: RuntimeRealmLeaseOwner,
        authority_key: RuntimePoolAuthorityKey,
    ) -> Result<RuntimeRealmLease, RuntimeRealmLeaseError> {
        self.checkout_with_load(owner, authority_key, RuntimeRealmLeasePoolLoad::default())
    }

    pub(crate) fn checkout_with_load(
        &self,
        owner: RuntimeRealmLeaseOwner,
        authority_key: RuntimePoolAuthorityKey,
        load: RuntimeRealmLeasePoolLoad,
    ) -> Result<RuntimeRealmLease, RuntimeRealmLeaseError> {
        let contract = self
            .inner
            .borrow_mut()
            .checkout(owner, authority_key, load)?;
        Ok(RuntimeRealmLease {
            inner: self.inner.clone(),
            contract,
            finished: false,
        })
    }

    #[cfg(test)]
    fn state(&self) -> RuntimeRealmLeaseState {
        self.inner.borrow().state
    }

    #[cfg(test)]
    fn generation(&self) -> RuntimeRealmLeaseGeneration {
        self.inner.borrow().generation
    }

    #[cfg(test)]
    fn active_contract(&self) -> Option<RuntimeRealmLeaseContract> {
        self.inner.borrow().active_contract.clone()
    }

    #[cfg(test)]
    fn condemnation_reason(&self) -> Option<RuntimeRealmLeaseCondemnationReason> {
        self.inner.borrow().condemnation_reason
    }
}

#[must_use]
#[derive(Debug)]
pub(crate) struct RuntimeRealmLease {
    inner: Rc<RefCell<RuntimeRealmLeaseInner>>,
    contract: RuntimeRealmLeaseContract,
    finished: bool,
}

impl RuntimeRealmLease {
    pub(crate) const fn contract(&self) -> &RuntimeRealmLeaseContract {
        &self.contract
    }

    pub(crate) fn mark_realm_ready(&mut self) -> Result<(), RuntimeRealmLeaseError> {
        self.transition(RuntimeRealmLeaseState::RealmReady)
    }

    pub(crate) fn mark_bundle_loaded(&mut self) -> Result<(), RuntimeRealmLeaseError> {
        self.transition(RuntimeRealmLeaseState::BundleLoaded)
    }

    pub(crate) fn mark_invoking(&mut self) -> Result<(), RuntimeRealmLeaseError> {
        self.transition(RuntimeRealmLeaseState::Invoking)
    }

    pub(crate) fn mark_draining(&mut self) -> Result<(), RuntimeRealmLeaseError> {
        self.transition(RuntimeRealmLeaseState::Draining)
    }

    pub(crate) fn return_clean(
        &mut self,
        observed_contract: &RuntimeRealmLeaseContract,
    ) -> Result<RuntimeRealmLeaseMetricLabels, RuntimeRealmLeaseError> {
        if self.finished {
            return Err(RuntimeRealmLeaseError::LeaseAlreadyFinished {
                generation: self.contract.generation(),
            });
        }
        let result = self
            .inner
            .borrow_mut()
            .return_clean(&self.contract, observed_contract);
        if result.is_ok() || contract_error_condemns(&result) {
            self.finished = true;
        }
        result
    }

    pub(crate) fn condemn(
        &mut self,
        observed_contract: &RuntimeRealmLeaseContract,
        reason: RuntimeRealmLeaseCondemnationReason,
    ) -> Result<RuntimeRealmLeaseMetricLabels, RuntimeRealmLeaseError> {
        if self.finished {
            return Err(RuntimeRealmLeaseError::LeaseAlreadyFinished {
                generation: self.contract.generation(),
            });
        }
        let result =
            self.inner
                .borrow_mut()
                .condemn_active(&self.contract, observed_contract, reason);
        if result.is_ok() || contract_error_condemns(&result) {
            self.finished = true;
        }
        result
    }

    fn transition(&mut self, to: RuntimeRealmLeaseState) -> Result<(), RuntimeRealmLeaseError> {
        if self.finished {
            return Err(RuntimeRealmLeaseError::LeaseAlreadyFinished {
                generation: self.contract.generation(),
            });
        }
        self.inner.borrow_mut().transition(&self.contract, to)
    }
}

impl Drop for RuntimeRealmLease {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let _ = self.inner.borrow_mut().abandon_active(&self.contract);
        self.finished = true;
    }
}

#[derive(Debug)]
struct RuntimeRealmLeaseInner {
    state: RuntimeRealmLeaseState,
    generation: RuntimeRealmLeaseGeneration,
    retained_owner: Option<RuntimeRealmLeaseOwner>,
    retained_authority_key: Option<RuntimePoolAuthorityKey>,
    active_contract: Option<RuntimeRealmLeaseContract>,
    condemnation_reason: Option<RuntimeRealmLeaseCondemnationReason>,
    retention_policy: RuntimeRealmLeaseRetentionPolicy,
}

impl RuntimeRealmLeaseInner {
    fn new(retention_policy: RuntimeRealmLeaseRetentionPolicy) -> Self {
        Self {
            state: RuntimeRealmLeaseState::BlankSubstrate,
            generation: RuntimeRealmLeaseGeneration::INITIAL,
            retained_owner: None,
            retained_authority_key: None,
            active_contract: None,
            condemnation_reason: None,
            retention_policy,
        }
    }

    fn checkout(
        &mut self,
        owner: RuntimeRealmLeaseOwner,
        authority_key: RuntimePoolAuthorityKey,
        load: RuntimeRealmLeasePoolLoad,
    ) -> Result<RuntimeRealmLeaseContract, RuntimeRealmLeaseError> {
        if let RuntimePoolAuthorityKey::Missing(reason) = authority_key {
            return Err(RuntimeRealmLeaseError::AuthorityKeyMissing(reason));
        }
        if let Some(reason) = self.condemnation_reason {
            return Err(RuntimeRealmLeaseError::SubstrateCondemned { reason });
        }
        if self.active_contract.is_some() {
            return Err(RuntimeRealmLeaseError::LeaseAlreadyActive {
                generation: self.generation,
            });
        }
        self.require_owner_match(&owner)?;
        self.require_authority_match(&authority_key)?;
        self.retention_policy.evaluate(owner.class(), load)?;

        if self.retained_owner.is_none() {
            self.retained_owner = Some(owner.clone());
        }
        if self.retained_authority_key.is_none() {
            self.retained_authority_key = Some(authority_key.clone());
        }

        let contract = RuntimeRealmLeaseContract {
            owner,
            authority_key,
            generation: self.generation,
        };
        self.active_contract = Some(contract.clone());
        self.state = RuntimeRealmLeaseState::ContractInstalled;
        Ok(contract)
    }

    fn transition(
        &mut self,
        lease_contract: &RuntimeRealmLeaseContract,
        to: RuntimeRealmLeaseState,
    ) -> Result<(), RuntimeRealmLeaseError> {
        self.validate_active_contract(lease_contract)?;
        let expected_from = match to {
            RuntimeRealmLeaseState::RealmReady => RuntimeRealmLeaseState::ContractInstalled,
            RuntimeRealmLeaseState::BundleLoaded => RuntimeRealmLeaseState::RealmReady,
            RuntimeRealmLeaseState::Invoking => RuntimeRealmLeaseState::BundleLoaded,
            RuntimeRealmLeaseState::Draining => RuntimeRealmLeaseState::Invoking,
            RuntimeRealmLeaseState::BlankSubstrate
            | RuntimeRealmLeaseState::ContractInstalled
            | RuntimeRealmLeaseState::Clean
            | RuntimeRealmLeaseState::Condemned => {
                return Err(RuntimeRealmLeaseError::InvalidTransition {
                    from: self.state,
                    to,
                });
            }
        };
        if self.state != expected_from {
            return Err(RuntimeRealmLeaseError::InvalidTransition {
                from: self.state,
                to,
            });
        }
        self.state = to;
        Ok(())
    }

    fn return_clean(
        &mut self,
        lease_contract: &RuntimeRealmLeaseContract,
        observed_contract: &RuntimeRealmLeaseContract,
    ) -> Result<RuntimeRealmLeaseMetricLabels, RuntimeRealmLeaseError> {
        self.validate_return_contracts(lease_contract, observed_contract)?;
        if self.state != RuntimeRealmLeaseState::Draining {
            return Err(RuntimeRealmLeaseError::InvalidTransition {
                from: self.state,
                to: RuntimeRealmLeaseState::Clean,
            });
        }
        self.state = RuntimeRealmLeaseState::Clean;
        self.active_contract = None;
        self.generation = self.generation.next();
        Ok(RuntimeRealmLeaseMetricLabels::for_contract(
            observed_contract,
            None,
            RuntimeRealmLeaseMetricDecision::ReturnedClean,
        ))
    }

    fn condemn_active(
        &mut self,
        lease_contract: &RuntimeRealmLeaseContract,
        observed_contract: &RuntimeRealmLeaseContract,
        reason: RuntimeRealmLeaseCondemnationReason,
    ) -> Result<RuntimeRealmLeaseMetricLabels, RuntimeRealmLeaseError> {
        self.validate_return_contracts(lease_contract, observed_contract)?;
        self.condemn(reason);
        Ok(RuntimeRealmLeaseMetricLabels::for_contract(
            observed_contract,
            Some(reason),
            RuntimeRealmLeaseMetricDecision::Condemned,
        ))
    }

    fn abandon_active(
        &mut self,
        lease_contract: &RuntimeRealmLeaseContract,
    ) -> Result<(), RuntimeRealmLeaseError> {
        self.validate_active_contract(lease_contract)?;
        self.condemn(RuntimeRealmLeaseCondemnationReason::Abandoned);
        Ok(())
    }

    fn validate_return_contracts(
        &mut self,
        lease_contract: &RuntimeRealmLeaseContract,
        observed_contract: &RuntimeRealmLeaseContract,
    ) -> Result<(), RuntimeRealmLeaseError> {
        self.validate_active_contract(lease_contract)?;
        if let Err(error) = self.validate_observed_contract(observed_contract) {
            self.condemn(match error {
                RuntimeRealmLeaseError::OwnerMismatch { .. } => {
                    RuntimeRealmLeaseCondemnationReason::OwnerMismatch
                }
                RuntimeRealmLeaseError::AuthorityKeyMismatch => {
                    RuntimeRealmLeaseCondemnationReason::AuthorityMismatch
                }
                RuntimeRealmLeaseError::GenerationMismatch { .. } => {
                    RuntimeRealmLeaseCondemnationReason::GenerationMismatch
                }
                _ => RuntimeRealmLeaseCondemnationReason::Dirty,
            });
            return Err(error);
        }
        Ok(())
    }

    fn validate_active_contract(
        &self,
        lease_contract: &RuntimeRealmLeaseContract,
    ) -> Result<(), RuntimeRealmLeaseError> {
        if let Some(reason) = self.condemnation_reason {
            return Err(RuntimeRealmLeaseError::SubstrateCondemned { reason });
        }
        let Some(active_contract) = &self.active_contract else {
            return Err(RuntimeRealmLeaseError::LeaseNotActive);
        };
        if active_contract != lease_contract {
            return Err(RuntimeRealmLeaseError::GenerationMismatch {
                expected: active_contract.generation(),
                actual: lease_contract.generation(),
            });
        }
        Ok(())
    }

    fn validate_observed_contract(
        &self,
        observed_contract: &RuntimeRealmLeaseContract,
    ) -> Result<(), RuntimeRealmLeaseError> {
        let Some(active_contract) = &self.active_contract else {
            return Err(RuntimeRealmLeaseError::LeaseNotActive);
        };
        if active_contract.owner() != observed_contract.owner() {
            return Err(RuntimeRealmLeaseError::OwnerMismatch {
                expected: active_contract.owner().clone(),
                actual: observed_contract.owner().clone(),
            });
        }
        if active_contract.authority_key() != observed_contract.authority_key() {
            return Err(RuntimeRealmLeaseError::AuthorityKeyMismatch);
        }
        if active_contract.generation() != observed_contract.generation() {
            return Err(RuntimeRealmLeaseError::GenerationMismatch {
                expected: active_contract.generation(),
                actual: observed_contract.generation(),
            });
        }
        Ok(())
    }

    fn require_owner_match(
        &self,
        owner: &RuntimeRealmLeaseOwner,
    ) -> Result<(), RuntimeRealmLeaseError> {
        let Some(retained_owner) = &self.retained_owner else {
            return Ok(());
        };
        if retained_owner == owner {
            return Ok(());
        }
        Err(RuntimeRealmLeaseError::OwnerMismatch {
            expected: retained_owner.clone(),
            actual: owner.clone(),
        })
    }

    fn require_authority_match(
        &self,
        authority_key: &RuntimePoolAuthorityKey,
    ) -> Result<(), RuntimeRealmLeaseError> {
        let Some(retained_authority_key) = &self.retained_authority_key else {
            return Ok(());
        };
        if retained_authority_key == authority_key {
            return Ok(());
        }
        Err(RuntimeRealmLeaseError::AuthorityKeyMismatch)
    }

    fn condemn(&mut self, reason: RuntimeRealmLeaseCondemnationReason) {
        self.state = RuntimeRealmLeaseState::Condemned;
        self.active_contract = None;
        self.condemnation_reason = Some(reason);
    }
}

impl RuntimeRealmLeaseRetentionPolicy {
    fn evaluate(
        &self,
        owner_class: RuntimeRealmLeaseOwnerClass,
        load: RuntimeRealmLeasePoolLoad,
    ) -> Result<(), RuntimeRealmLeaseError> {
        if load.active_leases_for_owner >= self.max_active_leases_per_owner {
            return Err(RuntimeRealmLeaseError::OwnerCapExceeded {
                owner_class,
                active_leases_for_owner: load.active_leases_for_owner,
                retained_substrates_for_owner: load.retained_substrates_for_owner,
                max_active_leases_per_owner: self.max_active_leases_per_owner,
                max_retained_substrates_per_owner: self.max_retained_substrates_per_owner,
            });
        }
        if load.retained_substrates_for_owner >= self.max_retained_substrates_per_owner {
            return Err(RuntimeRealmLeaseError::OwnerCapExceeded {
                owner_class,
                active_leases_for_owner: load.active_leases_for_owner,
                retained_substrates_for_owner: load.retained_substrates_for_owner,
                max_active_leases_per_owner: self.max_active_leases_per_owner,
                max_retained_substrates_per_owner: self.max_retained_substrates_per_owner,
            });
        }
        if self
            .max_idle_age
            .is_some_and(|max_idle_age| load.idle_age > max_idle_age)
        {
            return Err(RuntimeRealmLeaseError::EvictionRequired {
                reason: RuntimeRealmLeaseEvictionReason::IdleTtlExpired,
            });
        }
        if self
            .max_retained_memory_bytes
            .is_some_and(|max_bytes| load.retained_memory_bytes > max_bytes)
        {
            return Err(RuntimeRealmLeaseError::EvictionRequired {
                reason: RuntimeRealmLeaseEvictionReason::MemoryBudgetExceeded,
            });
        }
        if self
            .max_retained_code_cache_bytes
            .is_some_and(|max_bytes| load.retained_code_cache_bytes > max_bytes)
        {
            return Err(RuntimeRealmLeaseError::EvictionRequired {
                reason: RuntimeRealmLeaseEvictionReason::CodeCacheBudgetExceeded,
            });
        }
        Ok(())
    }
}

fn contract_error_condemns(
    result: &Result<RuntimeRealmLeaseMetricLabels, RuntimeRealmLeaseError>,
) -> bool {
    matches!(
        result,
        Err(RuntimeRealmLeaseError::OwnerMismatch { .. })
            | Err(RuntimeRealmLeaseError::AuthorityKeyMismatch)
            | Err(RuntimeRealmLeaseError::GenerationMismatch { .. })
            | Err(RuntimeRealmLeaseError::SubstrateCondemned { .. })
    )
}

#[cfg(test)]
mod tests {
    use crate::execution_plan::RuntimePoolAuthorityFacts;
    use crate::limits::RuntimeProfile;

    use super::*;

    fn controller() -> RuntimeRealmLeaseController {
        RuntimeRealmLeaseController::new(RuntimeRealmLeaseRetentionPolicy::default())
    }

    fn owner_a() -> RuntimeRealmLeaseOwner {
        RuntimeRealmLeaseOwner::tenant("tenant-a")
    }

    fn owner_b() -> RuntimeRealmLeaseOwner {
        RuntimeRealmLeaseOwner::tenant("tenant-b")
    }

    fn node_authority(grants: &[&str]) -> RuntimePoolAuthorityKey {
        RuntimePoolAuthorityKey::Exact(RuntimePoolAuthorityFacts::new(
            RuntimeProfile::NodeFull,
            grants.iter().map(|grant| (*grant).to_string()).collect(),
        ))
    }

    fn web_authority() -> RuntimePoolAuthorityKey {
        RuntimePoolAuthorityKey::Exact(RuntimePoolAuthorityFacts::new(
            RuntimeProfile::WebLean,
            Vec::new(),
        ))
    }

    fn checkout_ready_to_drain(controller: &RuntimeRealmLeaseController) -> RuntimeRealmLease {
        let mut lease = controller
            .checkout(owner_a(), node_authority(&["db"]))
            .expect("lease should checkout");
        lease.mark_realm_ready().expect("realm should become ready");
        lease
            .mark_bundle_loaded()
            .expect("bundle should become loaded");
        lease.mark_invoking().expect("lease should invoke");
        lease.mark_draining().expect("lease should drain");
        lease
    }

    #[test]
    fn lease_state_machine_accepts_clean_lifecycle_and_advances_generation() {
        let controller = controller();
        let mut lease = checkout_ready_to_drain(&controller);
        let contract = lease.contract().clone();

        let labels = lease
            .return_clean(&contract)
            .expect("drained lease should return clean");

        assert_eq!(controller.state(), RuntimeRealmLeaseState::Clean);
        assert_eq!(controller.active_contract(), None);
        assert_eq!(controller.generation(), RuntimeRealmLeaseGeneration(1));
        assert_eq!(labels.profile(), Some(RuntimeProfile::NodeFull));
        assert_eq!(labels.owner_class(), RuntimeRealmLeaseOwnerClass::Tenant);
        assert_eq!(labels.reason(), None);
        assert_eq!(
            labels.decision(),
            RuntimeRealmLeaseMetricDecision::ReturnedClean
        );
    }

    #[test]
    fn second_active_lease_is_rejected_per_isolate() {
        let controller = controller();
        let _lease = controller
            .checkout(owner_a(), node_authority(&["db"]))
            .expect("first lease should checkout");

        let error = controller
            .checkout(owner_a(), node_authority(&["db"]))
            .expect_err("second active lease must be rejected");

        assert_eq!(
            error,
            RuntimeRealmLeaseError::LeaseAlreadyActive {
                generation: RuntimeRealmLeaseGeneration(0)
            }
        );
        assert_eq!(
            controller.state(),
            RuntimeRealmLeaseState::ContractInstalled
        );
    }

    #[test]
    fn invalid_transitions_and_double_return_are_errors() {
        let controller = controller();
        let mut lease = controller
            .checkout(owner_a(), node_authority(&["db"]))
            .expect("lease should checkout");
        let contract = lease.contract().clone();

        assert_eq!(
            lease.mark_bundle_loaded(),
            Err(RuntimeRealmLeaseError::InvalidTransition {
                from: RuntimeRealmLeaseState::ContractInstalled,
                to: RuntimeRealmLeaseState::BundleLoaded,
            })
        );
        assert_eq!(
            lease.return_clean(&contract),
            Err(RuntimeRealmLeaseError::InvalidTransition {
                from: RuntimeRealmLeaseState::ContractInstalled,
                to: RuntimeRealmLeaseState::Clean,
            })
        );

        lease.mark_realm_ready().expect("realm should become ready");
        lease
            .mark_bundle_loaded()
            .expect("bundle should become loaded");
        lease.mark_invoking().expect("lease should invoke");
        lease.mark_draining().expect("lease should drain");
        lease
            .return_clean(&contract)
            .expect("drained lease should return clean");

        assert_eq!(
            lease.return_clean(&contract),
            Err(RuntimeRealmLeaseError::LeaseAlreadyFinished {
                generation: RuntimeRealmLeaseGeneration(0),
            })
        );
    }

    #[test]
    fn cross_tenant_checkout_is_rejected_before_contract_installation() {
        let controller = controller();
        let mut lease = checkout_ready_to_drain(&controller);
        let contract = lease.contract().clone();
        lease
            .return_clean(&contract)
            .expect("first owner should return clean");

        let error = controller
            .checkout(owner_b(), node_authority(&["db"]))
            .expect_err("cross-tenant substrate reclaim must be rejected");

        assert_eq!(
            error,
            RuntimeRealmLeaseError::OwnerMismatch {
                expected: owner_a(),
                actual: owner_b(),
            }
        );
        assert_eq!(controller.state(), RuntimeRealmLeaseState::Clean);
        assert_eq!(controller.active_contract(), None);
        assert_eq!(controller.generation(), RuntimeRealmLeaseGeneration(1));
    }

    #[test]
    fn authority_key_mismatch_is_rejected_before_contract_installation() {
        let controller = controller();
        let mut lease = checkout_ready_to_drain(&controller);
        let contract = lease.contract().clone();
        lease
            .return_clean(&contract)
            .expect("first authority should return clean");

        let error = controller
            .checkout(owner_a(), web_authority())
            .expect_err("authority key mismatch must be rejected");

        assert_eq!(error, RuntimeRealmLeaseError::AuthorityKeyMismatch);
        assert_eq!(controller.state(), RuntimeRealmLeaseState::Clean);
        assert_eq!(controller.active_contract(), None);
    }

    #[test]
    fn stale_generation_return_condemns_substrate() {
        let controller = controller();
        let mut first = checkout_ready_to_drain(&controller);
        let stale_contract = first.contract().clone();
        first
            .return_clean(&stale_contract)
            .expect("first lease should return clean");

        let mut second = checkout_ready_to_drain(&controller);
        let error = second
            .return_clean(&stale_contract)
            .expect_err("stale generation return must fail");

        assert_eq!(
            error,
            RuntimeRealmLeaseError::GenerationMismatch {
                expected: RuntimeRealmLeaseGeneration(1),
                actual: RuntimeRealmLeaseGeneration(0),
            }
        );
        assert_eq!(controller.state(), RuntimeRealmLeaseState::Condemned);
        assert_eq!(
            controller.condemnation_reason(),
            Some(RuntimeRealmLeaseCondemnationReason::GenerationMismatch)
        );
        assert!(matches!(
            controller.checkout(owner_a(), node_authority(&["db"])),
            Err(RuntimeRealmLeaseError::SubstrateCondemned {
                reason: RuntimeRealmLeaseCondemnationReason::GenerationMismatch
            })
        ));
    }

    #[test]
    fn mismatched_return_contract_condemns_substrate() {
        let controller = controller();
        let mut lease = checkout_ready_to_drain(&controller);
        let mut wrong_contract = lease.contract().clone();
        wrong_contract.owner = owner_b();

        let error = lease
            .return_clean(&wrong_contract)
            .expect_err("wrong owner return must fail");

        assert_eq!(
            error,
            RuntimeRealmLeaseError::OwnerMismatch {
                expected: owner_a(),
                actual: owner_b(),
            }
        );
        assert_eq!(controller.state(), RuntimeRealmLeaseState::Condemned);
        assert_eq!(
            controller.condemnation_reason(),
            Some(RuntimeRealmLeaseCondemnationReason::OwnerMismatch)
        );
    }

    #[test]
    fn dirty_timeout_panic_and_pressure_returns_are_non_reusable() {
        for reason in [
            RuntimeRealmLeaseCondemnationReason::Dirty,
            RuntimeRealmLeaseCondemnationReason::Panicked,
            RuntimeRealmLeaseCondemnationReason::TimedOut,
            RuntimeRealmLeaseCondemnationReason::ExternalPressure,
        ] {
            let controller = controller();
            let mut lease = controller
                .checkout(owner_a(), node_authority(&["db"]))
                .expect("lease should checkout");
            let contract = lease.contract().clone();

            let labels = lease
                .condemn(&contract, reason)
                .expect("explicit non-reusable return should condemn");

            assert_eq!(controller.state(), RuntimeRealmLeaseState::Condemned);
            assert_eq!(controller.condemnation_reason(), Some(reason));
            assert_eq!(
                labels.reason(),
                Some(RuntimeRealmLeaseMetricReason::Condemned(reason))
            );
            assert_eq!(
                labels.decision(),
                RuntimeRealmLeaseMetricDecision::Condemned
            );
            assert!(matches!(
                controller.checkout(owner_a(), node_authority(&["db"])),
                Err(RuntimeRealmLeaseError::SubstrateCondemned {
                    reason: observed
                }) if observed == reason
            ));
        }
    }

    #[test]
    fn abandoned_in_flight_lease_condemns_on_drop() {
        let controller = controller();
        {
            let _lease = controller
                .checkout(owner_a(), node_authority(&["db"]))
                .expect("lease should checkout");
            assert_eq!(
                controller.state(),
                RuntimeRealmLeaseState::ContractInstalled
            );
        }

        assert_eq!(controller.state(), RuntimeRealmLeaseState::Condemned);
        assert_eq!(
            controller.condemnation_reason(),
            Some(RuntimeRealmLeaseCondemnationReason::Abandoned)
        );
    }

    #[test]
    fn missing_authority_key_fails_closed() {
        let controller = controller();
        let error = controller
            .checkout(
                owner_a(),
                RuntimePoolAuthorityKey::Missing(
                    RuntimePoolAuthorityMissingReason::PermissionProfile,
                ),
            )
            .expect_err("realm lease requires an exact authority key");

        assert_eq!(
            error,
            RuntimeRealmLeaseError::AuthorityKeyMissing(
                RuntimePoolAuthorityMissingReason::PermissionProfile
            )
        );
        assert_eq!(controller.state(), RuntimeRealmLeaseState::BlankSubstrate);
    }

    #[test]
    fn owner_caps_reject_checkout_without_changing_authority_contract() {
        let policy = RuntimeRealmLeaseRetentionPolicy {
            max_active_leases_per_owner: 1,
            max_retained_substrates_per_owner: 2,
            ..RuntimeRealmLeaseRetentionPolicy::default()
        };
        let controller = RuntimeRealmLeaseController::new(policy);
        let error = controller
            .checkout_with_load(
                owner_a(),
                node_authority(&["db"]),
                RuntimeRealmLeasePoolLoad {
                    active_leases_for_owner: 1,
                    retained_substrates_for_owner: 0,
                    ..RuntimeRealmLeasePoolLoad::default()
                },
            )
            .expect_err("owner cap should reject checkout");

        assert_eq!(
            error,
            RuntimeRealmLeaseError::OwnerCapExceeded {
                owner_class: RuntimeRealmLeaseOwnerClass::Tenant,
                active_leases_for_owner: 1,
                retained_substrates_for_owner: 0,
                max_active_leases_per_owner: 1,
                max_retained_substrates_per_owner: 2,
            }
        );
        assert_eq!(controller.state(), RuntimeRealmLeaseState::BlankSubstrate);
    }

    #[test]
    fn authority_matching_precedes_owner_caps_and_eviction_decisions() {
        let policy = RuntimeRealmLeaseRetentionPolicy {
            max_active_leases_per_owner: 1,
            max_retained_substrates_per_owner: 1,
            max_idle_age: Some(Duration::from_secs(0)),
            max_retained_memory_bytes: Some(1),
            max_retained_code_cache_bytes: Some(1),
        };
        let controller = RuntimeRealmLeaseController::new(policy);
        let mut lease = checkout_ready_to_drain(&controller);
        let contract = lease.contract().clone();
        lease
            .return_clean(&contract)
            .expect("first authority should return clean");

        let error = controller
            .checkout_with_load(
                owner_a(),
                node_authority(&["other-service"]),
                RuntimeRealmLeasePoolLoad {
                    active_leases_for_owner: usize::MAX,
                    retained_substrates_for_owner: usize::MAX,
                    idle_age: Duration::from_secs(60),
                    retained_memory_bytes: usize::MAX,
                    retained_code_cache_bytes: usize::MAX,
                },
            )
            .expect_err("authority mismatch must win over caps and eviction");

        assert_eq!(error, RuntimeRealmLeaseError::AuthorityKeyMismatch);
        assert_eq!(controller.state(), RuntimeRealmLeaseState::Clean);
        assert_eq!(controller.active_contract(), None);
    }

    #[test]
    fn ttl_memory_and_code_cache_budget_hooks_request_eviction() {
        for (policy, load, reason) in [
            (
                RuntimeRealmLeaseRetentionPolicy {
                    max_idle_age: Some(Duration::from_secs(1)),
                    ..RuntimeRealmLeaseRetentionPolicy::default()
                },
                RuntimeRealmLeasePoolLoad {
                    idle_age: Duration::from_secs(2),
                    ..RuntimeRealmLeasePoolLoad::default()
                },
                RuntimeRealmLeaseEvictionReason::IdleTtlExpired,
            ),
            (
                RuntimeRealmLeaseRetentionPolicy {
                    max_retained_memory_bytes: Some(64),
                    ..RuntimeRealmLeaseRetentionPolicy::default()
                },
                RuntimeRealmLeasePoolLoad {
                    retained_memory_bytes: 65,
                    ..RuntimeRealmLeasePoolLoad::default()
                },
                RuntimeRealmLeaseEvictionReason::MemoryBudgetExceeded,
            ),
            (
                RuntimeRealmLeaseRetentionPolicy {
                    max_retained_code_cache_bytes: Some(32),
                    ..RuntimeRealmLeaseRetentionPolicy::default()
                },
                RuntimeRealmLeasePoolLoad {
                    retained_code_cache_bytes: 33,
                    ..RuntimeRealmLeasePoolLoad::default()
                },
                RuntimeRealmLeaseEvictionReason::CodeCacheBudgetExceeded,
            ),
        ] {
            let controller = RuntimeRealmLeaseController::new(policy);
            let error = controller
                .checkout_with_load(owner_a(), node_authority(&["db"]), load)
                .expect_err("retention hook should request eviction");

            assert_eq!(error, RuntimeRealmLeaseError::EvictionRequired { reason });
            assert_eq!(controller.state(), RuntimeRealmLeaseState::BlankSubstrate);
        }
    }

    #[test]
    fn metric_labels_are_bounded_to_profile_owner_class_reason_and_decision() {
        let controller = controller();
        let mut lease = controller
            .checkout(
                RuntimeRealmLeaseOwner::operator("operator-a"),
                node_authority(&["db"]),
            )
            .expect("lease should checkout");
        let contract = lease.contract().clone();

        let labels = lease
            .condemn(&contract, RuntimeRealmLeaseCondemnationReason::Dirty)
            .expect("dirty lease should produce bounded labels");

        assert_eq!(labels.profile(), Some(RuntimeProfile::NodeFull));
        assert_eq!(labels.owner_class(), RuntimeRealmLeaseOwnerClass::Operator);
        assert_eq!(
            labels.reason(),
            Some(RuntimeRealmLeaseMetricReason::Condemned(
                RuntimeRealmLeaseCondemnationReason::Dirty
            ))
        );
        assert_eq!(
            labels.decision(),
            RuntimeRealmLeaseMetricDecision::Condemned
        );

        let rejection = RuntimeRealmLeaseMetricLabels::for_checkout_rejection(
            Some(RuntimeProfile::NodeFull),
            RuntimeRealmLeaseOwnerClass::Tenant,
        );
        assert_eq!(rejection.profile(), Some(RuntimeProfile::NodeFull));
        assert_eq!(rejection.owner_class(), RuntimeRealmLeaseOwnerClass::Tenant);
        assert_eq!(rejection.reason(), None);
        assert_eq!(
            rejection.decision(),
            RuntimeRealmLeaseMetricDecision::CheckoutRejected
        );

        let eviction = RuntimeRealmLeaseMetricLabels::for_eviction(
            Some(RuntimeProfile::NodeFull),
            RuntimeRealmLeaseOwnerClass::Tenant,
            RuntimeRealmLeaseEvictionReason::IdleTtlExpired,
        );
        assert_eq!(eviction.profile(), Some(RuntimeProfile::NodeFull));
        assert_eq!(eviction.owner_class(), RuntimeRealmLeaseOwnerClass::Tenant);
        assert_eq!(
            eviction.reason(),
            Some(RuntimeRealmLeaseMetricReason::Evicted(
                RuntimeRealmLeaseEvictionReason::IdleTtlExpired
            ))
        );
        assert_eq!(
            eviction.decision(),
            RuntimeRealmLeaseMetricDecision::Evicted
        );
    }
}
