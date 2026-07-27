//! Node-scoped egress engine: the composition root that owns the shared
//! [`ProxySubstrate`] and the per-workload PEP registry.
//!
//! One `EgressEngine` per node. The engine's map is a **lifecycle registry** —
//! it is touched at register/stop/reload time only and is never consulted
//! at accept or on the request path. Each accepted connection is handled by the
//! [`WorkloadPep`]'s own accept task, which closes over that PEP's captured
//! context; the request handler literally cannot name another workload's state,
//! and it cannot name this map (enforced by the EE1 reachability lint in
//! `tests.rs` and the plan verifier).
//!
//! The map is keyed by the opaque [`nimbus_core::WorkloadId`] — never a
//! sandbox-layer id type — so `nimbus-proxy` never depends on `nimbus-sandbox`.
//! Sandbox-layer publishing machinery (trust-anchor files, roots, port
//! allocation) stays in `nimbus-sandbox` and is injected: the engine carries an
//! opaque per-entry `attachment` and exposes a per-workload
//! [`RegistrationSlot`]. Preparation for one workload cannot block unrelated
//! lifecycle operations, while same-workload register/stop/readiness calls
//! wait for the slot to commit or withdraw before re-checking exact state.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use nimbus_core::WorkloadId;

use crate::error::{EgressProxyError, Result};
use crate::fairness::FairnessRegistry;
use crate::substrate::ProxySubstrate;
use crate::worker::WorkloadPep;

enum EngineEntry<A> {
    Preparing {
        preparation: Arc<RegistrationPreparation>,
        /// Failed provider attempts retained while a newer exact preparation
        /// owns the workload id. They remain invisible to readiness and move
        /// with that preparation into its eventual lifecycle disposition.
        quarantined: Vec<Arc<LifecycleCell<A>>>,
    },
    Lifecycle(LifecycleRegistryEntry<A>),
}

struct LifecycleRegistryEntry<A> {
    primary: Option<Arc<LifecycleCell<A>>>,
    quarantined: Vec<Arc<LifecycleCell<A>>>,
}

impl<A> LifecycleRegistryEntry<A> {
    fn primary(lifecycle: Arc<LifecycleCell<A>>) -> Self {
        Self {
            primary: Some(lifecycle),
            quarantined: Vec::new(),
        }
    }

    fn fail_closed_primary(&self) -> Option<Arc<LifecycleCell<A>>> {
        self.quarantined
            .is_empty()
            .then(|| self.primary.as_ref().map(Arc::clone))
            .flatten()
    }

    fn all(&self) -> Vec<Arc<LifecycleCell<A>>> {
        self.primary
            .iter()
            .chain(&self.quarantined)
            .map(Arc::clone)
            .collect()
    }
}

const LIFECYCLE_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

struct LifecycleWaitBudget {
    deadline: Instant,
    #[cfg(test)]
    forced_expired: Option<Arc<AtomicBool>>,
}

impl LifecycleWaitBudget {
    fn new() -> Self {
        Self {
            deadline: Instant::now() + LIFECYCLE_WAIT_TIMEOUT,
            #[cfg(test)]
            forced_expired: None,
        }
    }

    fn remaining(&self) -> Duration {
        #[cfg(test)]
        if self
            .forced_expired
            .as_ref()
            .is_some_and(|expired| expired.load(Ordering::SeqCst))
        {
            return Duration::ZERO;
        }
        self.deadline.saturating_duration_since(Instant::now())
    }

    #[cfg(test)]
    fn controlled() -> (Self, Arc<AtomicBool>) {
        let forced_expired = Arc::new(AtomicBool::new(false));
        (
            Self {
                deadline: Instant::now() + LIFECYCLE_WAIT_TIMEOUT,
                forced_expired: Some(Arc::clone(&forced_expired)),
            },
            forced_expired,
        )
    }
}

struct RegistrationPreparation {
    resolved: Mutex<bool>,
    resolved_signal: Condvar,
    #[cfg(test)]
    wait_started: std::sync::atomic::AtomicBool,
}

impl RegistrationPreparation {
    fn new() -> Self {
        Self {
            resolved: Mutex::new(false),
            resolved_signal: Condvar::new(),
            #[cfg(test)]
            wait_started: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn wait_until(&self, budget: &LifecycleWaitBudget) -> Result<()> {
        let mut resolved = self
            .resolved
            .lock()
            .map_err(|_| EgressProxyError::OperationFailed {
                message: "egress registration preparation lock is poisoned".to_owned(),
            })?;
        while !*resolved {
            #[cfg(test)]
            self.wait_started.store(true, Ordering::SeqCst);
            let remaining = budget.remaining();
            if remaining.is_zero() {
                return Err(EgressProxyError::OperationFailed {
                    message: "timed out waiting for same-workload egress registration preparation"
                        .to_owned(),
                });
            }
            let (next, _) = self
                .resolved_signal
                .wait_timeout(resolved, remaining)
                .map_err(|_| EgressProxyError::OperationFailed {
                    message: "egress registration preparation lock is poisoned".to_owned(),
                })?;
            resolved = next;
        }
        Ok(())
    }

    fn is_resolved(&self) -> Result<bool> {
        self.resolved.lock().map(|resolved| *resolved).map_err(|_| {
            EgressProxyError::OperationFailed {
                message: "egress registration preparation lock is poisoned".to_owned(),
            }
        })
    }

    fn resolve(&self) {
        let mut resolved = match self.resolved.lock() {
            Ok(resolved) => resolved,
            Err(poisoned) => poisoned.into_inner(),
        };
        *resolved = true;
        self.resolved_signal.notify_all();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum LifecyclePhase {
    Running = 0,
    Stopping = 1,
    Retiring = 2,
    Retired = 3,
}

impl LifecyclePhase {
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Running,
            1 => Self::Stopping,
            2 => Self::Retiring,
            3 => Self::Retired,
            _ => unreachable!("lifecycle phase is written only by LifecycleCell"),
        }
    }
}

enum LifecycleState<A> {
    Running {
        pep: WorkloadPep,
        attachment: A,
    },
    Stopping {
        pep: WorkloadPep,
        attachment: A,
        provider_stopped: bool,
        active_executor: Option<u64>,
        next_executor: u64,
    },
    Retiring {
        pep: WorkloadPep,
        attachment: A,
        provider_stopped: bool,
        executor: u64,
        next_executor: u64,
    },
    Retired,
}

struct LifecycleCell<A> {
    state: Mutex<LifecycleState<A>>,
    changed: Condvar,
    phase: AtomicU8,
    #[cfg(test)]
    cleanup_wait_started: AtomicBool,
    #[cfg(test)]
    cleanup_wait_count: std::sync::atomic::AtomicUsize,
}

impl<A> LifecycleCell<A> {
    fn running(pep: WorkloadPep, attachment: A) -> Self {
        Self {
            state: Mutex::new(LifecycleState::Running { pep, attachment }),
            changed: Condvar::new(),
            phase: AtomicU8::new(LifecyclePhase::Running as u8),
            #[cfg(test)]
            cleanup_wait_started: AtomicBool::new(false),
            #[cfg(test)]
            cleanup_wait_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    fn stopping(pep: WorkloadPep, attachment: A) -> (Self, u64) {
        let executor = 1;
        (
            Self {
                state: Mutex::new(LifecycleState::Stopping {
                    pep,
                    attachment,
                    provider_stopped: false,
                    active_executor: Some(executor),
                    next_executor: executor + 1,
                }),
                changed: Condvar::new(),
                phase: AtomicU8::new(LifecyclePhase::Stopping as u8),
                #[cfg(test)]
                cleanup_wait_started: AtomicBool::new(false),
                #[cfg(test)]
                cleanup_wait_count: std::sync::atomic::AtomicUsize::new(0),
            },
            executor,
        )
    }

    fn phase(&self) -> LifecyclePhase {
        LifecyclePhase::from_u8(self.phase.load(Ordering::Acquire))
    }

    fn set_phase(&self, phase: LifecyclePhase) {
        self.phase.store(phase as u8, Ordering::Release);
    }

    fn lock(&self) -> Result<MutexGuard<'_, LifecycleState<A>>> {
        self.state
            .lock()
            .map_err(|_| EgressProxyError::OperationFailed {
                message: "egress per-workload lifecycle lock is poisoned".to_owned(),
            })
    }

    fn wait_for_change_until<'a>(
        &self,
        state: MutexGuard<'a, LifecycleState<A>>,
        budget: &LifecycleWaitBudget,
        context: &str,
    ) -> Result<MutexGuard<'a, LifecycleState<A>>> {
        #[cfg(test)]
        self.cleanup_wait_started.store(true, Ordering::SeqCst);
        #[cfg(test)]
        self.cleanup_wait_count.fetch_add(1, Ordering::SeqCst);
        let remaining = budget.remaining();
        if remaining.is_zero() {
            return Err(EgressProxyError::OperationFailed {
                message: format!("timed out waiting for {context}"),
            });
        }
        let (state, _) = self.changed.wait_timeout(state, remaining).map_err(|_| {
            EgressProxyError::OperationFailed {
                message: "egress per-workload lifecycle lock is poisoned".to_owned(),
            }
        })?;
        Ok(state)
    }

    fn restore_retiring(&self, executor: u64) {
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        let LifecycleState::Retiring {
            pep,
            attachment,
            provider_stopped,
            executor: current_executor,
            next_executor,
        } = std::mem::replace(&mut *state, LifecycleState::Retired)
        else {
            return;
        };
        if current_executor != executor {
            *state = LifecycleState::Retiring {
                pep,
                attachment,
                provider_stopped,
                executor: current_executor,
                next_executor,
            };
            return;
        }
        *state = LifecycleState::Stopping {
            pep,
            attachment,
            provider_stopped,
            active_executor: Some(executor),
            next_executor,
        };
        self.set_phase(LifecyclePhase::Stopping);
        self.state.clear_poison();
        self.changed.notify_all();
    }
}

/// State observed while atomically deciding whether to reserve a registration
/// slot or inspect the exact occupied lifecycle cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisteredLifecyclePhase {
    /// The provider is published and available to lifecycle readers.
    Running,
    /// The provider is fenced while one cleanup executor owns teardown.
    Stopping,
}

/// Linearizable registration decision for one workload.
///
/// The node-global map lock is released before `evidence` is derived from an
/// occupied per-workload cell. This makes registration inspection composable
/// without serializing unrelated workloads.
pub enum RegistrationDecision<'a, A, R> {
    /// The caller exclusively owns preparation for a previously vacant id.
    Reserved(RegistrationSlot<'a, A>),
    /// The id remains occupied by exact process-local provider evidence.
    Occupied {
        /// Whether the provider is currently published or stopping.
        phase: RegisteredLifecyclePhase,
        /// Caller-derived evidence from the occupied attachment.
        evidence: R,
    },
}

/// Node-scoped owner of the shared proxy substrate and the per-workload PEP
/// lifecycle registry. `A` is an opaque per-entry attachment owned by the
/// caller (e.g. the sandbox layer's published trust-anchor record); the engine
/// never inspects it.
pub struct EgressEngine<A = ()> {
    substrate: ProxySubstrate,
    fairness: Arc<FairnessRegistry>,
    peps: Mutex<HashMap<WorkloadId, EngineEntry<A>>>,
}

impl<A> EgressEngine<A> {
    /// Create an engine on the shared node-wide substrate.
    pub fn new() -> Self {
        Self::with_substrate(ProxySubstrate::shared())
    }

    /// Create an engine on an explicit substrate (tests / dedicated runtimes).
    pub fn with_substrate(substrate: ProxySubstrate) -> Self {
        Self {
            substrate,
            fairness: Arc::new(FairnessRegistry::new()),
            peps: Mutex::new(HashMap::new()),
        }
    }

    /// Node-wide per-tenant fairness registry (the EE3 seam). Resolved at PEP
    /// registration time; request paths hold captured handles, never this map.
    pub fn fairness(&self) -> &Arc<FairnessRegistry> {
        &self.fairness
    }

    /// Replace the fairness registry — the TAA5 knob. The engine owns only
    /// the seam; budget VALUES ride the tenant-admission plan, which
    /// constructs a configured registry and hands it in here.
    pub fn with_fairness(mut self, fairness: Arc<FairnessRegistry>) -> Self {
        self.fairness = fairness;
        self
    }

    /// The substrate this engine runs its PEPs on.
    pub fn substrate(&self) -> &ProxySubstrate {
        &self.substrate
    }

    /// True if a PEP is registered and running for `id`.
    pub fn contains(&self, id: &WorkloadId) -> Result<bool> {
        let guard = self.lock_after_preparation(id)?;
        let lifecycle = match guard.get(id) {
            Some(EngineEntry::Lifecycle(entry)) => entry.fail_closed_primary(),
            Some(EngineEntry::Preparing { .. }) => {
                unreachable!("preparing entries are resolved before lifecycle inspection")
            }
            None => None,
        };
        drop(guard);
        Ok(lifecycle.is_some_and(|lifecycle| lifecycle.phase() == LifecyclePhase::Running))
    }

    /// Number of running PEPs (the node-wide feature seam: fan-out,
    /// metrics, and fairness iterate lifecycle state, never request state).
    pub fn len(&self) -> Result<usize> {
        Ok(self
            .lock()?
            .values()
            .filter(|entry| {
                matches!(
                    entry,
                    EngineEntry::Lifecycle(LifecycleRegistryEntry {
                        primary: Some(lifecycle),
                        quarantined,
                    })
                        if quarantined.is_empty()
                            && lifecycle.phase() == LifecyclePhase::Running
                )
            })
            .count())
    }

    /// True if no PEPs are running.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(!self.lock()?.values().any(|entry| {
            matches!(
                entry,
                EngineEntry::Lifecycle(LifecycleRegistryEntry {
                    primary: Some(lifecycle),
                    quarantined,
                })
                    if quarantined.is_empty()
                        && lifecycle.phase() == LifecyclePhase::Running
            )
        }))
    }

    /// Reserve the registration slot for `id`.
    ///
    /// Returns `Ok(None)` if a PEP is already registered (the caller treats
    /// that as already-running or stopping). A same-workload caller waits for
    /// an in-progress preparation to commit or withdraw before re-checking;
    /// unrelated workload lifecycle operations never wait on this slot.
    /// Dropping the slot without committing withdraws its exact preparation
    /// marker and wakes same-workload waiters.
    pub fn try_reserve(&self, id: WorkloadId) -> Result<Option<RegistrationSlot<'_, A>>> {
        let mut guard = self.lock_after_preparation(&id)?;
        if guard.contains_key(&id) {
            return Ok(None);
        }
        let preparation = Arc::new(RegistrationPreparation::new());
        guard.insert(
            id.clone(),
            EngineEntry::Preparing {
                preparation: Arc::clone(&preparation),
                quarantined: Vec::new(),
            },
        );
        drop(guard);
        Ok(Some(RegistrationSlot {
            engine: self,
            id,
            preparation,
            committed: false,
        }))
    }

    /// Atomically reserve a vacant registration id or inspect the exact
    /// occupied lifecycle attachment.
    ///
    /// This is the registration/start idempotency seam. Unlike a
    /// read-then-[`Self::try_reserve`] sequence, an occupied result cannot
    /// disappear between inspection and reservation. A retiring cell is
    /// allowed to finish and the decision is retried against the current map
    /// entry. The caller callback never runs under the node-global map lock.
    pub fn reserve_or_inspect<R>(
        &self,
        id: WorkloadId,
        inspect: impl FnOnce(&A) -> R,
    ) -> Result<RegistrationDecision<'_, A, R>> {
        let budget = LifecycleWaitBudget::new();
        self.reserve_or_inspect_until(id, inspect, &budget)
    }

    fn reserve_or_inspect_until<R>(
        &self,
        id: WorkloadId,
        inspect: impl FnOnce(&A) -> R,
        budget: &LifecycleWaitBudget,
    ) -> Result<RegistrationDecision<'_, A, R>> {
        let mut inspect = Some(inspect);
        loop {
            let mut guard = self.lock_after_preparation_until(&id, budget)?;
            let lifecycle = match guard.get(&id) {
                None => {
                    let preparation = Arc::new(RegistrationPreparation::new());
                    guard.insert(
                        id.clone(),
                        EngineEntry::Preparing {
                            preparation: Arc::clone(&preparation),
                            quarantined: Vec::new(),
                        },
                    );
                    drop(guard);
                    return Ok(RegistrationDecision::Reserved(RegistrationSlot {
                        engine: self,
                        id,
                        preparation,
                        committed: false,
                    }));
                }
                Some(EngineEntry::Preparing { .. }) => {
                    unreachable!("preparing entries are resolved before registration decision")
                }
                Some(EngineEntry::Lifecycle(entry)) if !entry.quarantined.is_empty() => {
                    return Err(EgressProxyError::OperationFailed {
                        message: format!(
                            "egress registration for workload {id} is fenced by conflicting \
                             quarantined provider evidence"
                        ),
                    });
                }
                Some(EngineEntry::Lifecycle(entry)) => {
                    entry.primary.as_ref().map(Arc::clone).expect(
                        "an unquarantined lifecycle registry entry retains its primary cell",
                    )
                }
            };
            drop(guard);

            let mut state = lifecycle.lock()?;
            loop {
                match &*state {
                    LifecycleState::Running { attachment, .. } => {
                        let evidence = inspect
                            .take()
                            .expect("registration inspection callback runs at most once")(
                            attachment,
                        );
                        return Ok(RegistrationDecision::Occupied {
                            phase: RegisteredLifecyclePhase::Running,
                            evidence,
                        });
                    }
                    LifecycleState::Stopping { attachment, .. } => {
                        let evidence = inspect
                            .take()
                            .expect("registration inspection callback runs at most once")(
                            attachment,
                        );
                        return Ok(RegistrationDecision::Occupied {
                            phase: RegisteredLifecyclePhase::Stopping,
                            evidence,
                        });
                    }
                    LifecycleState::Retiring { .. } => {
                        state = lifecycle.wait_for_change_until(
                            state,
                            budget,
                            "same-workload egress registration retirement",
                        )?;
                    }
                    LifecycleState::Retired => break,
                }
            }
            drop(state);
        }
    }

    /// Consume exact preparation authority and retain a failed provider as a
    /// retryable stopping tombstone.
    ///
    /// A stale attempt never replaces or resolves a foreign preparation. Its
    /// cleanup evidence is quarantined beside that marker and carried into the
    /// foreign attempt's eventual lifecycle or drop disposition.
    fn retain_failed_registration(
        &self,
        id: WorkloadId,
        expected_preparation: &Arc<RegistrationPreparation>,
        pep: WorkloadPep,
        attachment: A,
    ) -> RetainedFailedRegistration<A> {
        let mut repaired_poison = false;
        let mut guard = match self.peps.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                repaired_poison = true;
                poisoned.into_inner()
            }
        };
        let (lifecycle, executor) = LifecycleCell::stopping(pep, attachment);
        let lifecycle = Arc::new(lifecycle);
        let conflict = match guard.remove(&id) {
            Some(EngineEntry::Preparing {
                preparation,
                quarantined,
            }) if Arc::ptr_eq(&preparation, expected_preparation) => {
                let conflict =
                    (!quarantined.is_empty()).then(|| EgressProxyError::OperationFailed {
                        message: format!(
                            "egress registration failure for workload {id} encountered earlier \
                             quarantined provider evidence; every failed registration remains \
                             retained under an exact cleanup tombstone"
                        ),
                    });
                guard.insert(
                    id.clone(),
                    EngineEntry::Lifecycle(LifecycleRegistryEntry {
                        primary: Some(Arc::clone(&lifecycle)),
                        quarantined,
                    }),
                );
                conflict
            }
            Some(EngineEntry::Preparing {
                preparation,
                mut quarantined,
            }) => {
                quarantined.push(Arc::clone(&lifecycle));
                guard.insert(
                    id.clone(),
                    EngineEntry::Preparing {
                        preparation,
                        quarantined,
                    },
                );
                Some(EgressProxyError::OperationFailed {
                    message: format!(
                        "egress registration failure for workload {id} no longer owns the current \
                         preparation; its provider evidence is retained in an exact quarantine \
                         tombstone without disturbing the foreign preparation"
                    ),
                })
            }
            Some(EngineEntry::Lifecycle(mut entry)) => {
                entry.quarantined.push(Arc::clone(&lifecycle));
                guard.insert(id.clone(), EngineEntry::Lifecycle(entry));
                Some(EgressProxyError::OperationFailed {
                    message: format!(
                        "egress registration failure for workload {id} conflicts with existing \
                         process-local provider evidence; the failed registration is retained in \
                         an exact quarantine tombstone"
                    ),
                })
            }
            None => {
                guard.insert(
                    id.clone(),
                    EngineEntry::Lifecycle(LifecycleRegistryEntry::primary(Arc::clone(&lifecycle))),
                );
                None
            }
        };
        if repaired_poison {
            self.peps.clear_poison();
        }
        drop(guard);
        expected_preparation.resolve();
        RetainedFailedRegistration {
            stop: StopHandle::new(id, lifecycle, executor),
            conflict,
        }
    }

    /// Atomically replace an exact running entry with a retryable stop handle.
    ///
    /// A stop in progress remains in the registry as a tombstone. It denies
    /// readiness and replacement registration while retaining the mutable PEP
    /// handle and caller attachment through every fallible cleanup step.
    /// Replaying this method against the same attachment returns the same stop
    /// state.
    pub fn begin_stop_if_attachment(
        &self,
        id: &WorkloadId,
        matches: impl Fn(&A) -> bool,
    ) -> Result<Option<StopHandle<A>>> {
        let budget = LifecycleWaitBudget::new();
        self.begin_stop_if_attachment_until(id, matches, &budget)
    }

    fn begin_stop_if_attachment_until(
        &self,
        id: &WorkloadId,
        matches: impl Fn(&A) -> bool,
        budget: &LifecycleWaitBudget,
    ) -> Result<Option<StopHandle<A>>> {
        let guard = self.lock_after_preparation_until(id, budget)?;
        let lifecycles = match guard.get(id) {
            None => return Ok(None),
            Some(EngineEntry::Preparing { .. }) => {
                unreachable!("preparing entries are resolved before lifecycle inspection")
            }
            Some(EngineEntry::Lifecycle(entry)) => entry.all(),
        };
        drop(guard);

        let mut matching = Vec::new();
        for lifecycle in lifecycles {
            let state = lifecycle.lock()?;
            if matches!(
                &*state,
                LifecycleState::Running { attachment, .. }
                    | LifecycleState::Stopping { attachment, .. }
                    | LifecycleState::Retiring { attachment, .. }
                    if matches(attachment)
            ) {
                matching.push(Arc::clone(&lifecycle));
            }
        }
        let lifecycle = match matching.len() {
            0 => return Ok(None),
            1 => matching.pop().expect("one exact lifecycle was counted"),
            count => {
                return Err(EgressProxyError::OperationFailed {
                    message: format!(
                        "egress cleanup attachment for workload {id} matches {count} retained \
                         provider lifecycles; exact cleanup remains fenced"
                    ),
                });
            }
        };

        let mut state = lifecycle.lock()?;
        loop {
            match &mut *state {
                LifecycleState::Running { attachment, .. } if !matches(attachment) => {
                    return Ok(None);
                }
                LifecycleState::Running { .. } => {
                    let LifecycleState::Running { pep, attachment } =
                        std::mem::replace(&mut *state, LifecycleState::Retired)
                    else {
                        unreachable!("running state was matched under the same cell lock")
                    };
                    let executor = 1;
                    *state = LifecycleState::Stopping {
                        pep,
                        attachment,
                        provider_stopped: false,
                        active_executor: Some(executor),
                        next_executor: executor + 1,
                    };
                    lifecycle.set_phase(LifecyclePhase::Stopping);
                    drop(state);
                    return Ok(Some(StopHandle::new(id.clone(), lifecycle, executor)));
                }
                LifecycleState::Stopping { attachment, .. } if !matches(attachment) => {
                    return Ok(None);
                }
                LifecycleState::Stopping {
                    active_executor: Some(_),
                    ..
                }
                | LifecycleState::Retiring { .. } => {
                    state = lifecycle.wait_for_change_until(
                        state,
                        budget,
                        "same-workload egress cleanup executor",
                    )?;
                }
                LifecycleState::Stopping {
                    active_executor,
                    next_executor,
                    ..
                } => {
                    let executor = *next_executor;
                    *next_executor = next_executor.checked_add(1).ok_or_else(|| {
                        EgressProxyError::OperationFailed {
                            message: format!(
                                "egress cleanup executor generation exhausted for workload {id}"
                            ),
                        }
                    })?;
                    *active_executor = Some(executor);
                    drop(state);
                    return Ok(Some(StopHandle::new(id.clone(), lifecycle, executor)));
                }
                LifecycleState::Retired => return Ok(None),
            }
        }
    }

    /// Remove an exact stopping tombstone after acknowledged provider stop.
    ///
    /// The composition owner remains responsible for completing its own
    /// artifact and durable-authority steps before invoking this method.
    pub fn complete_stop(&self, stop: &StopHandle<A>) -> Result<()> {
        stop.require_active()?;
        {
            let mut state = stop.lifecycle.lock()?;
            let LifecycleState::Stopping {
                provider_stopped,
                active_executor,
                ..
            } = &*state
            else {
                return Err(stop.stale_error());
            };
            if *active_executor != Some(stop.executor) {
                return Err(stop.stale_error());
            }
            if !provider_stopped {
                return Err(EgressProxyError::OperationFailed {
                    message: format!(
                        "egress stop for workload {} cannot complete before provider acknowledgement",
                        stop.id
                    ),
                });
            }
            let LifecycleState::Stopping {
                pep,
                attachment,
                provider_stopped,
                active_executor: _,
                next_executor,
            } = std::mem::replace(&mut *state, LifecycleState::Retired)
            else {
                unreachable!("stopping state was authenticated under the same cell lock")
            };
            *state = LifecycleState::Retiring {
                pep,
                attachment,
                provider_stopped,
                executor: stop.executor,
                next_executor,
            };
            stop.lifecycle.set_phase(LifecyclePhase::Retiring);
        }

        let mut guard = match self.lock() {
            Ok(guard) => guard,
            Err(error) => {
                stop.lifecycle.restore_retiring(stop.executor);
                return Err(error);
            }
        };
        let mut remove_entry = false;
        let owns_lifecycle = match guard.get_mut(&stop.id) {
            Some(EngineEntry::Lifecycle(entry))
                if entry
                    .primary
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &stop.lifecycle)) =>
            {
                entry.primary = None;
                remove_entry = entry.quarantined.is_empty();
                true
            }
            Some(EngineEntry::Lifecycle(entry)) => {
                let before = entry.quarantined.len();
                entry
                    .quarantined
                    .retain(|current| !Arc::ptr_eq(current, &stop.lifecycle));
                let removed = entry.quarantined.len() != before;
                remove_entry = removed && entry.primary.is_none() && entry.quarantined.is_empty();
                removed
            }
            Some(EngineEntry::Preparing { quarantined, .. }) => {
                let before = quarantined.len();
                quarantined.retain(|current| !Arc::ptr_eq(current, &stop.lifecycle));
                quarantined.len() != before
            }
            None => false,
        };
        if !owns_lifecycle {
            drop(guard);
            stop.lifecycle.restore_retiring(stop.executor);
            return Err(EgressProxyError::OperationFailed {
                message: format!(
                    "egress stop for workload {} does not match the current registry lifecycle",
                    stop.id
                ),
            });
        }
        if remove_entry {
            guard.remove(&stop.id);
        }
        drop(guard);

        let mut state = match stop.lifecycle.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        let retiring_matches = matches!(
            &*state,
            LifecycleState::Retiring { executor, .. } if *executor == stop.executor
        );
        if !retiring_matches {
            return Err(stop.stale_error());
        }
        *state = LifecycleState::Retired;
        stop.lifecycle.set_phase(LifecyclePhase::Retired);
        stop.lifecycle.state.clear_poison();
        stop.lifecycle.changed.notify_all();
        stop.deactivate();
        Ok(())
    }

    /// Run `f` against the PEP registered for `id`.
    ///
    /// Lifecycle-only accessor (reload, readiness, addresses): no PEP handle
    /// escapes the registry, and the request path never calls this — accept
    /// tasks are spawned by the PEP itself and own their captured context.
    pub fn with_pep<R>(
        &self,
        id: &WorkloadId,
        f: impl FnOnce(&WorkloadPep) -> R,
    ) -> Result<Option<R>> {
        let Some(lifecycle) = self.lifecycle(id)? else {
            return Ok(None);
        };
        let state = lifecycle.lock()?;
        match &*state {
            LifecycleState::Running { pep, .. } => Ok(Some(f(pep))),
            LifecycleState::Stopping { .. }
            | LifecycleState::Retiring { .. }
            | LifecycleState::Retired => Ok(None),
        }
    }

    /// Run `f` against the caller-owned lifecycle attachment for `id`.
    ///
    /// This is deliberately separate from [`Self::with_pep`]: composition
    /// owners can validate durable registration identity without exposing or
    /// moving the attachment and without making it reachable from request
    /// handling.
    pub fn with_attachment<R>(
        &self,
        id: &WorkloadId,
        f: impl FnOnce(&A) -> R,
    ) -> Result<Option<R>> {
        let Some(lifecycle) = self.lifecycle(id)? else {
            return Ok(None);
        };
        let state = lifecycle.lock()?;
        match &*state {
            LifecycleState::Running { attachment, .. } => Ok(Some(f(attachment))),
            LifecycleState::Stopping { .. }
            | LifecycleState::Retiring { .. }
            | LifecycleState::Retired => Ok(None),
        }
    }

    /// Read lifecycle attachment state from either Running or Stopping.
    ///
    /// This is a composition-control accessor only. Request handling and
    /// readiness must use [`Self::with_pep`] / [`Self::with_attachment`], which
    /// deliberately hide stopping entries.
    pub fn with_lifecycle_attachment<R>(
        &self,
        id: &WorkloadId,
        f: impl FnOnce(&A) -> R,
    ) -> Result<Option<R>> {
        let Some(lifecycle) = self.lifecycle(id)? else {
            return Ok(None);
        };
        let state = lifecycle.lock()?;
        match &*state {
            LifecycleState::Running { attachment, .. }
            | LifecycleState::Stopping { attachment, .. } => Ok(Some(f(attachment))),
            LifecycleState::Retiring { .. } | LifecycleState::Retired => Ok(None),
        }
    }

    /// Read the one exact lifecycle attachment matching caller-owned evidence.
    ///
    /// This includes quarantined failed registrations, which are deliberately
    /// hidden from readiness and ordinary registration views. More than one
    /// match is an authority ambiguity and fails closed.
    pub fn with_lifecycle_attachment_if<R>(
        &self,
        id: &WorkloadId,
        matches: impl Fn(&A) -> bool,
        inspect: impl FnOnce(&A) -> R,
    ) -> Result<Option<R>> {
        let guard = self.lock_after_preparation(id)?;
        let lifecycles = match guard.get(id) {
            Some(EngineEntry::Lifecycle(entry)) => entry.all(),
            Some(EngineEntry::Preparing { .. }) => {
                unreachable!("preparing entries are resolved before lifecycle inspection")
            }
            None => return Ok(None),
        };
        drop(guard);

        let mut evidence = None;
        let mut inspect = Some(inspect);
        for lifecycle in lifecycles {
            let state = lifecycle.lock()?;
            let attachment = match &*state {
                LifecycleState::Running { attachment, .. }
                | LifecycleState::Stopping { attachment, .. } => attachment,
                LifecycleState::Retiring { .. } | LifecycleState::Retired => continue,
            };
            if !matches(attachment) {
                continue;
            }
            if evidence.is_some() {
                return Err(EgressProxyError::OperationFailed {
                    message: format!(
                        "egress lifecycle evidence for workload {id} matches multiple retained \
                         provider lifecycles"
                    ),
                });
            }
            evidence = Some(inspect
                .take()
                .expect("exact lifecycle inspection runs at most once")(
                attachment
            ));
        }
        Ok(evidence)
    }

    fn lifecycle(&self, id: &WorkloadId) -> Result<Option<Arc<LifecycleCell<A>>>> {
        let guard = self.lock_after_preparation(id)?;
        let lifecycle = match guard.get(id) {
            Some(EngineEntry::Lifecycle(entry)) => entry.fail_closed_primary(),
            Some(EngineEntry::Preparing { .. }) => {
                unreachable!("preparing entries are resolved before lifecycle inspection")
            }
            None => None,
        };
        drop(guard);
        Ok(lifecycle)
    }

    fn lock(&self) -> Result<MutexGuard<'_, HashMap<WorkloadId, EngineEntry<A>>>> {
        self.peps
            .lock()
            .map_err(|_| EgressProxyError::OperationFailed {
                message: "egress engine registry lock is poisoned".to_owned(),
            })
    }

    fn lock_after_preparation(
        &self,
        id: &WorkloadId,
    ) -> Result<MutexGuard<'_, HashMap<WorkloadId, EngineEntry<A>>>> {
        let budget = LifecycleWaitBudget::new();
        self.lock_after_preparation_until(id, &budget)
    }

    fn lock_after_preparation_until(
        &self,
        id: &WorkloadId,
        budget: &LifecycleWaitBudget,
    ) -> Result<MutexGuard<'_, HashMap<WorkloadId, EngineEntry<A>>>> {
        loop {
            let guard = self.lock()?;
            let preparation = match guard.get(id) {
                Some(EngineEntry::Preparing { preparation, .. }) => Arc::clone(preparation),
                _ => return Ok(guard),
            };
            if preparation.is_resolved()? {
                return Err(EgressProxyError::OperationFailed {
                    message: format!(
                        "resolved egress registration preparation marker remains installed for \
                         workload {id}; registry recovery is incomplete"
                    ),
                });
            }
            drop(guard);
            preparation.wait_until(budget)?;
        }
    }
}

impl<A> Default for EgressEngine<A> {
    fn default() -> Self {
        Self::new()
    }
}

/// Retryable process-local evidence for one PEP stop attempt.
///
/// Exactly one handle owns cleanup execution for a workload at a time.
/// Dropping an incomplete handle releases that executor token and wakes one
/// same-workload retry; it never removes the occupied lifecycle cell or its
/// provider evidence.
pub struct StopHandle<A> {
    id: WorkloadId,
    lifecycle: Arc<LifecycleCell<A>>,
    executor: u64,
    active: AtomicBool,
}

impl<A> StopHandle<A> {
    fn new(id: WorkloadId, lifecycle: Arc<LifecycleCell<A>>, executor: u64) -> Self {
        Self {
            id,
            lifecycle,
            executor,
            active: AtomicBool::new(true),
        }
    }

    /// Workload whose exact provider effect is stopping.
    pub fn id(&self) -> &WorkloadId {
        &self.id
    }

    /// Retry acknowledged provider shutdown without discarding the handle.
    pub fn shutdown_provider(&self) -> Result<()> {
        let mut state = self.lock()?;
        let LifecycleState::Stopping {
            pep,
            provider_stopped,
            active_executor,
            ..
        } = &mut *state
        else {
            return Err(self.stale_error());
        };
        if *active_executor != Some(self.executor) {
            return Err(self.stale_error());
        }
        if *provider_stopped {
            return Ok(());
        }
        pep.shutdown()?;
        *provider_stopped = true;
        Ok(())
    }

    /// Whether explicit shutdown produced provider-absence evidence.
    pub fn provider_stopped(&self) -> Result<bool> {
        let state = self.lock()?;
        let LifecycleState::Stopping {
            provider_stopped,
            active_executor,
            ..
        } = &*state
        else {
            return Err(self.stale_error());
        };
        if *active_executor != Some(self.executor) {
            return Err(self.stale_error());
        }
        Ok(*provider_stopped)
    }

    /// Read caller-owned cleanup state while retaining the tombstone.
    pub fn with_attachment<R>(&self, f: impl FnOnce(&A) -> R) -> Result<R> {
        let state = self.lock()?;
        let LifecycleState::Stopping {
            attachment,
            active_executor,
            ..
        } = &*state
        else {
            return Err(self.stale_error());
        };
        if *active_executor != Some(self.executor) {
            return Err(self.stale_error());
        }
        Ok(f(attachment))
    }

    /// Mutate caller-owned cleanup progress while retaining the tombstone.
    pub fn with_attachment_mut<R>(&self, f: impl FnOnce(&mut A) -> R) -> Result<R> {
        let mut state = self.lock()?;
        let LifecycleState::Stopping {
            attachment,
            active_executor,
            ..
        } = &mut *state
        else {
            return Err(self.stale_error());
        };
        if *active_executor != Some(self.executor) {
            return Err(self.stale_error());
        }
        Ok(f(attachment))
    }

    fn lock(&self) -> Result<MutexGuard<'_, LifecycleState<A>>> {
        self.require_active()?;
        self.lifecycle.lock()
    }

    fn require_active(&self) -> Result<()> {
        if self.active.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(self.stale_error())
        }
    }

    fn deactivate(&self) {
        self.active.store(false, Ordering::Release);
    }

    fn stale_error(&self) -> EgressProxyError {
        EgressProxyError::OperationFailed {
            message: format!(
                "egress cleanup executor {} for workload {} no longer owns lifecycle authority",
                self.executor, self.id
            ),
        }
    }
}

impl<A> Drop for StopHandle<A> {
    fn drop(&mut self) {
        if !self.active.swap(false, Ordering::AcqRel) {
            return;
        }
        let mut state = match self.lifecycle.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let LifecycleState::Stopping {
            active_executor, ..
        } = &mut *state
            && *active_executor == Some(self.executor)
        {
            *active_executor = None;
            self.lifecycle.state.clear_poison();
            self.lifecycle.changed.notify_all();
        }
    }
}

/// A reserved registration slot for one workload id.
///
/// The engine publishes an exact per-workload preparation marker under the
/// short node-global map lock, then releases that lock before returning this
/// slot. Same-workload lifecycle calls wait on the marker; unrelated workload
/// calls proceed. Commit atomically swaps the exact marker for the running PEP.
#[must_use = "an uncommitted slot releases the registration on drop"]
pub struct RegistrationSlot<'a, A> {
    engine: &'a EgressEngine<A>,
    id: WorkloadId,
    preparation: Arc<RegistrationPreparation>,
    committed: bool,
}

/// Failed registration commit with caller-owned cleanup evidence intact.
///
/// A caller may already have activated durable listener authority before the
/// final process-local registry transition. Returning both the provider and
/// attachment prevents an error here from silently discarding the only
/// cleanup handle for that activated effect. Dropping the failure retains the
/// provider as a stopping tombstone, so explicit error handling is not
/// required to preserve cleanup authority.
pub struct RegistrationCommitFailure<'a, A> {
    parts: Option<RegistrationCommitFailureParts<'a, A>>,
}

struct RegistrationCommitFailureParts<'a, A> {
    error: Box<EgressProxyError>,
    pep: Box<WorkloadPep>,
    attachment: Box<A>,
    slot: RegistrationSlot<'a, A>,
}

/// Engine-owned cleanup evidence retained after a failed registration commit.
///
/// `conflict` is present when a pre-existing primary lifecycle forced this
/// provider into a separate quarantine tombstone. The stop handle always
/// addresses the exact retained cell, so cleanup never overwrites or borrows
/// the conflicting provider's authority.
pub struct RetainedFailedRegistration<A> {
    stop: StopHandle<A>,
    conflict: Option<EgressProxyError>,
}

impl<A> RetainedFailedRegistration<A> {
    /// Recover the exact cleanup executor plus an optional collision
    /// diagnostic. The provider and attachment remain engine-owned.
    pub fn into_parts(self) -> (StopHandle<A>, Option<EgressProxyError>) {
        (self.stop, self.conflict)
    }
}

impl<A> RegistrationCommitFailure<'_, A> {
    /// Address of the still caller-owned provider effect.
    pub fn provider_local_addr(&self) -> std::net::SocketAddr {
        self.parts
            .as_ref()
            .expect("commit failure retains its provider until resolution")
            .pep
            .local_addr()
    }

    /// Retain the exact failed provider under the preparation capability that
    /// produced this commit failure.
    pub fn retain(mut self) -> (EgressProxyError, RetainedFailedRegistration<A>) {
        let RegistrationCommitFailureParts {
            error,
            pep,
            attachment,
            slot,
        } = self
            .parts
            .take()
            .expect("commit failure is consumed exactly once");
        slot.retain_failed(*error, *pep, *attachment)
    }
}

impl<A> std::fmt::Debug for RegistrationCommitFailure<'_, A> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RegistrationCommitFailure")
            .field(
                "error",
                &self.parts.as_ref().map(|parts| parts.error.as_ref()),
            )
            .field("provider_and_attachment_retained", &true)
            .finish()
    }
}

impl<A> Drop for RegistrationCommitFailure<'_, A> {
    fn drop(&mut self) {
        let Some(RegistrationCommitFailureParts {
            error,
            pep,
            attachment,
            slot,
        }) = self.parts.take()
        else {
            return;
        };
        let (_, retained) = slot.retain_failed(*error, *pep, *attachment);
        drop(retained);
    }
}

impl<'a, A> RegistrationSlot<'a, A> {
    /// The workload id this slot reserves.
    pub fn id(&self) -> &WorkloadId {
        &self.id
    }

    /// Commit the reservation by atomically replacing this slot's exact
    /// preparation marker with `pep` and its caller-owned `attachment`.
    pub fn commit(
        mut self,
        pep: WorkloadPep,
        attachment: A,
    ) -> std::result::Result<(), RegistrationCommitFailure<'a, A>> {
        let mut guard = match self.engine.lock() {
            Ok(guard) => guard,
            Err(error) => {
                return Err(RegistrationCommitFailure {
                    parts: Some(RegistrationCommitFailureParts {
                        error: Box::new(error),
                        pep: Box::new(pep),
                        attachment: Box::new(attachment),
                        slot: self,
                    }),
                });
            }
        };
        let commit_error = match guard.get(&self.id) {
            Some(EngineEntry::Preparing {
                preparation,
                quarantined,
            }) if Arc::ptr_eq(preparation, &self.preparation) && quarantined.is_empty() => None,
            Some(EngineEntry::Preparing {
                preparation,
                quarantined,
            }) if Arc::ptr_eq(preparation, &self.preparation) => {
                Some(EgressProxyError::OperationFailed {
                    message: format!(
                        "egress registration slot for workload {} cannot publish a running \
                         primary while {} quarantined provider cleanup attempt(s) remain",
                        self.id,
                        quarantined.len()
                    ),
                })
            }
            _ => Some(EgressProxyError::OperationFailed {
                message: format!(
                    "egress registration slot for workload {} no longer owns its preparation marker",
                    self.id
                ),
            }),
        };
        if let Some(error) = commit_error {
            return Err(RegistrationCommitFailure {
                parts: Some(RegistrationCommitFailureParts {
                    error: Box::new(error),
                    pep: Box::new(pep),
                    attachment: Box::new(attachment),
                    slot: self,
                }),
            });
        }
        let quarantined = match guard.remove(&self.id) {
            Some(EngineEntry::Preparing {
                preparation,
                quarantined,
            }) if Arc::ptr_eq(&preparation, &self.preparation) => quarantined,
            _ => unreachable!("preparation ownership was authenticated under the same map lock"),
        };
        guard.insert(
            self.id.clone(),
            EngineEntry::Lifecycle(LifecycleRegistryEntry {
                primary: Some(Arc::new(LifecycleCell::running(pep, attachment))),
                quarantined,
            }),
        );
        self.committed = true;
        drop(guard);
        self.preparation.resolve();
        Ok(())
    }

    /// Convert this exact preparation into a retained stopping tombstone.
    ///
    /// This is the pre-commit post-activation failure seam. It consumes the
    /// slot capability, so callers cannot reconstruct recovery authority from
    /// a workload id alone.
    pub fn retain_failed(
        mut self,
        error: EgressProxyError,
        pep: WorkloadPep,
        attachment: A,
    ) -> (EgressProxyError, RetainedFailedRegistration<A>) {
        let retained = self.engine.retain_failed_registration(
            self.id.clone(),
            &self.preparation,
            pep,
            attachment,
        );
        self.committed = true;
        (error, retained)
    }
}

impl<A> Drop for RegistrationSlot<'_, A> {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut repaired_poison = false;
        let mut guard = match self.engine.peps.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                repaired_poison = true;
                poisoned.into_inner()
            }
        };
        let quarantined = match guard.get_mut(&self.id) {
            Some(EngineEntry::Preparing {
                preparation,
                quarantined,
            }) if Arc::ptr_eq(preparation, &self.preparation) => Some(std::mem::take(quarantined)),
            _ => None,
        };
        let restored_invariant = quarantined.is_some();
        if let Some(quarantined) = quarantined {
            if quarantined.is_empty() {
                guard.remove(&self.id);
            } else {
                guard.insert(
                    self.id.clone(),
                    EngineEntry::Lifecycle(LifecycleRegistryEntry {
                        primary: None,
                        quarantined,
                    }),
                );
            }
        }
        drop(guard);
        if repaired_poison && restored_invariant {
            self.engine.peps.clear_poison();
        }
        self.preparation.resolve();
    }
}

#[cfg(test)]
#[path = "engine/tests.rs"]
mod tests;
