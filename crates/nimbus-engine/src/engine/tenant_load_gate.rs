use std::sync::{Condvar, Mutex};

use tokio::sync::Notify;

pub(super) struct TenantLoadGate {
    held: Mutex<bool>,
    available: Condvar,
    notify: Notify,
}

impl TenantLoadGate {
    pub(super) fn new() -> Self {
        Self {
            held: Mutex::new(false),
            available: Condvar::new(),
            notify: Notify::new(),
        }
    }

    pub(super) fn blocking_lock(&self) -> TenantLoadGateGuard<'_> {
        let mut held = self
            .held
            .lock()
            .expect("tenant load gate lock should not be poisoned");
        while *held {
            held = self
                .available
                .wait(held)
                .expect("tenant load gate lock should not be poisoned");
        }
        *held = true;
        TenantLoadGateGuard { gate: self }
    }

    pub(super) async fn lock(&self) -> TenantLoadGateGuard<'_> {
        loop {
            let notified = self.notify.notified();
            {
                let mut held = self
                    .held
                    .lock()
                    .expect("tenant load gate lock should not be poisoned");
                if !*held {
                    *held = true;
                    return TenantLoadGateGuard { gate: self };
                }
            }
            notified.await;
        }
    }
}

pub(super) struct TenantLoadGateGuard<'a> {
    gate: &'a TenantLoadGate,
}

impl Drop for TenantLoadGateGuard<'_> {
    fn drop(&mut self) {
        let mut held = self
            .gate
            .held
            .lock()
            .expect("tenant load gate lock should not be poisoned");
        *held = false;
        self.gate.available.notify_one();
        self.gate.notify.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, mpsc as std_mpsc};
    use std::time::Duration;

    use tokio::sync::mpsc as tokio_mpsc;
    use tokio::time::timeout;

    use super::TenantLoadGate;

    #[test]
    fn blocking_lock_parks_until_guard_drops() {
        let gate = Arc::new(TenantLoadGate::new());
        let guard = gate.blocking_lock();
        let (started_tx, started_rx) = std_mpsc::channel();
        let (acquired_tx, acquired_rx) = std_mpsc::channel();
        let waiting_gate = gate.clone();

        let waiter = std::thread::spawn(move || {
            started_tx.send(()).expect("started signal should send");
            let _guard = waiting_gate.blocking_lock();
            acquired_tx.send(()).expect("acquired signal should send");
        });

        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("blocking waiter should start");
        assert!(
            acquired_rx.recv_timeout(Duration::from_millis(25)).is_err(),
            "blocking waiter must not acquire while the first guard is held"
        );

        drop(guard);
        acquired_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("blocking waiter should acquire after release");
        waiter.join().expect("blocking waiter should finish");
    }

    #[tokio::test]
    async fn async_lock_waits_for_blocking_guard() {
        let gate = Arc::new(TenantLoadGate::new());
        let guard = gate.blocking_lock();
        let (acquired_tx, mut acquired_rx) = tokio_mpsc::channel(1);
        let waiting_gate = gate.clone();

        let waiter = tokio::spawn(async move {
            let _guard = waiting_gate.lock().await;
            acquired_tx
                .send(())
                .await
                .expect("acquired signal should send");
        });

        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(
            acquired_rx.try_recv().is_err(),
            "async waiter must not acquire while the blocking guard is held"
        );

        drop(guard);
        timeout(Duration::from_secs(1), acquired_rx.recv())
            .await
            .expect("async waiter should acquire after release")
            .expect("acquired signal should arrive");
        waiter.await.expect("async waiter should finish");
    }
}
