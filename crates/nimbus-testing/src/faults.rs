use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use nimbus_core::Error;
use nimbus_storage::{FaultInjector, FaultPoint};
use tokio::sync::Notify;

pub struct BlockingFaultInjector {
    point: FaultPoint,
    entered: Notify,
    release_gate: (Mutex<bool>, Condvar),
}

impl BlockingFaultInjector {
    pub fn new(point: FaultPoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            entered: Notify::new(),
            release_gate: (Mutex::new(false), Condvar::new()),
        })
    }

    pub async fn wait_until_entered(&self) {
        self.entered.notified().await;
    }

    pub fn release(&self) {
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        *released = true;
        cvar.notify_all();
    }
}

impl FaultInjector for BlockingFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point != self.point {
            return Ok(());
        }
        self.entered.notify_one();
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        while !*released {
            released = cvar
                .wait(released)
                .expect("blocking fault injector should wait for release");
        }
        Ok(())
    }
}

pub struct ArmedBlockingFaultInjector {
    armed: AtomicBool,
    inner: Arc<BlockingFaultInjector>,
}

impl ArmedBlockingFaultInjector {
    pub fn new(point: FaultPoint) -> Arc<Self> {
        Arc::new(Self {
            armed: AtomicBool::new(false),
            inner: BlockingFaultInjector::new(point),
        })
    }

    pub fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    pub async fn wait_until_entered(&self) {
        self.inner.wait_until_entered().await;
    }

    pub fn release(&self) {
        self.armed.store(false, Ordering::Release);
        self.inner.release();
    }
}

impl FaultInjector for ArmedBlockingFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if !self.armed.load(Ordering::Acquire) {
            return Ok(());
        }
        self.inner.check(point)
    }
}

/// Which visits to a `FaultPoint` should fail, counted per matching call.
#[derive(Debug, Clone, Copy)]
enum CountedFaultMode {
    /// Fail exactly the `n`th matching call (1-indexed); every other call
    /// succeeds.
    Nth(u64),
    /// Fail each of the first `n` matching calls, then succeed on every
    /// call after that.
    FirstN(u64),
}

/// A `FaultInjector` that fails a matching `FaultPoint` a deterministic
/// number of times, tracked by an atomic visit counter. Unlike
/// `ScriptedFaultInjector` (nimbus-storage), which schedules faults across
/// potentially many distinct `FaultPoint`s up front, this type is scoped to
/// a single point and expresses the two counted shapes tests need most:
/// "fail the Nth call" and "fail N times then succeed".
pub struct CountedFaultInjector {
    point: FaultPoint,
    mode: CountedFaultMode,
    visits: AtomicU64,
    failures: AtomicU64,
}

impl CountedFaultInjector {
    /// Fails only the `n`th call (1-indexed) that checks `point`; every
    /// other call, including calls after the `n`th, succeeds.
    pub fn fail_nth_call(point: FaultPoint, n: u64) -> Arc<Self> {
        Arc::new(Self {
            point,
            mode: CountedFaultMode::Nth(n),
            visits: AtomicU64::new(0),
            failures: AtomicU64::new(0),
        })
    }

    /// Fails the first `n` calls that check `point`, then succeeds on every
    /// call after that.
    pub fn fail_first_n_calls(point: FaultPoint, n: u64) -> Arc<Self> {
        Arc::new(Self {
            point,
            mode: CountedFaultMode::FirstN(n),
            visits: AtomicU64::new(0),
            failures: AtomicU64::new(0),
        })
    }

    /// Total number of times `check` was called for the configured point,
    /// including calls that did not fail.
    pub fn visit_count(&self) -> u64 {
        self.visits.load(Ordering::Acquire)
    }

    /// Total number of times `check` actually returned an injected failure.
    pub fn failure_count(&self) -> u64 {
        self.failures.load(Ordering::Acquire)
    }
}

impl FaultInjector for CountedFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point != self.point {
            return Ok(());
        }
        let visit = self.visits.fetch_add(1, Ordering::AcqRel) + 1;
        let should_fail = match self.mode {
            CountedFaultMode::Nth(n) => visit == n,
            CountedFaultMode::FirstN(n) => visit <= n,
        };
        if !should_fail {
            return Ok(());
        }
        self.failures.fetch_add(1, Ordering::AcqRel);
        Err(Error::Internal(format!(
            "injected counted fault at {} on visit {}",
            point.as_str(),
            visit
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn blocking_fault_injector_waits_until_release() {
        let injector = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
        let worker = tokio::task::spawn_blocking({
            let injector = injector.clone();
            move || injector.check(FaultPoint::JournalDurableAppendBeforeApply)
        });

        tokio::time::timeout(Duration::from_secs(1), injector.wait_until_entered())
            .await
            .expect("fault injector should observe the matching fault");
        assert!(
            !worker.is_finished(),
            "fault check should remain blocked until the gate is released"
        );

        injector.release();
        worker
            .await
            .expect("fault injector worker should join")
            .expect("fault injector should complete successfully");
    }

    #[tokio::test]
    async fn armed_blocking_fault_injector_ignores_faults_until_armed() {
        let injector = ArmedBlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);

        injector
            .check(FaultPoint::JournalDurableAppendBeforeApply)
            .expect("unarmed injector should ignore matching faults");

        let worker = tokio::task::spawn_blocking({
            let injector = injector.clone();
            move || injector.check(FaultPoint::JournalDurableAppendBeforeApply)
        });

        worker
            .await
            .expect("unarmed injector worker should join")
            .expect("unarmed injector should ignore the fault successfully");

        injector.arm();
        let armed_worker = tokio::task::spawn_blocking({
            let injector = injector.clone();
            move || injector.check(FaultPoint::JournalDurableAppendBeforeApply)
        });

        tokio::time::timeout(Duration::from_secs(1), injector.wait_until_entered())
            .await
            .expect("armed injector should observe the matching fault");
        assert!(
            !armed_worker.is_finished(),
            "armed injector should block until the gate is released"
        );

        injector.release();
        armed_worker
            .await
            .expect("armed injector worker should join")
            .expect("armed injector should complete successfully");
    }

    #[test]
    fn counted_fault_injector_ignores_unmatched_points() {
        let injector =
            CountedFaultInjector::fail_nth_call(FaultPoint::JournalDurableAppendBeforeApply, 1);

        injector
            .check(FaultPoint::JournalFlushBeforeVisibility)
            .expect("unmatched fault point should never fail");
        assert_eq!(injector.visit_count(), 0);
        assert_eq!(injector.failure_count(), 0);
    }

    #[test]
    fn counted_fault_injector_fails_only_the_nth_call() {
        let injector =
            CountedFaultInjector::fail_nth_call(FaultPoint::JournalDurableAppendBeforeApply, 3);

        for expected_visit in 1..=2 {
            injector
                .check(FaultPoint::JournalDurableAppendBeforeApply)
                .unwrap_or_else(|_| panic!("call {expected_visit} should succeed"));
        }
        injector
            .check(FaultPoint::JournalDurableAppendBeforeApply)
            .expect_err("the 3rd call should fail");
        for expected_visit in 4..=5 {
            injector
                .check(FaultPoint::JournalDurableAppendBeforeApply)
                .unwrap_or_else(|_| panic!("call {expected_visit} should succeed"));
        }

        assert_eq!(injector.visit_count(), 5);
        assert_eq!(injector.failure_count(), 1);
    }

    #[test]
    fn counted_fault_injector_fails_first_n_calls_then_succeeds() {
        let injector = CountedFaultInjector::fail_first_n_calls(
            FaultPoint::JournalDurableAppendBeforeApply,
            2,
        );

        injector
            .check(FaultPoint::JournalDurableAppendBeforeApply)
            .expect_err("call 1 should fail");
        injector
            .check(FaultPoint::JournalDurableAppendBeforeApply)
            .expect_err("call 2 should fail");
        injector
            .check(FaultPoint::JournalDurableAppendBeforeApply)
            .expect("call 3 should succeed");
        injector
            .check(FaultPoint::JournalDurableAppendBeforeApply)
            .expect("call 4 should succeed");

        assert_eq!(injector.visit_count(), 4);
        assert_eq!(injector.failure_count(), 2);
    }

    #[test]
    fn counted_fault_injector_fail_first_zero_calls_never_fails() {
        let injector = CountedFaultInjector::fail_first_n_calls(
            FaultPoint::JournalDurableAppendBeforeApply,
            0,
        );

        injector
            .check(FaultPoint::JournalDurableAppendBeforeApply)
            .expect("with n=0 the first call should succeed");
        assert_eq!(injector.failure_count(), 0);
    }
}
