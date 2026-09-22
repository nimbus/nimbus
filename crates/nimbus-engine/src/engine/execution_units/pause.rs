#[cfg(any(test, feature = "test-hooks"))]
use nimbus_core::SequenceNumber;
use nimbus_core::{Error, Result, TenantId};

#[cfg(any(test, feature = "test-hooks"))]
use std::collections::{HashMap, hash_map::Entry};
#[cfg(any(test, feature = "test-hooks"))]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(any(test, feature = "test-hooks"))]
use std::time::{Duration, Instant};

#[cfg(any(test, feature = "test-hooks"))]
const COMMIT_FAULT_RELEASE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Label(&'static str);

impl Label {
    const fn new(value: &'static str) -> Self {
        Self(value)
    }
}

pub mod labels {
    use super::Label;

    pub const PREPARE_COMPLETE: Label = Label::new("PREPARE_COMPLETE");
    pub const PRE_ASSIGN: Label = Label::new("PRE_ASSIGN");
    pub const JOURNAL_ASSIGN_AFTER_STAGE: Label = Label::new("JOURNAL_ASSIGN_AFTER_STAGE");
    pub const POST_VALIDATE_PRE_STAGE: Label = Label::new("POST_VALIDATE_PRE_STAGE");
    pub const PRE_PERSIST: Label = Label::new("PRE_PERSIST");
    pub const DURABLE_BEFORE_PUBLISH: Label = Label::new("DURABLE_BEFORE_PUBLISH");
    pub const SCHEMA_ASSIGNED_BEFORE_VISIBLE: Label = Label::new("SCHEMA_ASSIGNED_BEFORE_VISIBLE");
    pub const SCHEDULER_DURABLE_BEFORE_ACK: Label = Label::new("SCHEDULER_DURABLE_BEFORE_ACK");
    pub const POST_PUBLISH_PRE_FANOUT: Label = Label::new("POST_PUBLISH_PRE_FANOUT");
}

#[derive(Debug, Default)]
pub enum Fault {
    #[default]
    Noop,
    #[cfg_attr(
        not(any(test, feature = "test-hooks")),
        expect(dead_code, reason = "constructed by the test-only fault controller")
    )]
    Error(Error),
}

impl Fault {
    pub(crate) fn into_result(self) -> Result<()> {
        match self {
            Self::Noop => Ok(()),
            Self::Error(error) => Err(error),
        }
    }
}

/// The commits that a fault applies to.
///
/// A tenant-scoped fault is taken only by a commit of that tenant. An
/// engine-wide fault is taken by the next commit of any tenant that reaches the
/// label, which includes reserved tenants such as the `_nimbus` projection. A
/// commit consults its tenant scope before the engine scope.
#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FaultScope {
    Engine,
    Tenant(TenantId),
}

#[cfg(any(test, feature = "test-hooks"))]
type FaultKey = (Label, FaultScope);

#[derive(Clone, Default)]
pub(crate) struct CommitFaultClient {
    #[cfg(any(test, feature = "test-hooks"))]
    state: Arc<CommitFaultState>,
}

impl CommitFaultClient {
    /// Counts one hit at `label` for `tenant` and returns the fault that this
    /// hit receives.
    #[inline]
    pub(crate) fn wait(&self, label: Label, tenant: &TenantId) -> Fault {
        #[cfg(any(test, feature = "test-hooks"))]
        {
            self.state.wait(label, tenant)
        }
        #[cfg(not(any(test, feature = "test-hooks")))]
        {
            let _ = (label, tenant);
            Fault::Noop
        }
    }

    /// Reports whether a commit of `tenant` would find a fault at `label` in
    /// either scope.
    #[inline]
    pub(crate) fn is_armed(&self, label: Label, tenant: &TenantId) -> bool {
        #[cfg(any(test, feature = "test-hooks"))]
        {
            let registry = self
                .state
                .registry
                .lock()
                .expect("execution unit commit fault lock should not be poisoned");
            registry
                .armed
                .contains_key(&(label, FaultScope::Tenant(tenant.clone())))
                || registry.armed.contains_key(&(label, FaultScope::Engine))
        }
        #[cfg(not(any(test, feature = "test-hooks")))]
        {
            let _ = (label, tenant);
            false
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn faults(&self) -> CommitFaults {
        CommitFaults {
            state: self.state.clone(),
        }
    }
}

/// Entry point for tests that inject commit faults. It only chooses a scope;
/// every injector lives on the scoped [`CommitFaultHandle`], so a test states
/// which commits it targets before it can arm anything.
#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone)]
pub struct CommitFaults {
    state: Arc<CommitFaultState>,
}

#[cfg(any(test, feature = "test-hooks"))]
impl CommitFaults {
    /// Faults that only commits of `tenant` take and hits that only commits of
    /// `tenant` count.
    pub fn for_tenant(&self, tenant: &TenantId) -> CommitFaultHandle {
        CommitFaultHandle {
            state: self.state.clone(),
            scope: FaultScope::Tenant(tenant.clone()),
        }
    }

    /// Faults that the next commit of any tenant takes and hits that every
    /// tenant's commits count. A tenant-scoped fault at the same label wins for
    /// that tenant.
    pub fn engine_wide(&self) -> CommitFaultHandle {
        CommitFaultHandle {
            state: self.state.clone(),
            scope: FaultScope::Engine,
        }
    }
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone)]
pub struct CommitFaultHandle {
    state: Arc<CommitFaultState>,
    scope: FaultScope,
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug)]
enum ArmedFault {
    Pause {
        entered: bool,
        released: bool,
    },
    Error(Error),
    ErrorOnNthHit {
        remaining: usize,
        error: Error,
    },
    PanicOnNthHit {
        remaining: usize,
    },
    RetryableConflicts {
        remaining: usize,
        conflicting_sequence: Option<SequenceNumber>,
    },
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Default)]
struct CommitFaultState {
    registry: Mutex<CommitFaultRegistry>,
    condvar: Condvar,
    hits_condvar: Condvar,
}

/// Hit counts and armed faults share one lock so that counting a hit and
/// deciding its fault are one step. A waiter that observes hit `n` therefore
/// also observes the fault that hit `n` received; a concurrent commit cannot
/// take that fault between the two.
///
/// Both maps are keyed by label and scope. A hit counts once under its
/// tenant's scope and once under the engine scope.
#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Default)]
struct CommitFaultRegistry {
    armed: HashMap<FaultKey, ArmedFault>,
    hits: HashMap<FaultKey, usize>,
}

#[cfg(any(test, feature = "test-hooks"))]
impl CommitFaultHandle {
    pub fn scope(&self) -> &FaultScope {
        &self.scope
    }

    fn key(&self, label: Label) -> FaultKey {
        (label, self.scope.clone())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CommitFaultRegistry> {
        self.state
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned")
    }

    pub fn arm(&self, label: Label) {
        let mut registry = self.lock();
        match registry.armed.entry(self.key(label)) {
            Entry::Vacant(entry) => entry.insert(ArmedFault::Pause {
                entered: false,
                released: false,
            }),
            Entry::Occupied(_) => panic!("commit fault label {label:?} is already armed"),
        };
    }

    pub fn inject(&self, label: Label, fault: Fault) {
        let mut registry = self.lock();
        let key = self.key(label);
        match fault {
            Fault::Noop => {
                registry.armed.remove(&key);
            }
            Fault::Error(error) => match registry.armed.entry(key) {
                Entry::Vacant(entry) => {
                    entry.insert(ArmedFault::Error(error));
                }
                Entry::Occupied(_) => {
                    panic!("commit fault label {label:?} is already armed")
                }
            },
        }
    }

    pub fn inject_error_on_nth_hit(&self, label: Label, hit: usize, error: Error) {
        assert!(hit > 0, "fault hit index must be positive");
        let mut registry = self.lock();
        match registry.armed.entry(self.key(label)) {
            Entry::Vacant(entry) => {
                entry.insert(ArmedFault::ErrorOnNthHit {
                    remaining: hit,
                    error,
                });
            }
            Entry::Occupied(_) => panic!("commit fault label {label:?} is already armed"),
        }
    }

    pub fn inject_panic_on_nth_hit(&self, label: Label, hit: usize) {
        assert!(hit > 0, "fault hit index must be positive");
        let mut registry = self.lock();
        match registry.armed.entry(self.key(label)) {
            Entry::Vacant(entry) => {
                entry.insert(ArmedFault::PanicOnNthHit { remaining: hit });
            }
            Entry::Occupied(_) => panic!("commit fault label {label:?} is already armed"),
        }
    }

    pub fn inject_retryable_conflicts(
        &self,
        label: Label,
        count: usize,
        conflicting_sequence: Option<SequenceNumber>,
    ) {
        assert!(count > 0, "retryable conflict count must be positive");
        let mut registry = self.lock();
        match registry.armed.entry(self.key(label)) {
            Entry::Vacant(entry) => entry.insert(ArmedFault::RetryableConflicts {
                remaining: count,
                conflicting_sequence,
            }),
            Entry::Occupied(_) => panic!("commit fault label {label:?} is already armed"),
        };
    }

    /// Hits at `label` within this handle's scope. The engine scope counts
    /// every tenant's commits; a tenant scope counts only that tenant's.
    pub fn hit_count(&self, label: Label) -> usize {
        self.lock()
            .hits
            .get(&self.key(label))
            .copied()
            .unwrap_or_default()
    }

    pub fn wait_until_hits(
        &self,
        label: Label,
        expected: usize,
        timeout: std::time::Duration,
    ) -> bool {
        let deadline = Instant::now() + timeout;
        let key = self.key(label);
        let mut registry = self.lock();
        loop {
            if registry.hits.get(&key).copied().unwrap_or_default() >= expected {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let remaining = deadline - now;
            let (next, result) = self
                .state
                .hits_condvar
                .wait_timeout(registry, remaining)
                .expect("execution unit commit fault hit wait should not be poisoned");
            registry = next;
            if result.timed_out() && registry.hits.get(&key).copied().unwrap_or_default() < expected
            {
                return false;
            }
        }
    }

    pub fn wait_until_entered(&self, label: Label, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let key = self.key(label);
        let mut registry = self.lock();
        loop {
            if matches!(
                registry.armed.get(&key),
                Some(ArmedFault::Pause { entered: true, .. })
            ) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            let remaining = deadline - now;
            let (next, result) = self
                .state
                .condvar
                .wait_timeout(registry, remaining)
                .expect("execution unit commit fault wait should not be poisoned");
            registry = next;
            if result.timed_out()
                && !matches!(
                    registry.armed.get(&key),
                    Some(ArmedFault::Pause { entered: true, .. })
                )
            {
                return false;
            }
        }
    }

    pub fn release(&self, label: Label) {
        let mut registry = self.lock();
        let Some(ArmedFault::Pause { released, .. }) = registry.armed.get_mut(&self.key(label))
        else {
            panic!("commit fault label {label:?} is not armed as a pause");
        };
        *released = true;
        self.state.condvar.notify_all();
    }
}

#[cfg(any(test, feature = "test-hooks"))]
impl CommitFaultState {
    fn wait(&self, label: Label, tenant: &TenantId) -> Fault {
        let tenant_key = (label, FaultScope::Tenant(tenant.clone()));
        let engine_key = (label, FaultScope::Engine);
        let mut registry = self
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        *registry.hits.entry(tenant_key.clone()).or_default() += 1;
        *registry.hits.entry(engine_key.clone()).or_default() += 1;
        // Waiters wake only after this critical section releases the lock, so
        // the hit becomes visible together with the fault decision below.
        self.hits_condvar.notify_all();
        let (key, fault) = if let Some(fault) = registry.armed.remove(&tenant_key) {
            (tenant_key, fault)
        } else if let Some(fault) = registry.armed.remove(&engine_key) {
            (engine_key, fault)
        } else {
            return Fault::Noop;
        };
        match fault {
            ArmedFault::Error(error) => Fault::Error(error),
            ArmedFault::ErrorOnNthHit { remaining, error } => {
                if remaining > 1 {
                    registry.armed.insert(
                        key,
                        ArmedFault::ErrorOnNthHit {
                            remaining: remaining - 1,
                            error,
                        },
                    );
                    Fault::Noop
                } else {
                    Fault::Error(error)
                }
            }
            ArmedFault::PanicOnNthHit { remaining } => {
                if remaining > 1 {
                    registry.armed.insert(
                        key,
                        ArmedFault::PanicOnNthHit {
                            remaining: remaining - 1,
                        },
                    );
                    Fault::Noop
                } else {
                    drop(registry);
                    panic!("injected commit fault panic at {label:?}")
                }
            }
            ArmedFault::RetryableConflicts {
                remaining,
                conflicting_sequence,
            } => {
                if remaining > 1 {
                    registry.armed.insert(
                        key,
                        ArmedFault::RetryableConflicts {
                            remaining: remaining - 1,
                            conflicting_sequence,
                        },
                    );
                }
                Fault::Error(Error::retryable_conflict(
                    "injected optimistic conflict",
                    conflicting_sequence,
                ))
            }
            ArmedFault::Pause {
                entered: true,
                released,
            } => {
                registry.armed.insert(
                    key,
                    ArmedFault::Pause {
                        entered: true,
                        released,
                    },
                );
                Fault::Noop
            }
            ArmedFault::Pause { released, .. } => {
                registry.armed.insert(
                    key.clone(),
                    ArmedFault::Pause {
                        entered: true,
                        released,
                    },
                );
                self.condvar.notify_all();
                loop {
                    match registry.armed.get(&key) {
                        Some(ArmedFault::Pause {
                            released: false, ..
                        }) => {
                            let (next, _) = self
                                .condvar
                                .wait_timeout_while(
                                    registry,
                                    COMMIT_FAULT_RELEASE_TIMEOUT,
                                    |registry| {
                                        matches!(
                                            registry.armed.get(&key),
                                            Some(ArmedFault::Pause {
                                                released: false,
                                                ..
                                            })
                                        )
                                    },
                                )
                                .expect("execution unit commit fault wait should not be poisoned");
                            registry = next;
                            assert!(
                                !matches!(
                                    registry.armed.get(&key),
                                    Some(ArmedFault::Pause {
                                        released: false,
                                        ..
                                    })
                                ),
                                "commit fault pause {label:?} was not released within \
                                 {COMMIT_FAULT_RELEASE_TIMEOUT:?}; the test likely exited before \
                                 calling release()"
                            );
                        }
                        Some(ArmedFault::Pause { released: true, .. }) => {
                            registry.armed.remove(&key);
                            return Fault::Noop;
                        }
                        Some(
                            ArmedFault::Error(_)
                            | ArmedFault::ErrorOnNthHit { .. }
                            | ArmedFault::PanicOnNthHit { .. }
                            | ArmedFault::RetryableConflicts { .. },
                        ) => {
                            unreachable!("an entered pause cannot change fault kind")
                        }
                        None => return Fault::Noop,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(name: &str) -> TenantId {
        TenantId::new(name).expect("test tenant id should be valid")
    }

    fn conflict_at(fault: &Fault, sequence: u64) -> bool {
        matches!(
            fault,
            Fault::Error(error) if error.conflicting_sequence() == Some(SequenceNumber(sequence))
        )
    }

    #[test]
    fn observed_hit_already_carries_its_fault() {
        // A caller that waits for hit `n` and then lets other commits proceed
        // must find the fault already delivered to hit `n`; a later commit at
        // the same label must not be able to take it.
        let alpha = tenant("alpha");
        for _ in 0..500 {
            let client = CommitFaultClient::default();
            let faults = client.faults().for_tenant(&alpha);
            faults.inject_retryable_conflicts(labels::PRE_ASSIGN, 1, Some(SequenceNumber(7)));
            let first = {
                let client = client.clone();
                let alpha = alpha.clone();
                std::thread::spawn(move || client.wait(labels::PRE_ASSIGN, &alpha))
            };
            assert!(faults.wait_until_hits(labels::PRE_ASSIGN, 1, Duration::from_secs(5)));
            assert!(
                !client.is_armed(labels::PRE_ASSIGN, &alpha),
                "hit 1 became visible before its fault was decided"
            );
            assert!(matches!(
                client.wait(labels::PRE_ASSIGN, &alpha),
                Fault::Noop
            ));
            let first = first.join().expect("first commit thread should join");
            assert!(
                conflict_at(&first, 7),
                "hit 1 should receive the injected conflict, got {first:?}"
            );
        }
    }

    #[test]
    fn tenant_scoped_fault_ignores_other_tenants() {
        let alpha = tenant("alpha");
        let beta = tenant("beta");
        let client = CommitFaultClient::default();
        let faults = client.faults().for_tenant(&alpha);
        faults.inject_retryable_conflicts(labels::PRE_ASSIGN, 1, Some(SequenceNumber(3)));

        assert!(matches!(
            client.wait(labels::PRE_ASSIGN, &beta),
            Fault::Noop
        ));
        assert!(client.is_armed(labels::PRE_ASSIGN, &alpha));
        assert!(!client.is_armed(labels::PRE_ASSIGN, &beta));
        assert_eq!(faults.hit_count(labels::PRE_ASSIGN), 0);
        assert_eq!(
            client.faults().engine_wide().hit_count(labels::PRE_ASSIGN),
            1
        );

        let taken = client.wait(labels::PRE_ASSIGN, &alpha);
        assert!(
            conflict_at(&taken, 3),
            "alpha should take its fault, got {taken:?}"
        );
        assert_eq!(faults.hit_count(labels::PRE_ASSIGN), 1);
        assert!(!client.is_armed(labels::PRE_ASSIGN, &alpha));
    }

    #[test]
    fn engine_wide_fault_is_taken_by_any_tenant_after_tenant_scope() {
        let alpha = tenant("alpha");
        let beta = tenant("beta");
        let client = CommitFaultClient::default();
        client.faults().engine_wide().inject_retryable_conflicts(
            labels::PRE_ASSIGN,
            1,
            Some(SequenceNumber(1)),
        );
        client
            .faults()
            .for_tenant(&alpha)
            .inject_retryable_conflicts(labels::PRE_ASSIGN, 1, Some(SequenceNumber(2)));

        // Alpha consults its own scope first and leaves the engine fault alone.
        let alpha_fault = client.wait(labels::PRE_ASSIGN, &alpha);
        assert!(conflict_at(&alpha_fault, 2), "got {alpha_fault:?}");
        assert!(client.is_armed(labels::PRE_ASSIGN, &beta));

        // Any tenant takes the engine-wide fault.
        let beta_fault = client.wait(labels::PRE_ASSIGN, &beta);
        assert!(conflict_at(&beta_fault, 1), "got {beta_fault:?}");
        assert!(!client.is_armed(labels::PRE_ASSIGN, &alpha));
        assert!(!client.is_armed(labels::PRE_ASSIGN, &beta));
    }

    #[test]
    fn pause_is_released_through_the_scope_that_armed_it() {
        let alpha = tenant("alpha");
        let client = CommitFaultClient::default();
        let faults = client.faults().for_tenant(&alpha);
        faults.arm(labels::PRE_PERSIST);
        let paused = {
            let client = client.clone();
            let alpha = alpha.clone();
            std::thread::spawn(move || client.wait(labels::PRE_PERSIST, &alpha))
        };
        assert!(faults.wait_until_entered(labels::PRE_PERSIST, Duration::from_secs(5)));
        assert!(
            !client
                .faults()
                .engine_wide()
                .wait_until_entered(labels::PRE_PERSIST, Duration::from_millis(50)),
            "an engine-wide handle must not observe a tenant-scoped pause"
        );
        faults.release(labels::PRE_PERSIST);
        assert!(matches!(
            paused.join().expect("paused commit should join"),
            Fault::Noop
        ));
        assert!(!client.is_armed(labels::PRE_PERSIST, &alpha));
    }
}
