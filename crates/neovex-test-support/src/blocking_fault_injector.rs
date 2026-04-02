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
}
