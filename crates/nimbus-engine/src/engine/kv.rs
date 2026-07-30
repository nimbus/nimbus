use nimbus_core::{Error, Result, TenantId};
use nimbus_storage::{KvBatchOp, KvBatchOutcome, KvEntry, KvPut, KvScanPage, TenantKvStore};

use crate::persistence::TenantPersistence;

use super::{Engine, tenants::with_tenant_runtime_operation};

impl Engine {
    pub fn tenant_kv_get(
        &self,
        tenant_id: &TenantId,
        key: &[u8],
        now_ms: i64,
    ) -> Result<Option<KvEntry>> {
        self.with_tenant_kv_store(tenant_id, |store| store.kv_get(tenant_id, key, now_ms))
    }

    pub fn tenant_kv_put(&self, tenant_id: &TenantId, put: KvPut) -> Result<()> {
        self.with_tenant_kv_store(tenant_id, |store| store.kv_put(tenant_id, put))
    }

    pub fn tenant_kv_delete(&self, tenant_id: &TenantId, key: &[u8]) -> Result<bool> {
        self.with_tenant_kv_store(tenant_id, |store| store.kv_delete(tenant_id, key))
    }

    pub fn tenant_kv_apply_batch(
        &self,
        tenant_id: &TenantId,
        ops: &[KvBatchOp],
    ) -> Result<KvBatchOutcome> {
        self.with_tenant_kv_store(tenant_id, |store| store.kv_apply_batch(tenant_id, ops))
    }

    pub fn tenant_kv_scan(
        &self,
        tenant_id: &TenantId,
        prefix: &[u8],
        cursor: Option<&[u8]>,
        limit: usize,
        now_ms: i64,
    ) -> Result<KvScanPage> {
        self.with_tenant_kv_store(tenant_id, |store| {
            store.kv_scan(tenant_id, prefix, cursor, limit, now_ms)
        })
    }

    fn with_tenant_kv_store<T>(
        &self,
        tenant_id: &TenantId,
        task: impl FnOnce(&dyn TenantKvStore) -> Result<T>,
    ) -> Result<T> {
        with_tenant_runtime_operation(self.get_existing_tenant(tenant_id)?, tenant_id, |runtime| {
            match runtime.store() {
                TenantPersistence::Redb(store) => task(store.as_ref()),
                TenantPersistence::Sqlite(_) => Err(Error::Internal(
                    "TenantKvStore is not available for the configured tenant provider".to_string(),
                )),
                #[cfg(feature = "libsql")]
                TenantPersistence::LibsqlReplica(_) => Err(Error::Internal(
                    "TenantKvStore is not available for the configured tenant provider".to_string(),
                )),
                #[cfg(feature = "postgres")]
                TenantPersistence::Postgres(_) => Err(Error::Internal(
                    "TenantKvStore is not available for the configured tenant provider".to_string(),
                )),
                #[cfg(feature = "mysql")]
                TenantPersistence::MySql(_) => Err(Error::Internal(
                    "TenantKvStore is not available for the configured tenant provider".to_string(),
                )),
                #[cfg(any(test, feature = "test-hooks"))]
                TenantPersistence::Memory(_) => Err(Error::Internal(
                    "TenantKvStore is not available for the configured tenant provider".to_string(),
                )),
            }
        })
    }
}
