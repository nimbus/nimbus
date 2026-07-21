//! Durable embedded tenant-incarnation authority.
//!
//! Tenant databases are intentionally deleted on tenant removal, so ordering
//! state that must survive same-id recreation belongs in the control plane.

use std::sync::Arc;

use nimbus_core::{Error, Result, TenantId};
use redb::{ReadableTable, TableDefinition, TableError};

use crate::UsageStore;
use crate::store::map_redb_error;

const TENANT_INCARNATIONS: TableDefinition<&[u8], u64> =
    TableDefinition::new("tenant_incarnations");

/// Monotonic incarnation allocator over the embedded control database.
#[derive(Clone)]
pub struct TenantIncarnationStore {
    usage_store: Arc<UsageStore>,
}

impl TenantIncarnationStore {
    pub(crate) fn new(usage_store: Arc<UsageStore>) -> Self {
        Self { usage_store }
    }

    /// Advances the durable incarnation for a newly created tenant.
    ///
    /// Gaps are legal: a crash after allocation but before tenant creation can
    /// consume a value, while reuse of an older value would be unsafe.
    pub fn advance(&self, tenant_id: &TenantId) -> Result<u64> {
        let write_txn = self
            .usage_store
            .database()
            .begin_write()
            .map_err(map_redb_error)?;
        let next = {
            let mut table = write_txn
                .open_table(TENANT_INCARNATIONS)
                .map_err(map_redb_error)?;
            let current = table
                .get(tenant_id.as_str().as_bytes())
                .map_err(map_redb_error)?
                .map(|value| value.value())
                .unwrap_or(0);
            let next = current.checked_add(1).ok_or_else(|| {
                Error::ResourceExhausted(format!("tenant incarnation exhausted for {tenant_id}"))
            })?;
            table
                .insert(tenant_id.as_str().as_bytes(), next)
                .map_err(map_redb_error)?;
            next
        };
        write_txn.commit().map_err(map_redb_error)?;
        Ok(next)
    }

    /// Returns the incarnation allocated by the tenant creation lifecycle.
    ///
    /// An active tenant without this record is corrupt. Nimbus is pre-launch,
    /// so opening must fail closed rather than minting compatibility state that
    /// could collide with work from an older tenant lifetime.
    pub fn current(&self, tenant_id: &TenantId) -> Result<u64> {
        let read_txn = self
            .usage_store
            .database()
            .begin_read()
            .map_err(map_redb_error)?;
        let table = match read_txn.open_table(TENANT_INCARNATIONS) {
            Ok(table) => table,
            Err(TableError::TableDoesNotExist(_)) => {
                return Err(missing_incarnation(tenant_id));
            }
            Err(error) => return Err(map_redb_error(error)),
        };
        let incarnation = table
            .get(tenant_id.as_str().as_bytes())
            .map_err(map_redb_error)?
            .map(|value| value.value());
        require_tenant_incarnation(incarnation, tenant_id)
    }
}

pub(crate) fn require_tenant_incarnation(
    incarnation: Option<u64>,
    tenant_id: &TenantId,
) -> Result<u64> {
    incarnation
        .filter(|incarnation| *incarnation > 0)
        .ok_or_else(|| missing_incarnation(tenant_id))
}

fn missing_incarnation(tenant_id: &TenantId) -> Error {
    Error::Serialization(format!(
        "active tenant {tenant_id} is missing a positive durable incarnation"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> TenantIncarnationStore {
        TenantIncarnationStore::new(Arc::new(
            UsageStore::create_in_memory().expect("control store should create"),
        ))
    }

    #[test]
    fn incarnation_requires_creation_and_advances_monotonically_per_tenant() {
        let store = store();
        let first = TenantId::new("first").unwrap();
        let second = TenantId::new("second").unwrap();

        assert!(matches!(
            store.current(&first),
            Err(Error::Serialization(_))
        ));
        assert_eq!(store.advance(&first).unwrap(), 1);
        assert_eq!(store.current(&first).unwrap(), 1);
        assert_eq!(store.advance(&first).unwrap(), 2);
        assert_eq!(store.advance(&first).unwrap(), 3);
        assert_eq!(store.advance(&second).unwrap(), 1);
        assert_eq!(store.current(&second).unwrap(), 1);

        assert!(matches!(
            require_tenant_incarnation(None, &first),
            Err(Error::Serialization(_))
        ));
        assert!(matches!(
            require_tenant_incarnation(Some(0), &first),
            Err(Error::Serialization(_))
        ));
    }
}
