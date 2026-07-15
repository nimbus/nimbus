#[cfg(any(test, feature = "test-hooks"))]
use nimbus_core::SequenceNumber;
use nimbus_core::{Error, Result};

#[cfg(any(test, feature = "test-hooks"))]
use std::collections::{HashMap, hash_map::Entry};
#[cfg(any(test, feature = "test-hooks"))]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(any(test, feature = "test-hooks"))]
use std::time::Instant;

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
    pub const POST_VALIDATE_PRE_STAGE: Label = Label::new("POST_VALIDATE_PRE_STAGE");
    pub const PRE_PERSIST: Label = Label::new("PRE_PERSIST");
    pub const DURABLE_BEFORE_PUBLISH: Label = Label::new("DURABLE_BEFORE_PUBLISH");
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
    RetryableConflicts {
        remaining: usize,
        conflicting_sequence: Option<SequenceNumber>,
    },
}

#[cfg(any(test, feature = "test-hooks"))]
#[derive(Debug, Default)]
struct CommitFaultState {
    armed: Mutex<HashMap<Label, ArmedFault>>,
    hits: Mutex<HashMap<Label, usize>>,
    condvar: Condvar,
    hits_condvar: Condvar,
}

#[cfg(any(test, feature = "test-hooks"))]
impl CommitFaultHandle {
    pub fn arm(&self, label: Label) {
        let mut armed = self
            .state
            .armed
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        match armed.entry(label) {
            Entry::Vacant(entry) => entry.insert(ArmedFault::Pause {
                entered: false,
                released: false,
            }),
            Entry::Occupied(_) => panic!("commit fault label {label:?} is already armed"),
        };
    }

    pub fn inject(&self, label: Label, fault: Fault) {
        let mut armed = self
            .state
            .armed
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
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

    pub fn inject_retryable_conflicts(
        &self,
        label: Label,
        count: usize,
        conflicting_sequence: Option<SequenceNumber>,
    ) {
        assert!(count > 0, "retryable conflict count must be positive");
        let mut armed = self
            .state
            .armed
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
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
            .hits
            .lock()
            .expect("execution unit commit fault hit lock should not be poisoned")
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
        let mut hits = self
            .state
            .hits
            .lock()
            .expect("execution unit commit fault hit lock should not be poisoned");
        loop {
            if hits.get(&label).copied().unwrap_or_default() >= expected {
                return true;
            }
            let now = Instant::now();
            let Some(remaining) = deadline.checked_duration_since(now) else {
                return false;
            };
            let (next, result) = self
                .state
                .hits_condvar
                .wait_timeout(hits, remaining)
                .expect("execution unit commit fault hit wait should not be poisoned");
            hits = next;
            if result.timed_out() && hits.get(&label).copied().unwrap_or_default() < expected {
                return false;
            }
        }
    }

    pub fn wait_until_entered(&self, label: Label, timeout: std::time::Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut armed = self
            .state
            .armed
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        loop {
            if matches!(
                armed.get(&label),
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
                .wait_timeout(armed, remaining)
                .expect("execution unit commit fault wait should not be poisoned");
            armed = next;
            if result.timed_out()
                && !matches!(
                    armed.get(&label),
                    Some(ArmedFault::Pause { entered: true, .. })
                )
            {
                return false;
            }
        }
    }

    pub fn release(&self, label: Label) {
        let mut armed = self
            .state
            .armed
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
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
        {
            let mut hits = self
                .hits
                .lock()
                .expect("execution unit commit fault hit lock should not be poisoned");
            *hits.entry(label).or_default() += 1;
            self.hits_condvar.notify_all();
        }
        let mut armed = self
            .armed
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        let Some(fault) = armed.remove(&label) else {
            return Fault::Noop;
        };
        match fault {
            ArmedFault::Error(error) => Fault::Error(error),
            ArmedFault::RetryableConflicts {
                remaining,
                conflicting_sequence,
            } => {
                if remaining > 1 {
                    armed.insert(
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
                armed.insert(
                    label,
                    ArmedFault::Pause {
                        entered: true,
                        released,
                    },
                );
                Fault::Noop
            }
            ArmedFault::Pause { released, .. } => {
                armed.insert(
                    label,
                    ArmedFault::Pause {
                        entered: true,
                        released,
                    },
                );
                self.condvar.notify_all();
                loop {
                    match armed.get(&label) {
                        Some(ArmedFault::Pause {
                            released: false, ..
                        }) => {
                            armed = self
                                .condvar
                                .wait(armed)
                                .expect("execution unit commit fault wait should not be poisoned");
                        }
                        Some(ArmedFault::Pause { released: true, .. }) => {
                            armed.remove(&label);
                            return Fault::Noop;
                        }
                        Some(ArmedFault::Error(_) | ArmedFault::RetryableConflicts { .. }) => {
                            unreachable!("an entered pause cannot change fault kind")
                        }
                        None => return Fault::Noop,
                    }
                }
            }
        }
    }
}
