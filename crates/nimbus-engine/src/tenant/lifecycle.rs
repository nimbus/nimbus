use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};

use nimbus_core::{Error, Result, TenantId};
use tokio::sync::Notify;

// Tenant lifecycle is a close-then-drain protocol:
// once deletion begins we first mark the tenant deleted so no new operations
// can enter, then we wait for the in-flight operation count to drain to zero.
// Sync callers block on the condvar path while async callers await Notify,
// but both are driven by the same atomic state and RAII operation guards.
pub(super) struct TenantLifecycle {
    deleted: AtomicBool,
    active_operations: AtomicUsize,
    zero_active_lock: Mutex<()>,
    zero_active: Condvar,
    zero_active_notify: Notify,
}

impl TenantLifecycle {
    pub(super) fn new() -> Self {
        Self {
            deleted: AtomicBool::new(false),
            active_operations: AtomicUsize::new(0),
            zero_active_lock: Mutex::new(()),
            zero_active: Condvar::new(),
            zero_active_notify: Notify::new(),
        }
    }

    pub(super) fn enter_operation(&self, tenant_id: &TenantId) -> Result<()> {
        if self.deleted.load(Ordering::Acquire) {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        }
        self.active_operations.fetch_add(1, Ordering::AcqRel);
        if self.deleted.load(Ordering::Acquire) {
            self.release_operation();
            return Err(Error::TenantNotFound(tenant_id.clone()));
        }
        Ok(())
    }

    pub(super) fn release_operation(&self) {
        if self.active_operations.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.zero_active.notify_all();
            self.zero_active_notify.notify_waiters();
        }
    }

    pub(super) fn begin_delete_blocking(&self) {
        self.deleted.store(true, Ordering::Release);
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

    pub(super) async fn begin_delete_async(&self) {
        self.deleted.store(true, Ordering::Release);
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
