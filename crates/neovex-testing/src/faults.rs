use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use neovex_storage::{FaultInjector, FaultPoint};
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
    fn check(&self, point: FaultPoint) -> neovex_core::Result<()> {
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
    fn check(&self, point: FaultPoint) -> neovex_core::Result<()> {
        if !self.armed.load(Ordering::Acquire) {
            return Ok(());
        }
        self.inner.check(point)
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
}
