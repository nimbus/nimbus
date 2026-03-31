use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use neovex_core::{Error, Result, Schema, TenantId};
use neovex_storage::TenantStore;

use crate::subscriptions::SubscriptionRegistry;

/// Runtime state for a loaded tenant.
pub struct TenantRuntime {
    pub store: Arc<TenantStore>,
    pub subscriptions: SubscriptionRegistry,
    pub schema: RwLock<Schema>,
    lifecycle: RwLock<()>,
    deleted: AtomicBool,
}

pub struct TenantOperationGuard<'a> {
    _guard: RwLockReadGuard<'a, ()>,
}

pub struct TenantDeletionGuard<'a> {
    _guard: RwLockWriteGuard<'a, ()>,
}

impl TenantRuntime {
    /// Creates a tenant runtime from a store.
    pub fn new(store: TenantStore) -> Result<Self> {
        let schema = store.load_schema()?;
        Ok(Self {
            store: Arc::new(store),
            subscriptions: SubscriptionRegistry::new(),
            schema: RwLock::new(schema),
            lifecycle: RwLock::new(()),
            deleted: AtomicBool::new(false),
        })
    }

    /// Returns the current schema snapshot.
    pub fn schema(&self) -> Schema {
        self.schema
            .read()
            .expect("schema lock should not be poisoned")
            .clone()
    }

    /// Enters a tenant operation, preventing deletion while the operation is active.
    pub fn enter_operation(&self, tenant_id: &TenantId) -> Result<TenantOperationGuard<'_>> {
        let guard = self
            .lifecycle
            .read()
            .expect("tenant lifecycle lock should not be poisoned");
        if self.deleted.load(Ordering::SeqCst) {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        }

        Ok(TenantOperationGuard { _guard: guard })
    }

    /// Begins tenant deletion and blocks until all in-flight operations complete.
    pub fn begin_delete(&self) -> TenantDeletionGuard<'_> {
        let guard = self
            .lifecycle
            .write()
            .expect("tenant lifecycle lock should not be poisoned");
        self.deleted.store(true, Ordering::SeqCst);
        TenantDeletionGuard { _guard: guard }
    }
}
