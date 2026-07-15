use nimbus_core::{Error, Result};

#[cfg(test)]
use std::collections::{HashMap, hash_map::Entry};
#[cfg(test)]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(test)]
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
        not(test),
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
    #[cfg(test)]
    state: Arc<CommitFaultState>,
}

impl CommitFaultClient {
    #[inline]
    pub(crate) fn wait(&self, label: Label) -> Fault {
        #[cfg(test)]
        {
            self.state.wait(label)
        }
        #[cfg(not(test))]
        {
            let _ = label;
            Fault::Noop
        }
    }

    #[cfg(test)]
    pub(crate) fn handle(&self) -> CommitFaultHandle {
        CommitFaultHandle {
            state: self.state.clone(),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct CommitFaultHandle {
    state: Arc<CommitFaultState>,
}

#[cfg(test)]
#[derive(Debug)]
enum ArmedFault {
    Pause { entered: bool, released: bool },
    Error(Error),
}

#[cfg(test)]
#[derive(Debug, Default)]
struct CommitFaultState {
    armed: Mutex<HashMap<Label, ArmedFault>>,
    condvar: Condvar,
}

#[cfg(test)]
impl CommitFaultHandle {
    pub(crate) fn arm(&self, label: Label) {
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

    pub(crate) fn inject(&self, label: Label, fault: Fault) {
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

    pub(crate) fn wait_until_entered(&self, label: Label, timeout: std::time::Duration) -> bool {
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

    pub(crate) fn release(&self, label: Label) {
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

#[cfg(test)]
impl CommitFaultState {
    fn wait(&self, label: Label) -> Fault {
        let mut armed = self
            .armed
            .lock()
            .expect("execution unit commit fault lock should not be poisoned");
        let Some(fault) = armed.remove(&label) else {
            return Fault::Noop;
        };
        match fault {
            ArmedFault::Error(error) => Fault::Error(error),
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
                        Some(ArmedFault::Error(_)) => {
                            unreachable!("an entered pause cannot change fault kind")
                        }
                        None => return Fault::Noop,
                    }
                }
            }
        }
    }
}
