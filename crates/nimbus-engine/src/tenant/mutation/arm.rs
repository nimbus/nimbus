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
    Libsql,
    Postgres,
    MySql,
}

impl PersistenceAdapter {
    fn from_persistence(persistence: &TenantPersistence) -> Self {
        match persistence {
            TenantPersistence::Redb(_) => Self::Redb,
            TenantPersistence::Sqlite(_) => Self::Sqlite,
            TenantPersistence::LibsqlReplica(_) => Self::Libsql,
            TenantPersistence::Postgres(_) => Self::Postgres,
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
        for adapter in [
            PersistenceAdapter::Memory,
            PersistenceAdapter::Redb,
            PersistenceAdapter::Sqlite,
            PersistenceAdapter::Libsql,
            PersistenceAdapter::Postgres,
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
