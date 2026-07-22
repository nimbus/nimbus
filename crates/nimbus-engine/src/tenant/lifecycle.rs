use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

use nimbus_core::{Error, Result, StorageErrorKind, TenantId};
use tokio::sync::Notify;

// Tenant lifecycle is a close-then-drain protocol:
// once deletion begins we first mark the tenant deleted so no new operations
// can enter, then we wait for the in-flight operation count to drain to zero.
// Sync callers block on the condvar path while async callers await Notify,
// but both are driven by the same atomic state and RAII operation guards.
pub(super) struct TenantLifecycle {
    deleted: AtomicBool,
    eviction_started: AtomicBool,
    eviction_complete: AtomicBool,
    active_operations: AtomicUsize,
    zero_active_lock: Mutex<()>,
    zero_active: Condvar,
    zero_active_notify: Notify,
    eviction_complete_lock: Mutex<()>,
    eviction_complete_condvar: Condvar,
    eviction_complete_notify: Notify,
}

impl TenantLifecycle {
    pub(super) fn new() -> Self {
        Self {
            deleted: AtomicBool::new(false),
            eviction_started: AtomicBool::new(false),
            eviction_complete: AtomicBool::new(false),
            active_operations: AtomicUsize::new(0),
            zero_active_lock: Mutex::new(()),
            zero_active: Condvar::new(),
            zero_active_notify: Notify::new(),
            eviction_complete_lock: Mutex::new(()),
            eviction_complete_condvar: Condvar::new(),
            eviction_complete_notify: Notify::new(),
        }
    }

    pub(super) fn enter_operation(&self, tenant_id: &TenantId) -> Result<()> {
        if let Some(error) = self.operation_rejection_if_deleted(tenant_id) {
            return Err(error);
        }
        self.active_operations.fetch_add(1, Ordering::AcqRel);
        if let Some(error) = self.operation_rejection_if_deleted(tenant_id) {
            self.release_operation();
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn operation_rejection_if_deleted(&self, tenant_id: &TenantId) -> Option<Error> {
        if !self.deleted.load(Ordering::Acquire) {
            return None;
        }
        if self.eviction_started.load(Ordering::Acquire) {
            // Durable-recovery eviction is transient: the tenant still exists,
            // and async load paths fence on this runtime before reopening it.
            // Sync/admission races must not turn that window into a false 404.
            return Some(Error::storage(
                StorageErrorKind::Unavailable,
                format!("tenant {tenant_id} runtime is restarting after durable recovery"),
            ));
        }
        Some(Error::TenantNotFound(tenant_id.clone()))
    }

    pub(super) fn release_operation(&self) {
        let _guard = self
            .zero_active_lock
            .lock()
            .expect("tenant lifecycle wait lock should not be poisoned");
        if self.active_operations.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.zero_active.notify_all();
            self.zero_active_notify.notify_waiters();
        }
    }

    pub(super) fn begin_delete_blocking(&self) {
        self.mark_deleted();
        self.wait_for_operations_blocking();
    }

    pub(super) fn mark_deleted(&self) {
        self.deleted.store(true, Ordering::Release);
    }

    pub(super) fn begin_eviction(&self) -> bool {
        if self.eviction_started.swap(true, Ordering::AcqRel) {
            return false;
        }
        self.mark_deleted();
        true
    }

    pub(super) fn eviction_started(&self) -> bool {
        self.eviction_started.load(Ordering::Acquire)
    }

    pub(super) fn finish_eviction(&self) {
        let _guard = self
            .eviction_complete_lock
            .lock()
            .expect("tenant eviction completion lock should not be poisoned");
        self.eviction_complete.store(true, Ordering::Release);
        self.eviction_complete_condvar.notify_all();
        self.eviction_complete_notify.notify_waiters();
    }

    pub(super) fn wait_for_eviction_complete_blocking(&self) {
        let mut guard = self
            .eviction_complete_lock
            .lock()
            .expect("tenant eviction completion lock should not be poisoned");
        while !self.eviction_complete.load(Ordering::Acquire) {
            guard = self
                .eviction_complete_condvar
                .wait(guard)
                .expect("tenant eviction completion wait should not be poisoned");
        }
    }

    pub(super) async fn wait_for_eviction_complete(&self) {
        loop {
            if self.eviction_complete.load(Ordering::Acquire) {
                return;
            }
            let notified = self.eviction_complete_notify.notified();
            if self.eviction_complete.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    pub(super) fn wait_for_operations_blocking(&self) {
        let mut guard = self
            .zero_active_lock
            .lock()
            .expect("tenant lifecycle wait lock should not be poisoned");
        while self.active_operations.load(Ordering::Acquire) != 0 {
            guard = self
                .zero_active
                .wait(guard)
                .expect("tenant lifecycle wait should not be poisoned");
        }
    }

    pub(super) async fn wait_for_operations_async(&self) {
        loop {
            if self.active_operations.load(Ordering::Acquire) == 0 {
                return;
            }
            let notified = self.zero_active_notify.notified();
            if self.active_operations.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};
    use std::thread;
    use std::time::Duration;

    use super::*;

    fn signal(pair: &(Mutex<bool>, Condvar)) {
        let (lock, cvar) = pair;
        let mut signaled = lock
            .lock()
            .expect("test signal lock should not be poisoned");
        *signaled = true;
        cvar.notify_all();
    }

    fn wait_for_signal(pair: &(Mutex<bool>, Condvar), timeout: Duration, context: &str) {
        let (lock, cvar) = pair;
        let signaled = lock
            .lock()
            .expect("test signal lock should not be poisoned");
        let (signaled, result) = cvar
            .wait_timeout_while(signaled, timeout, |signaled| !*signaled)
            .expect("test signal wait should not be poisoned");
        assert!(
            !result.timed_out() && *signaled,
            "{context} should be signaled before timeout"
        );
    }

    #[test]
    fn release_operation_notifies_blocking_delete_after_waiter_registers() {
        let lifecycle = Arc::new(TenantLifecycle::new());
        let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
        lifecycle
            .enter_operation(&tenant_id)
            .expect("operation should enter before deletion begins");
        lifecycle.deleted.store(true, Ordering::Release);

        let wait_guard = lifecycle
            .zero_active_lock
            .lock()
            .expect("tenant lifecycle wait lock should not be poisoned");
        assert_eq!(lifecycle.active_operations.load(Ordering::Acquire), 1);

        let release_started = Arc::new((Mutex::new(false), Condvar::new()));
        let release_allowed = Arc::new((Mutex::new(false), Condvar::new()));
        let release_finished = Arc::new((Mutex::new(false), Condvar::new()));

        let releaser = {
            let lifecycle = lifecycle.clone();
            let release_started = release_started.clone();
            let release_allowed = release_allowed.clone();
            let release_finished = release_finished.clone();
            thread::spawn(move || {
                signal(&release_started);
                wait_for_signal(
                    &release_allowed,
                    Duration::from_secs(1),
                    "release allowance",
                );
                lifecycle.release_operation();
                signal(&release_finished);
            })
        };

        wait_for_signal(
            &release_started,
            Duration::from_secs(1),
            "release thread start",
        );
        signal(&release_allowed);

        let finished = release_finished
            .0
            .lock()
            .expect("release-finished lock should not be poisoned");
        let (finished, result) = release_finished
            .1
            .wait_timeout_while(finished, Duration::from_millis(100), |finished| !*finished)
            .expect("release-finished wait should not be poisoned");
        assert!(
            result.timed_out() && !*finished,
            "release_operation must not notify while the blocking deleter still holds the wait lock"
        );
        drop(finished);

        let (wait_guard, result) = lifecycle
            .zero_active
            .wait_timeout(wait_guard, Duration::from_secs(1))
            .expect("blocking delete wait should not be poisoned");
        assert!(
            !result.timed_out(),
            "blocking delete waiter should be notified after it parks"
        );
        assert_eq!(lifecycle.active_operations.load(Ordering::Acquire), 0);
        drop(wait_guard);

        wait_for_signal(
            &release_finished,
            Duration::from_secs(1),
            "release completion",
        );
        releaser
            .join()
            .expect("release thread should not panic during lifecycle test");
    }

    #[test]
    fn operation_racing_eviction_is_retryable_not_tenant_not_found() {
        let lifecycle = TenantLifecycle::new();
        let tenant_id = TenantId::new("evicted").expect("tenant id should be valid");
        lifecycle.begin_eviction();

        let error = lifecycle
            .enter_operation(&tenant_id)
            .expect_err("operation must not enter an evicting runtime");
        assert_eq!(error.storage_kind(), Some(StorageErrorKind::Unavailable));
        assert_eq!(
            error.retryability(),
            nimbus_core::Retryability::RetryableAfterBackoff
        );
        assert!(
            !matches!(error, Error::TenantNotFound(_)),
            "durable-recovery eviction must not masquerade as tenant deletion"
        );
    }

    #[test]
    fn operation_after_explicit_delete_remains_tenant_not_found() {
        let lifecycle = TenantLifecycle::new();
        let tenant_id = TenantId::new("deleted").expect("tenant id should be valid");
        lifecycle.mark_deleted();

        assert!(matches!(
            lifecycle.enter_operation(&tenant_id),
            Err(Error::TenantNotFound(found)) if found == tenant_id
        ));
    }
}
