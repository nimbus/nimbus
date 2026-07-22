use std::fmt;
use std::hash::{Hash, Hasher};
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{NimbusRuntimeError, Result};
use crate::limits::{RuntimePolicy, RuntimePoolKind};

/// The authority class that owns guest-mutated runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeOwnerClass {
    Tenant,
    System,
    Operator,
    Tooling,
}

/// Canonical identity for the principal incarnation allowed to reuse mutable
/// runtime state.
///
/// The audit label is deliberately excluded from equality and hashing. Tenant
/// owners must be projected from the Engine's canonical subject and durable
/// incarnation; labels are never authority.
#[derive(Clone)]
pub struct RuntimeOwnerId {
    class: RuntimeOwnerClass,
    stable_subject: Arc<str>,
    incarnation: NonZeroU64,
    audit_label: Option<Arc<str>>,
}

impl RuntimeOwnerId {
    pub fn tenant(
        stable_subject: impl Into<String>,
        canonical_incarnation: NonZeroU64,
        audit_label: Option<impl Into<String>>,
    ) -> Result<Self> {
        Self::new(
            RuntimeOwnerClass::Tenant,
            stable_subject,
            canonical_incarnation,
            audit_label,
        )
    }

    pub fn system_session(
        stable_subject: impl Into<String>,
        session_generation: NonZeroU64,
        audit_label: Option<impl Into<String>>,
    ) -> Result<Self> {
        Self::new(
            RuntimeOwnerClass::System,
            stable_subject,
            session_generation,
            audit_label,
        )
    }

    pub fn trusted_session(
        class: RuntimeOwnerClass,
        stable_subject: impl Into<String>,
        session_generation: NonZeroU64,
        audit_label: Option<impl Into<String>>,
    ) -> Result<Self> {
        if matches!(class, RuntimeOwnerClass::Tenant | RuntimeOwnerClass::System) {
            return Err(NimbusRuntimeError::Contract(
                "trusted runtime sessions must use the operator or tooling owner class".to_string(),
            ));
        }
        Self::new(class, stable_subject, session_generation, audit_label)
    }

    fn new(
        class: RuntimeOwnerClass,
        stable_subject: impl Into<String>,
        incarnation: NonZeroU64,
        audit_label: Option<impl Into<String>>,
    ) -> Result<Self> {
        let stable_subject = stable_subject.into();
        if stable_subject.trim().is_empty() {
            return Err(NimbusRuntimeError::Contract(
                "runtime owner stable subject must not be empty".to_string(),
            ));
        }
        let audit_label = audit_label.map(Into::into).map(Arc::<str>::from);
        Ok(Self {
            class,
            stable_subject: Arc::from(stable_subject),
            incarnation,
            audit_label,
        })
    }

    pub const fn class(&self) -> RuntimeOwnerClass {
        self.class
    }

    pub fn stable_subject(&self) -> &str {
        &self.stable_subject
    }

    pub const fn incarnation(&self) -> NonZeroU64 {
        self.incarnation
    }

    pub fn audit_label(&self) -> Option<&str> {
        self.audit_label.as_deref()
    }
}

impl PartialEq for RuntimeOwnerId {
    fn eq(&self, other: &Self) -> bool {
        self.class == other.class
            && self.stable_subject == other.stable_subject
            && self.incarnation == other.incarnation
    }
}

impl Eq for RuntimeOwnerId {}

impl Hash for RuntimeOwnerId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.class.hash(state);
        self.stable_subject.hash(state);
        self.incarnation.hash(state);
    }
}

impl fmt::Debug for RuntimeOwnerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeOwnerId")
            .field("class", &self.class)
            .field("stable_subject", &"<redacted>")
            .field("incarnation", &self.incarnation)
            .field("audit_label", &self.audit_label)
            .finish()
    }
}

#[derive(Debug)]
struct RuntimeOwnerLeaseState {
    owner_id: RuntimeOwnerId,
    revoked: AtomicBool,
}

/// Cloneable proof that one canonical runtime owner is still admitted.
#[derive(Clone)]
pub struct RuntimeOwnerLease {
    state: Arc<RuntimeOwnerLeaseState>,
}

impl RuntimeOwnerLease {
    pub fn owner_id(&self) -> &RuntimeOwnerId {
        &self.state.owner_id
    }

    pub fn is_revoked(&self) -> bool {
        self.state.revoked.load(Ordering::Acquire)
    }

    pub fn ensure_active(&self) -> Result<()> {
        if self.is_revoked() {
            return Err(NimbusRuntimeError::Contract(
                "runtime owner lease has been revoked".to_string(),
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for RuntimeOwnerLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeOwnerLease")
            .field("owner_id", self.owner_id())
            .field("revoked", &self.is_revoked())
            .finish()
    }
}

impl PartialEq for RuntimeOwnerLease {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for RuntimeOwnerLease {}

/// Manager-side revocation authority paired with a runtime owner lease.
#[derive(Clone)]
pub struct RuntimeOwnerRevocation {
    state: Arc<RuntimeOwnerLeaseState>,
}

impl RuntimeOwnerRevocation {
    pub fn owner_id(&self) -> &RuntimeOwnerId {
        &self.state.owner_id
    }

    pub fn revoke(&self) -> bool {
        !self.state.revoked.swap(true, Ordering::AcqRel)
    }

    pub fn is_revoked(&self) -> bool {
        self.state.revoked.load(Ordering::Acquire)
    }
}

impl fmt::Debug for RuntimeOwnerRevocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeOwnerRevocation")
            .field("owner_id", self.owner_id())
            .field("revoked", &self.is_revoked())
            .finish()
    }
}

/// Issuer owned by the compute runtime manager (or an explicitly trusted
/// embedder). It separates lease creation from lease use and keeps revocation
/// authority out of invocation contexts.
#[derive(Debug, Clone, Default)]
pub struct RuntimeOwnerLeaseIssuer;

impl RuntimeOwnerLeaseIssuer {
    pub fn issue(&self, owner_id: RuntimeOwnerId) -> (RuntimeOwnerLease, RuntimeOwnerRevocation) {
        let state = Arc::new(RuntimeOwnerLeaseState {
            owner_id,
            revoked: AtomicBool::new(false),
        });
        (
            RuntimeOwnerLease {
                state: state.clone(),
            },
            RuntimeOwnerRevocation { state },
        )
    }
}

/// Canonical deployment/configuration generation that is part of retained
/// state authority but independent from the tenant owner's lifetime.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RuntimeDeploymentAuthorityId {
    stable_deployment: Arc<str>,
    generation: NonZeroU64,
}

impl RuntimeDeploymentAuthorityId {
    pub fn new(stable_deployment: impl Into<String>, generation: NonZeroU64) -> Result<Self> {
        let stable_deployment = stable_deployment.into();
        if stable_deployment.trim().is_empty() {
            return Err(NimbusRuntimeError::Contract(
                "runtime deployment authority subject must not be empty".to_string(),
            ));
        }
        Ok(Self {
            stable_deployment: Arc::from(stable_deployment),
            generation,
        })
    }

    pub fn stable_deployment(&self) -> &str {
        &self.stable_deployment
    }

    pub const fn generation(&self) -> NonZeroU64 {
        self.generation
    }
}

impl fmt::Debug for RuntimeDeploymentAuthorityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeDeploymentAuthorityId")
            .field("stable_deployment", &"<redacted>")
            .field("generation", &self.generation)
            .finish()
    }
}

#[derive(Debug)]
struct RuntimeDeploymentAuthorityLeaseState {
    authority_id: RuntimeDeploymentAuthorityId,
    revoked: AtomicBool,
}

#[derive(Clone)]
pub struct RuntimeDeploymentAuthorityLease {
    state: Arc<RuntimeDeploymentAuthorityLeaseState>,
}

impl RuntimeDeploymentAuthorityLease {
    pub fn authority_id(&self) -> &RuntimeDeploymentAuthorityId {
        &self.state.authority_id
    }

    pub fn is_revoked(&self) -> bool {
        self.state.revoked.load(Ordering::Acquire)
    }

    pub fn ensure_active(&self) -> Result<()> {
        if self.is_revoked() {
            return Err(NimbusRuntimeError::Contract(
                "runtime deployment authority lease has been revoked".to_string(),
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for RuntimeDeploymentAuthorityLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeDeploymentAuthorityLease")
            .field("authority_id", self.authority_id())
            .field("revoked", &self.is_revoked())
            .finish()
    }
}

impl PartialEq for RuntimeDeploymentAuthorityLease {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

impl Eq for RuntimeDeploymentAuthorityLease {}

#[derive(Clone)]
pub struct RuntimeDeploymentAuthorityRevocation {
    state: Arc<RuntimeDeploymentAuthorityLeaseState>,
}

impl RuntimeDeploymentAuthorityRevocation {
    pub fn authority_id(&self) -> &RuntimeDeploymentAuthorityId {
        &self.state.authority_id
    }

    pub fn revoke(&self) -> bool {
        !self.state.revoked.swap(true, Ordering::AcqRel)
    }

    pub fn is_revoked(&self) -> bool {
        self.state.revoked.load(Ordering::Acquire)
    }
}

impl fmt::Debug for RuntimeDeploymentAuthorityRevocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeDeploymentAuthorityRevocation")
            .field("authority_id", self.authority_id())
            .field("revoked", &self.is_revoked())
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeDeploymentAuthorityLeaseIssuer;

impl RuntimeDeploymentAuthorityLeaseIssuer {
    pub fn issue(
        &self,
        authority_id: RuntimeDeploymentAuthorityId,
    ) -> (
        RuntimeDeploymentAuthorityLease,
        RuntimeDeploymentAuthorityRevocation,
    ) {
        let state = Arc::new(RuntimeDeploymentAuthorityLeaseState {
            authority_id,
            revoked: AtomicBool::new(false),
        });
        (
            RuntimeDeploymentAuthorityLease {
                state: state.clone(),
            },
            RuntimeDeploymentAuthorityRevocation { state },
        )
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeReuseAuthority<K> {
    owner_lease: RuntimeOwnerLease,
    deployment_lease: Option<RuntimeDeploymentAuthorityLease>,
    key: K,
}

impl<K> RuntimeReuseAuthority<K> {
    #[cfg(test)]
    pub(crate) fn new(owner_lease: RuntimeOwnerLease, key: K) -> Result<Self> {
        Self::new_with_deployment(owner_lease, None, key)
    }

    pub(crate) fn new_with_deployment(
        owner_lease: RuntimeOwnerLease,
        deployment_lease: Option<RuntimeDeploymentAuthorityLease>,
        key: K,
    ) -> Result<Self> {
        owner_lease.ensure_active()?;
        if let Some(deployment_lease) = &deployment_lease {
            deployment_lease.ensure_active()?;
        }
        Ok(Self {
            owner_lease,
            deployment_lease,
            key,
        })
    }

    pub(crate) fn owner_lease(&self) -> &RuntimeOwnerLease {
        &self.owner_lease
    }

    pub(crate) fn owner_id(&self) -> &RuntimeOwnerId {
        self.owner_lease.owner_id()
    }

    pub(crate) fn deployment_lease(&self) -> Option<&RuntimeDeploymentAuthorityLease> {
        self.deployment_lease.as_ref()
    }

    pub(crate) const fn key(&self) -> &K {
        &self.key
    }

    fn ensure_active(&self) -> Result<()> {
        self.owner_lease.ensure_active()?;
        if let Some(deployment_lease) = &self.deployment_lease {
            deployment_lease.ensure_active()?;
        }
        Ok(())
    }
}

impl<K: PartialEq> RuntimeReuseAuthority<K> {
    pub(crate) fn matches_exact(&self, other: &Self) -> bool {
        self.owner_lease == other.owner_lease
            && self.deployment_lease == other.deployment_lease
            && self.key == other.key
    }
}

impl<K: fmt::Debug> fmt::Debug for RuntimeReuseAuthority<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeReuseAuthority")
            .field("owner_id", self.owner_id())
            .field(
                "deployment_authority_id",
                &self
                    .deployment_lease()
                    .map(RuntimeDeploymentAuthorityLease::authority_id),
            )
            .field("key", &self.key)
            .finish()
    }
}

impl<K: PartialEq> PartialEq for RuntimeReuseAuthority<K> {
    fn eq(&self, other: &Self) -> bool {
        self.owner_id() == other.owner_id()
            && self.key == other.key
            && self
                .deployment_lease
                .as_ref()
                .map(|lease| lease.authority_id())
                == other
                    .deployment_lease
                    .as_ref()
                    .map(|lease| lease.authority_id())
    }
}

impl<K: Eq> Eq for RuntimeReuseAuthority<K> {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct OwnerPartitionedPoolStats {
    pub(crate) hits: usize,
    pub(crate) misses: usize,
    pub(crate) owner_mismatch_denials: usize,
    pub(crate) revoked_discards: usize,
    pub(crate) evictions: usize,
    pub(crate) retirements: usize,
}

pub(crate) struct RetainedCheckout<T, K> {
    value: T,
    authority: RuntimeReuseAuthority<K>,
}

impl<T, K> RetainedCheckout<T, K> {
    pub(crate) fn fresh(value: T, authority: RuntimeReuseAuthority<K>) -> Self {
        Self { value, authority }
    }

    pub(crate) fn value(&self) -> &T {
        &self.value
    }

    pub(crate) fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }

    pub(crate) fn authority(&self) -> &RuntimeReuseAuthority<K> {
        &self.authority
    }

    pub(crate) fn into_parts(self) -> (T, RuntimeReuseAuthority<K>) {
        (self.value, self.authority)
    }
}

struct OwnerPartitionedEntry<T, K> {
    checkout: RetainedCheckout<T, K>,
    last_used_sequence: u64,
}

/// Backend-neutral retained-state pool. Entries are partitioned first by an
/// exact live owner lease, then by the closed backend authority key.
pub(crate) struct OwnerPartitionedPool<T, K> {
    entries: Vec<OwnerPartitionedEntry<T, K>>,
    global_capacity: usize,
    per_owner_capacity: usize,
    next_sequence: u64,
    stats: OwnerPartitionedPoolStats,
}

impl<T, K> OwnerPartitionedPool<T, K>
where
    K: Clone + PartialEq,
{
    pub(crate) fn new(global_capacity: usize, per_owner_capacity: usize) -> Self {
        let global_capacity = global_capacity.max(1);
        Self {
            entries: Vec::new(),
            global_capacity,
            per_owner_capacity: per_owner_capacity.max(1).min(global_capacity),
            next_sequence: 1,
            stats: OwnerPartitionedPoolStats::default(),
        }
    }

    pub(crate) fn checkout(
        &mut self,
        authority: &RuntimeReuseAuthority<K>,
    ) -> Result<Option<RetainedCheckout<T, K>>> {
        authority.ensure_active()?;
        self.discard_revoked();
        let reusable_index = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.checkout.authority.matches_exact(authority))
            .max_by_key(|(_, entry)| entry.last_used_sequence)
            .map(|(index, _)| index);
        let Some(index) = reusable_index else {
            if self.entries.iter().any(|entry| {
                entry.checkout.authority.key() == authority.key()
                    && !entry.checkout.authority.matches_exact(authority)
            }) {
                self.stats.owner_mismatch_denials =
                    self.stats.owner_mismatch_denials.saturating_add(1);
            }
            self.stats.misses = self.stats.misses.saturating_add(1);
            return Ok(None);
        };
        self.stats.hits = self.stats.hits.saturating_add(1);
        Ok(Some(self.entries.swap_remove(index).checkout))
    }

    /// Returns `Some(value)` when revocation condemned the checked-out state.
    pub(crate) fn retain(&mut self, checkout: RetainedCheckout<T, K>) -> Option<T> {
        if checkout.authority.ensure_active().is_err() {
            self.stats.revoked_discards = self.stats.revoked_discards.saturating_add(1);
            return Some(checkout.value);
        }
        let owner_id = checkout.authority.owner_id().clone();
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.entries.push(OwnerPartitionedEntry {
            checkout,
            last_used_sequence: sequence,
        });
        while self.owner_len(&owner_id) > self.per_owner_capacity {
            if !self.evict_lru_for_owner(&owner_id) {
                break;
            }
        }
        while self.entries.len() > self.global_capacity {
            if !self.evict_fair_global_lru() {
                break;
            }
        }
        None
    }

    pub(crate) fn set_capacities(
        &mut self,
        global_capacity: usize,
        per_owner_capacity: usize,
    ) -> usize {
        self.global_capacity = global_capacity.max(1);
        self.per_owner_capacity = per_owner_capacity.max(1).min(self.global_capacity);
        let before = self.entries.len();
        let owners = self
            .entries
            .iter()
            .map(|entry| entry.checkout.authority.owner_id().clone())
            .collect::<std::collections::HashSet<_>>();
        for owner_id in owners {
            while self.owner_len(&owner_id) > self.per_owner_capacity {
                if !self.evict_lru_for_owner(&owner_id) {
                    break;
                }
            }
        }
        while self.entries.len() > self.global_capacity {
            if !self.evict_fair_global_lru() {
                break;
            }
        }
        before.saturating_sub(self.entries.len())
    }

    pub(crate) fn retire_owner(&mut self, owner_id: &RuntimeOwnerId) -> usize {
        self.retire_matching(|authority| authority.owner_id() == owner_id)
    }

    pub(crate) fn retire_deployment_authority(
        &mut self,
        authority_id: &RuntimeDeploymentAuthorityId,
    ) -> usize {
        self.retire_matching(|authority| {
            authority
                .deployment_lease()
                .is_some_and(|lease| lease.authority_id() == authority_id)
        })
    }

    #[cfg(test)]
    pub(crate) fn evict_global_lru(&mut self, count: usize) -> usize {
        let mut evicted = 0;
        for _ in 0..count {
            if !self.evict_fair_global_lru() {
                break;
            }
            evicted += 1;
        }
        evicted
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn owner_len(&self, owner_id: &RuntimeOwnerId) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.checkout.authority.owner_id() == owner_id)
            .count()
    }

    #[cfg(test)]
    pub(crate) fn most_recent(&self) -> Option<&T> {
        self.entries
            .iter()
            .max_by_key(|entry| entry.last_used_sequence)
            .map(|entry| entry.checkout.value())
    }

    pub(crate) const fn stats(&self) -> OwnerPartitionedPoolStats {
        self.stats
    }

    fn discard_revoked(&mut self) {
        let before = self.entries.len();
        self.entries.retain(|entry| {
            !entry.checkout.authority.owner_lease().is_revoked()
                && !entry
                    .checkout
                    .authority
                    .deployment_lease()
                    .is_some_and(RuntimeDeploymentAuthorityLease::is_revoked)
        });
        self.stats.revoked_discards = self
            .stats
            .revoked_discards
            .saturating_add(before.saturating_sub(self.entries.len()));
    }

    fn retire_matching(&mut self, predicate: impl Fn(&RuntimeReuseAuthority<K>) -> bool) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|entry| !predicate(entry.checkout.authority()));
        let retired = before.saturating_sub(self.entries.len());
        self.stats.retirements = self.stats.retirements.saturating_add(retired);
        retired
    }

    fn evict_lru_for_owner(&mut self, owner_id: &RuntimeOwnerId) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.checkout.authority.owner_id() == owner_id)
            .min_by_key(|(_, entry)| entry.last_used_sequence)
            .map(|(index, _)| index)
        else {
            return false;
        };
        self.entries.swap_remove(index);
        self.stats.evictions = self.stats.evictions.saturating_add(1);
        true
    }

    fn evict_fair_global_lru(&mut self) -> bool {
        let Some(max_owner_count) = self
            .entries
            .iter()
            .map(|entry| self.owner_len(entry.checkout.authority.owner_id()))
            .max()
        else {
            return false;
        };
        let Some(index) = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                self.owner_len(entry.checkout.authority.owner_id()) == max_owner_count
            })
            .min_by_key(|(_, entry)| entry.last_used_sequence)
            .map(|(index, _)| index)
        else {
            return false;
        };
        self.entries.swap_remove(index);
        self.stats.evictions = self.stats.evictions.saturating_add(1);
        true
    }
}

pub(crate) fn validate_retained_state_admission(
    policy: &RuntimePolicy,
    context: &crate::RuntimeInvocationContext,
) -> Result<()> {
    if let Some(owner_lease) = context.runtime_owner_lease() {
        owner_lease.ensure_active()?;
        if let Some(deployment_lease) = context.deployment_authority_lease() {
            deployment_lease.ensure_active()?;
        }
        return Ok(());
    }
    if matches!(
        policy.limits().runtime_pool_kind,
        RuntimePoolKind::WarmPool
            | RuntimePoolKind::WarmContextRecycle
            | RuntimePoolKind::RetainedStorePool
            | RuntimePoolKind::BunJscTrustedRetained
    ) {
        return Err(NimbusRuntimeError::Contract(format!(
            "runtime pool kind {:?} retains guest-mutated state and requires a runtime owner lease",
            policy.limits().runtime_pool_kind
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn nonzero(value: u64) -> NonZeroU64 {
        NonZeroU64::new(value).expect("fixture incarnation must be positive")
    }

    #[test]
    fn audit_labels_do_not_participate_in_owner_identity() {
        let first = RuntimeOwnerId::tenant("tenant-subject", nonzero(7), Some("alpha"))
            .expect("owner should build");
        let renamed = RuntimeOwnerId::tenant("tenant-subject", nonzero(7), Some("renamed"))
            .expect("owner should build");

        assert_eq!(first, renamed);
        assert_eq!(HashSet::from([first, renamed]).len(), 1);
    }

    #[test]
    fn subject_and_incarnation_are_both_authority_dimensions() {
        let first = RuntimeOwnerId::tenant("tenant-a", nonzero(1), Some("shared-label"))
            .expect("owner should build");
        let other_subject = RuntimeOwnerId::tenant("tenant-b", nonzero(1), Some("shared-label"))
            .expect("owner should build");
        let other_incarnation =
            RuntimeOwnerId::tenant("tenant-a", nonzero(2), Some("shared-label"))
                .expect("owner should build");

        assert_ne!(first, other_subject);
        assert_ne!(first, other_incarnation);
    }

    #[test]
    fn revocation_is_shared_by_every_lease_clone() {
        let owner = RuntimeOwnerId::tenant("tenant-a", nonzero(1), Some("tenant-a"))
            .expect("owner should build");
        let (lease, revocation) = RuntimeOwnerLeaseIssuer.issue(owner);
        let clone = lease.clone();

        assert!(lease.ensure_active().is_ok());
        assert!(revocation.revoke());
        assert!(!revocation.revoke(), "revocation must be idempotent");
        assert!(lease.ensure_active().is_err());
        assert!(clone.ensure_active().is_err());
    }

    fn owner_lease(subject: &str, incarnation: u64) -> RuntimeOwnerLease {
        let owner = RuntimeOwnerId::tenant(subject, nonzero(incarnation), Some("test-owner"))
            .expect("owner should build");
        RuntimeOwnerLeaseIssuer.issue(owner).0
    }

    #[test]
    fn owner_partition_requires_the_exact_live_lease() {
        let lease = owner_lease("tenant-a", 1);
        let reissued_same_id = owner_lease("tenant-a", 1);
        let authority =
            RuntimeReuseAuthority::new(lease.clone(), "bundle-a").expect("authority should build");
        let reissued_authority = RuntimeReuseAuthority::new(reissued_same_id, "bundle-a")
            .expect("authority should build");
        let mut pool = OwnerPartitionedPool::new(4, 2);
        assert!(
            pool.retain(RetainedCheckout::fresh("secret-a", authority.clone()))
                .is_none()
        );

        assert!(
            pool.checkout(&reissued_authority)
                .expect("checkout should be admitted")
                .is_none(),
            "reissuing an equal owner ID must not resurrect the old lease partition"
        );
        let reused = pool
            .checkout(&authority)
            .expect("checkout should be admitted")
            .expect("the exact lease should reuse its entry");
        assert_eq!(reused.value(), &"secret-a");
        assert_eq!(pool.stats().owner_mismatch_denials, 1);
    }

    #[test]
    fn owner_cap_evicts_locally_before_global_fairness_touches_another_owner() {
        let owner_a = owner_lease("tenant-a", 1);
        let owner_b = owner_lease("tenant-b", 1);
        let mut pool = OwnerPartitionedPool::new(4, 2);
        for (lease, key, value) in [
            (owner_a.clone(), "a-1", 1),
            (owner_b.clone(), "b-1", 2),
            (owner_a.clone(), "a-2", 3),
            (owner_a.clone(), "a-3", 4),
        ] {
            let authority = RuntimeReuseAuthority::new(lease, key).expect("authority should build");
            assert!(
                pool.retain(RetainedCheckout::fresh(value, authority))
                    .is_none()
            );
        }

        assert_eq!(pool.owner_len(owner_a.owner_id()), 2);
        assert_eq!(pool.owner_len(owner_b.owner_id()), 1);
        assert_eq!(pool.stats().evictions, 1);
        let owner_b_entry = pool
            .checkout(&RuntimeReuseAuthority::new(owner_b, "b-1").expect("authority should build"))
            .expect("checkout should be admitted")
            .expect("the other owner must retain its allocation");
        assert_eq!(owner_b_entry.value(), &2);
    }

    #[test]
    fn revoked_owner_and_deployment_are_discarded_before_reinsertion() {
        let owner = RuntimeOwnerId::tenant("tenant-a", nonzero(1), Some("tenant-a"))
            .expect("owner should build");
        let (owner_lease, owner_revocation) = RuntimeOwnerLeaseIssuer.issue(owner);
        let deployment = RuntimeDeploymentAuthorityId::new("deployment-a", nonzero(7))
            .expect("deployment should build");
        let (deployment_lease, deployment_revocation) =
            RuntimeDeploymentAuthorityLeaseIssuer.issue(deployment);
        let authority = RuntimeReuseAuthority::new_with_deployment(
            owner_lease,
            Some(deployment_lease),
            "bundle-a",
        )
        .expect("authority should build");
        let mut pool = OwnerPartitionedPool::new(4, 2);

        deployment_revocation.revoke();
        assert_eq!(
            pool.retain(RetainedCheckout::fresh("secret", authority)),
            Some("secret")
        );
        assert_eq!(pool.stats().revoked_discards, 1);
        assert_eq!(pool.len(), 0);
        assert!(owner_revocation.revoke());
    }

    #[test]
    fn concurrent_pressure_eviction_and_owner_retirement_drop_entry_once() {
        let owner = owner_lease("pressure-retirement-owner", 1);
        let authority =
            RuntimeReuseAuthority::new(owner.clone(), "bundle-a").expect("authority should build");
        let pool = Arc::new(std::sync::Mutex::new(OwnerPartitionedPool::new(2, 1)));
        assert!(
            pool.lock()
                .expect("pool lock should not be poisoned")
                .retain(RetainedCheckout::fresh("secret", authority))
                .is_none()
        );
        let start = Arc::new(std::sync::Barrier::new(3));

        let pressure = {
            let pool = pool.clone();
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                pool.lock()
                    .expect("pool lock should not be poisoned")
                    .evict_global_lru(1)
            })
        };
        let retirement = {
            let pool = pool.clone();
            let start = start.clone();
            let owner_id = owner.owner_id().clone();
            std::thread::spawn(move || {
                start.wait();
                pool.lock()
                    .expect("pool lock should not be poisoned")
                    .retire_owner(&owner_id)
            })
        };
        start.wait();
        let evicted = pressure.join().expect("pressure thread should join");
        let retired = retirement.join().expect("retirement thread should join");

        let pool = pool.lock().expect("pool lock should not be poisoned");
        assert_eq!(pool.len(), 0);
        assert_eq!(evicted + retired, 1, "the entry must be dropped once");
        assert_eq!(pool.stats().evictions + pool.stats().retirements, 1);
    }

    #[test]
    fn simultaneous_owner_and_deployment_revocation_condemns_checked_out_return() {
        let owner =
            RuntimeOwnerId::tenant("simultaneous-retirement-owner", nonzero(1), Some("tenant"))
                .expect("owner should build");
        let (owner_lease, owner_revocation) = RuntimeOwnerLeaseIssuer.issue(owner);
        let deployment = RuntimeDeploymentAuthorityId::new("deployment", nonzero(1))
            .expect("deployment should build");
        let (deployment_lease, deployment_revocation) =
            RuntimeDeploymentAuthorityLeaseIssuer.issue(deployment);
        let authority = RuntimeReuseAuthority::new_with_deployment(
            owner_lease,
            Some(deployment_lease),
            "bundle",
        )
        .expect("authority should build");
        let start = Arc::new(std::sync::Barrier::new(3));
        let owner_thread = {
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                owner_revocation.revoke()
            })
        };
        let deployment_thread = {
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                deployment_revocation.revoke()
            })
        };
        start.wait();
        assert!(owner_thread.join().expect("owner revocation should join"));
        assert!(
            deployment_thread
                .join()
                .expect("deployment revocation should join")
        );

        let mut pool = OwnerPartitionedPool::new(2, 1);
        assert_eq!(
            pool.retain(RetainedCheckout::fresh("checked-out-secret", authority)),
            Some("checked-out-secret")
        );
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.stats().revoked_discards, 1);
    }
}
