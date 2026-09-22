#[cfg(any(test, feature = "test-hooks"))]
use nimbus_core::SequenceNumber;
use nimbus_core::{Error, Result};

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

#[derive(Clone, Default)]
pub(crate) struct CommitFaultClient {
    #[cfg(any(test, feature = "test-hooks"))]
    state: Arc<CommitFaultState>,
}

impl CommitFaultClient {
    #[inline]
    pub(crate) fn wait(&self, label: Label) -> Fault {
        #[cfg(any(test, feature = "test-hooks"))]
        {
            self.state.wait(label)
        }
        #[cfg(not(any(test, feature = "test-hooks")))]
        {
            let _ = label;
            Fault::Noop
        }
    }

    #[inline]
    pub(crate) fn is_armed(&self, label: Label) -> bool {
        #[cfg(any(test, feature = "test-hooks"))]
        {
            self.state
                .registry
                .lock()
                .expect("execution unit commit fault lock should not be poisoned")
                .armed
                .contains_key(&label)
        }
        #[cfg(not(any(test, feature = "test-hooks")))]
        {
            let _ = label;
            false
        }
    }

    #[cfg(any(test, feature = "test-hooks"))]
    pub(crate) fn handle(&self) -> CommitFaultHandle {
        CommitFaultHandle {
            state: self.state.clone(),
        }
    }
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Clone)]
pub struct CommitFaultHandle {
    state: Arc<CommitFaultState>,
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
#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Default)]
struct CommitFaultRegistry {
    armed: HashMap<Label, ArmedFault>,
    hits: HashMap<Label, usize>,
}

#[cfg(any(test, feature = "test-hooks"))]
impl CommitFaultHandle {
    pub fn arm(&self, label: Label) {
        let mut registry = self
            .state
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        let armed = &mut registry.armed;
        match armed.entry(label) {
            Entry::Vacant(entry) => entry.insert(ArmedFault::Pause {
                entered: false,
                released: false,
            }),
            Entry::Occupied(_) => panic!("commit fault label {label:?} is already armed"),
        };
    }

    pub fn inject(&self, label: Label, fault: Fault) {
        let mut registry = self
            .state
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        let armed = &mut registry.armed;
        match fault {
            Fault::Noop => {
                armed.remove(&label);
                self.state.condvar.notify_all();
            }
            Fault::Error(error) => {
                match armed.entry(label) {
                    Entry::Vacant(entry) => entry.insert(ArmedFault::Error(error)),
                    Entry::Occupied(_) => {
                        panic!("commit fault label {label:?} is already armed")
                    }
                };
            }
        }
    }

    pub fn inject_error_on_nth_hit(&self, label: Label, hit: usize, error: Error) {
        assert!(hit > 0, "fault hit index must be positive");
        let mut registry = self
            .state
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        let armed = &mut registry.armed;
        match armed.entry(label) {
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
        let mut registry = self
            .state
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        let armed = &mut registry.armed;
        match armed.entry(label) {
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
        let mut registry = self
            .state
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        let armed = &mut registry.armed;
        match armed.entry(label) {
            Entry::Vacant(entry) => entry.insert(ArmedFault::RetryableConflicts {
                remaining: count,
                conflicting_sequence,
            }),
            Entry::Occupied(_) => panic!("commit fault label {label:?} is already armed"),
        };
    }

    pub fn hit_count(&self, label: Label) -> usize {
        self.state
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned")
            .hits
            .get(&label)
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
        let mut registry = self
            .state
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        loop {
            if registry.hits.get(&label).copied().unwrap_or_default() >= expected {
                return true;
            }
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return false;
            };
            let (next, result) = self
                .state
                .hits_condvar
                .wait_timeout(registry, remaining)
                .expect("execution unit commit fault hit wait should not be poisoned");
            registry = next;
            if result.timed_out()
                && registry.hits.get(&label).copied().unwrap_or_default() < expected
            {
                return false;
            }
        }
    }

    pub fn wait_until_entered(&self, label: Label, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut registry = self
            .state
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        loop {
            if matches!(
                registry.armed.get(&label),
                Some(ArmedFault::Pause { entered: true, .. })
            ) {
                return true;
            }
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return false;
            };
            let (next, result) = self
                .state
                .condvar
                .wait_timeout(registry, remaining)
                .expect("execution unit commit fault wait should not be poisoned");
            registry = next;
            if result.timed_out()
                && !matches!(
                    registry.armed.get(&label),
                    Some(ArmedFault::Pause { entered: true, .. })
                )
            {
                return false;
            }
        }
    }

    pub fn release(&self, label: Label) {
        let mut registry = self
            .state
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        let armed = &mut registry.armed;
        let Some(ArmedFault::Pause { released, .. }) = armed.get_mut(&label) else {
            panic!("commit fault label {label:?} is not armed as a pause");
        };
        *released = true;
        self.state.condvar.notify_all();
    }
}

#[cfg(any(test, feature = "test-hooks"))]
impl CommitFaultState {
    fn wait(&self, label: Label) -> Fault {
        let mut registry = self
            .registry
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        *registry.hits.entry(label).or_default() += 1;
        // Waiters wake only after this critical section releases the lock, so
        // the hit becomes visible together with the fault decision below.
        self.hits_condvar.notify_all();
        let Some(fault) = registry.armed.remove(&label) else {
            return Fault::Noop;
        };
        match fault {
            ArmedFault::Error(error) => Fault::Error(error),
            ArmedFault::ErrorOnNthHit { remaining, error } => {
                if remaining > 1 {
                    registry.armed.insert(
                        label,
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
                        label,
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
                        label,
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
                    label,
                    ArmedFault::Pause {
                        entered: true,
                        released,
                    },
                );
                Fault::Noop
            }
            ArmedFault::Pause { released, .. } => {
                registry.armed.insert(
                    label,
                    ArmedFault::Pause {
                        entered: true,
                        released,
                    },
                );
                self.condvar.notify_all();
                loop {
                    match registry.armed.get(&label) {
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
                                            registry.armed.get(&label),
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
                                    registry.armed.get(&label),
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
                            registry.armed.remove(&label);
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

    #[test]
    fn observed_hit_already_carries_its_fault() {
        // A caller that waits for hit `n` and then lets other commits proceed
        // must find the fault already delivered to hit `n`; a later commit at
        // the same label must not be able to take it.
        for _ in 0..500 {
            let client = CommitFaultClient::default();
            let faults = client.handle();
            faults.inject_retryable_conflicts(labels::PRE_ASSIGN, 1, Some(SequenceNumber(7)));
            let first = {
                let client = client.clone();
                std::thread::spawn(move || client.wait(labels::PRE_ASSIGN))
            };
            assert!(faults.wait_until_hits(labels::PRE_ASSIGN, 1, Duration::from_secs(5)));
            assert!(
                !client.is_armed(labels::PRE_ASSIGN),
                "hit 1 became visible before its fault was decided"
            );
            assert!(matches!(client.wait(labels::PRE_ASSIGN), Fault::Noop));
            let first = first.join().expect("first commit thread should join");
            assert!(
                matches!(
                    &first,
                    Fault::Error(error) if error.conflicting_sequence() == Some(SequenceNumber(7))
                ),
                "hit 1 should receive the injected conflict, got {first:?}"
            );
        }
    }
}
