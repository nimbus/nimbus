use serde::Serialize;

use crate::persistence::TenantPersistence;

/// Immutable owner of a loaded tenant's serial mutation order.
///
/// Every production persistence adapter uses the bounded ordered publisher.
/// `SerialReference` exists only to compare that production adapter with the
/// pre-publisher orchestration in deterministic tests; it is never selected by
/// production topology or by a runtime fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommitterArm {
    OrderedPublisher,
    #[cfg(any(test, feature = "test-hooks"))]
    SerialReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PersistenceAdapter {
    #[cfg(any(test, feature = "test-hooks"))]
    Memory,
    Redb,
    Sqlite,
    #[cfg(feature = "libsql")]
    Libsql,
    #[cfg(feature = "postgres")]
    Postgres,
    #[cfg(feature = "mysql")]
    MySql,
}

impl PersistenceAdapter {
    fn from_persistence(persistence: &TenantPersistence) -> Self {
        match persistence {
            TenantPersistence::Redb(_) => Self::Redb,
            TenantPersistence::Sqlite(_) => Self::Sqlite,
            #[cfg(feature = "libsql")]
            TenantPersistence::LibsqlReplica(_) => Self::Libsql,
            #[cfg(feature = "postgres")]
            TenantPersistence::Postgres(_) => Self::Postgres,
            #[cfg(feature = "mysql")]
            TenantPersistence::MySql(_) => Self::MySql,
            #[cfg(any(test, feature = "test-hooks"))]
            TenantPersistence::Memory(_) => Self::Memory,
        }
    }

    fn committer_arm(self) -> CommitterArm {
        // Arm selection describes orchestration ownership, not whether a
        // process-local write-log window is authoritative. Provider adapters
        // still require lease admission and storage-backed OCC validation.
        let _ = self;
        CommitterArm::OrderedPublisher
    }
}

impl CommitterArm {
    pub(crate) fn for_persistence(persistence: &TenantPersistence) -> Self {
        PersistenceAdapter::from_persistence(persistence).committer_arm()
    }

    pub(crate) fn uses_ordered_publisher(self) -> bool {
        match self {
            Self::OrderedPublisher => true,
            #[cfg(any(test, feature = "test-hooks"))]
            Self::SerialReference => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_production_persistence_adapters_select_ordered_publisher() {
        // The provider adapters appear only when their feature builds them, so
        // the assertion covers exactly the adapters this build can select
        // rather than naming variants that do not exist.
        for adapter in [
            PersistenceAdapter::Memory,
            PersistenceAdapter::Redb,
            PersistenceAdapter::Sqlite,
            #[cfg(feature = "libsql")]
            PersistenceAdapter::Libsql,
            #[cfg(feature = "postgres")]
            PersistenceAdapter::Postgres,
            #[cfg(feature = "mysql")]
            PersistenceAdapter::MySql,
        ] {
            assert_eq!(
                adapter.committer_arm(),
                CommitterArm::OrderedPublisher,
                "{adapter:?} must install the only production committer arm"
            );
        }
    }
}
