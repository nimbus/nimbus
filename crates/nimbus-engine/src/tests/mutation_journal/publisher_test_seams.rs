use super::*;

/// A blocking test seam holds a production worker thread until its test
/// releases it. If the test body exits early — a failed assertion, an unmet
/// wait — the release never runs, so an unbounded wait here would deadlock the
/// tokio runtime's blocking pool at drop and hide the real failure behind an
/// infinite hang. Bound every release wait and fail loudly instead.
const BLOCKING_TEST_RELEASE_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) fn wait_for_test_release<'a, T>(
    condvar: &Condvar,
    guard: std::sync::MutexGuard<'a, T>,
    mut released: impl FnMut(&mut T) -> bool,
    what: &str,
) -> std::sync::MutexGuard<'a, T> {
    let (mut guard, _) = condvar
        .wait_timeout_while(guard, BLOCKING_TEST_RELEASE_TIMEOUT, |state| {
            !released(state)
        })
        .expect("blocking test release wait should succeed");
    assert!(
        released(&mut guard),
        "{what} was never released within {BLOCKING_TEST_RELEASE_TIMEOUT:?}; the test body \
         very likely failed or returned before calling its release method"
    );
    guard
}

fn wait_for_fault_release(released: &(Mutex<bool>, Condvar), what: &str) {
    let (lock, condvar) = released;
    let guard = lock.lock().expect("fault release lock should acquire");
    drop(wait_for_test_release(
        condvar,
        guard,
        |released| *released,
        what,
    ));
}
pub(super) struct RetryableThenBlockingAppendFaultInjector {
    append_visits: std::sync::atomic::AtomicU64,
    retry_entered: (Mutex<bool>, Condvar),
    retry_released: (Mutex<bool>, Condvar),
}

pub(super) struct BlockingDefinitiveAppendFaultInjector {
    append_visits: std::sync::atomic::AtomicU64,
    fail_on_visit: u64,
    failed: AtomicBool,
    entered: (Mutex<bool>, Condvar),
    released: (Mutex<bool>, Condvar),
}

#[derive(Default)]
pub(super) struct DurableAppendThenRecoveryFaultInjector {
    armed: AtomicBool,
    append_failed: AtomicBool,
    recovery_failed: AtomicBool,
}

impl DurableAppendThenRecoveryFaultInjector {
    pub(super) fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }
}

impl nimbus_storage::FaultInjector for DurableAppendThenRecoveryFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if !self.armed.load(Ordering::Acquire) {
            return Ok(());
        }
        if point == FaultPoint::JournalFlushBeforeVisibility
            && !self.append_failed.swap(true, Ordering::AcqRel)
        {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Unavailable,
                "injected append acknowledgement failure after durable visibility",
            ));
        }
        if point == FaultPoint::StorageCommitAfterVisibilityBeforeReturn
            && !self.recovery_failed.swap(true, Ordering::AcqRel)
        {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Unavailable,
                "injected durable journal recovery failure",
            ));
        }
        Ok(())
    }
}

impl BlockingDefinitiveAppendFaultInjector {
    pub(super) fn new() -> Arc<Self> {
        Self::new_on_visit(1)
    }

    pub(super) fn new_on_visit(fail_on_visit: u64) -> Arc<Self> {
        Arc::new(Self {
            append_visits: std::sync::atomic::AtomicU64::new(0),
            fail_on_visit,
            failed: AtomicBool::new(false),
            entered: (Mutex::new(false), Condvar::new()),
            released: (Mutex::new(false), Condvar::new()),
        })
    }

    pub(super) fn wait_until_blocked(&self, timeout: Duration) -> bool {
        let (lock, condvar) = &self.entered;
        let entered = lock.lock().expect("definitive entered lock should acquire");
        if *entered {
            return true;
        }
        let (entered, _) = condvar
            .wait_timeout_while(entered, timeout, |entered| !*entered)
            .expect("definitive entered wait should succeed");
        *entered
    }

    pub(super) fn release_failure(&self) {
        let (lock, condvar) = &self.released;
        *lock.lock().expect("definitive release lock should acquire") = true;
        condvar.notify_all();
    }
}

impl nimbus_storage::FaultInjector for BlockingDefinitiveAppendFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point != FaultPoint::JournalAppendBeforeDurableFlush {
            return Ok(());
        }
        let visit = self.append_visits.fetch_add(1, Ordering::AcqRel) + 1;
        if visit != self.fail_on_visit || self.failed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let (entered_lock, entered_condvar) = &self.entered;
        *entered_lock
            .lock()
            .expect("definitive entered lock should acquire") = true;
        entered_condvar.notify_all();
        wait_for_fault_release(&self.released, "definitive publisher failure");
        Err(Error::InvalidInput(
            "injected definitive publisher failure".to_string(),
        ))
    }
}

pub(super) struct BlockingAmbiguousApplyFaultInjector {
    failed: AtomicBool,
    entered: (Mutex<bool>, Condvar),
    released: (Mutex<bool>, Condvar),
}

impl BlockingAmbiguousApplyFaultInjector {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            failed: AtomicBool::new(false),
            entered: (Mutex::new(false), Condvar::new()),
            released: (Mutex::new(false), Condvar::new()),
        })
    }

    pub(super) fn wait_until_blocked(&self, timeout: Duration) -> bool {
        let (lock, condvar) = &self.entered;
        let entered = lock.lock().expect("ambiguous entered lock should acquire");
        if *entered {
            return true;
        }
        let (entered, _) = condvar
            .wait_timeout_while(entered, timeout, |entered| !*entered)
            .expect("ambiguous entered wait should succeed");
        *entered
    }

    pub(super) fn release_failure(&self) {
        let (lock, condvar) = &self.released;
        *lock.lock().expect("ambiguous release lock should acquire") = true;
        condvar.notify_all();
    }
}

impl nimbus_storage::FaultInjector for BlockingAmbiguousApplyFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point != FaultPoint::JournalDurableAppendBeforeApply
            || self.failed.swap(true, Ordering::AcqRel)
        {
            return Ok(());
        }
        let (entered_lock, entered_condvar) = &self.entered;
        *entered_lock
            .lock()
            .expect("ambiguous entered lock should acquire") = true;
        entered_condvar.notify_all();
        wait_for_fault_release(&self.released, "ambiguous publisher apply failure");
        Err(Error::Internal(
            "injected ambiguous publisher apply failure".to_string(),
        ))
    }
}

impl RetryableThenBlockingAppendFaultInjector {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            append_visits: std::sync::atomic::AtomicU64::new(0),
            retry_entered: (Mutex::new(false), Condvar::new()),
            retry_released: (Mutex::new(false), Condvar::new()),
        })
    }

    pub(super) fn wait_until_retry_blocked(&self, timeout: Duration) -> bool {
        let (lock, condvar) = &self.retry_entered;
        let entered = lock.lock().expect("retry-entered lock should acquire");
        if *entered {
            return true;
        }
        let (entered, _) = condvar
            .wait_timeout_while(entered, timeout, |entered| !*entered)
            .expect("retry-entered wait should succeed");
        *entered
    }

    pub(super) fn release_retry(&self) {
        let (lock, condvar) = &self.retry_released;
        *lock.lock().expect("retry-release lock should acquire") = true;
        condvar.notify_all();
    }
}

impl nimbus_storage::FaultInjector for RetryableThenBlockingAppendFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point != FaultPoint::JournalAppendBeforeDurableFlush {
            return Ok(());
        }
        let visit = self.append_visits.fetch_add(1, Ordering::AcqRel) + 1;
        if visit == 2 {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Transient,
                "injected transient publisher append failure",
            ));
        }
        if visit == 3 {
            let (entered_lock, entered_condvar) = &self.retry_entered;
            *entered_lock
                .lock()
                .expect("retry-entered lock should acquire") = true;
            entered_condvar.notify_all();
            wait_for_fault_release(&self.retry_released, "retryable append failure");
        }
        Ok(())
    }
}

pub(super) struct RetryExhaustionThenHealthyAppendFaultInjector {
    visits: std::sync::atomic::AtomicU64,
}

pub(super) struct ArmedOneShotDirectFaultInjector {
    point: FaultPoint,
    armed: AtomicBool,
    failed: AtomicBool,
    fail_on_visit: u64,
    visits_after_arm: std::sync::atomic::AtomicU64,
}

impl ArmedOneShotDirectFaultInjector {
    pub(super) fn new(point: FaultPoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            armed: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            fail_on_visit: 1,
            visits_after_arm: std::sync::atomic::AtomicU64::new(0),
        })
    }

    pub(super) fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }
}

impl nimbus_storage::FaultInjector for ArmedOneShotDirectFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point != self.point || !self.armed.load(Ordering::Acquire) {
            return Ok(());
        }
        let visit = self.visits_after_arm.fetch_add(1, Ordering::AcqRel) + 1;
        if visit == self.fail_on_visit && !self.failed.swap(true, Ordering::AcqRel) {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Transient,
                format!("injected one-shot direct fault at {}", point.as_str()),
            ));
        }
        Ok(())
    }
}

impl RetryExhaustionThenHealthyAppendFaultInjector {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            visits: std::sync::atomic::AtomicU64::new(0),
        })
    }
}

impl nimbus_storage::FaultInjector for RetryExhaustionThenHealthyAppendFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point == FaultPoint::JournalAppendBeforeDurableFlush
            && self.visits.fetch_add(1, Ordering::AcqRel) < 4
        {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Transient,
                "injected retry exhaustion before durable advance",
            ));
        }
        Ok(())
    }
}

/// Wedges a tenant's committed-mutation dispatcher on its first dispatch so a
/// concurrent durable-recovery eviction has to reach its bounded observer-drain
/// timeout instead of draining. Later dispatches — including those of the
/// runtime reopened after eviction — pass straight through.
pub(super) struct WedgedFirstDispatchObserver {
    pub(super) entered: std::sync::mpsc::SyncSender<()>,
    pub(super) release: Mutex<std::sync::mpsc::Receiver<()>>,
    pub(super) wedge_next: AtomicBool,
}

impl crate::CommittedMutationObserver for WedgedFirstDispatchObserver {
    fn committed_mutation_applied(&self, _event: crate::CommittedMutationEvent) {
        if !self.wedge_next.swap(false, Ordering::AcqRel) {
            return;
        }
        self.entered
            .send(())
            .expect("test should wait for the wedged observer");
        self.release
            .lock()
            .expect("wedged observer release receiver should lock")
            .recv_timeout(BLOCKING_TEST_RELEASE_TIMEOUT)
            .expect("test should release the wedged observer within the blocking-test timeout");
    }
}

pub(super) struct NestedWriteDuringEvictionObserver {
    pub(super) engine: std::sync::Weak<Engine>,
    pub(super) entered: std::sync::mpsc::SyncSender<()>,
    pub(super) release: Mutex<std::sync::mpsc::Receiver<()>>,
    pub(super) result: std::sync::mpsc::SyncSender<nimbus_core::Result<()>>,
}

impl crate::CommittedMutationObserver for NestedWriteDuringEvictionObserver {
    fn committed_mutation_applied(&self, event: crate::CommittedMutationEvent) {
        self.entered
            .send(())
            .expect("test should wait for the nested observer");
        self.release
            .lock()
            .expect("nested observer release receiver should lock")
            .recv_timeout(BLOCKING_TEST_RELEASE_TIMEOUT)
            .expect("test should release the nested observer within the blocking-test timeout");
        let result = self
            .engine
            .upgrade()
            .expect("engine should remain live during observer callback")
            .insert_document(
                &event.tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(99))]),
            )
            .map(|_| ());
        self.result
            .send(result)
            .expect("test should wait for the nested write result");
    }
}
