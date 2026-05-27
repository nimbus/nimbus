#[cfg(test)]
use std::collections::BTreeMap;

#[cfg(test)]
use nimbus_core::{Error, Result};
use nimbus_core::{TableId, TableName, TableState};
use serde::{Deserialize, Serialize};

pub(crate) const DEFAULT_TABLE_NAMESPACE: &str = "default";

pub(crate) fn hidden_table_namespace(table_id: &TableId) -> String {
    format!("hidden:{}", table_id.as_str())
}

pub(crate) fn deleting_table_namespace(table_id: &TableId) -> String {
    format!("deleting:{}", table_id.as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableLifecycleTransition {
    StageHidden,
    ActivateHidden,
    MarkDeleting,
    HardDelete,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TableLifecycleStateMachine;

pub fn apply_table_lifecycle_transition(
    current: Option<TableState>,
    transition: TableLifecycleTransition,
) -> nimbus_core::Result<Option<TableState>> {
    TableLifecycleStateMachine::apply(current, transition)
}

impl TableLifecycleStateMachine {
    pub fn apply(
        current: Option<TableState>,
        transition: TableLifecycleTransition,
    ) -> nimbus_core::Result<Option<TableState>> {
        match (current, transition) {
            (None, TableLifecycleTransition::StageHidden) => Ok(Some(TableState::Hidden)),
            (Some(TableState::Hidden), TableLifecycleTransition::ActivateHidden) => {
                Ok(Some(TableState::Active))
            }
            (Some(TableState::Active), TableLifecycleTransition::MarkDeleting) => {
                Ok(Some(TableState::Deleting))
            }
            (Some(TableState::Deleting), TableLifecycleTransition::HardDelete) => Ok(None),
            (state, transition) => Err(nimbus_core::Error::Conflict(format!(
                "invalid table lifecycle transition {:?} from {:?}",
                transition, state
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg(test)]
pub(crate) struct TableCatalogKey {
    namespace: String,
    table_name: TableName,
}

#[cfg(test)]
impl TableCatalogKey {
    pub fn default_namespace(table_name: TableName) -> Self {
        Self {
            namespace: DEFAULT_TABLE_NAMESPACE.to_string(),
            table_name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub(crate) struct TableCatalogEntry {
    key: TableCatalogKey,
    table_id: TableId,
    state: TableState,
}

#[cfg(test)]
impl TableCatalogEntry {
    pub fn new(key: TableCatalogKey, table_id: TableId) -> Self {
        Self {
            key,
            table_id,
            state: TableState::Active,
        }
    }

    pub fn with_state(key: TableCatalogKey, table_id: TableId, state: TableState) -> Self {
        Self {
            key,
            table_id,
            state,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg(test)]
pub(crate) struct TenantTableCatalog {
    entries: BTreeMap<TableCatalogKey, (TableId, TableState)>,
}

#[cfg(test)]
impl TenantTableCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolve_or_create(&mut self, key: TableCatalogKey) -> TableId {
        self.entries
            .entry(key)
            .or_insert_with(|| (TableId::new(), TableState::Active))
            .0
            .clone()
    }

    pub fn resolve_or_create_default(&mut self, table_name: TableName) -> TableId {
        self.resolve_or_create(TableCatalogKey::default_namespace(table_name))
    }

    pub fn insert_existing(&mut self, entry: TableCatalogEntry) -> Result<Option<TableId>> {
        if self
            .entries
            .iter()
            .any(|(key, (table_id, _))| key != &entry.key && table_id == &entry.table_id)
        {
            return Err(Error::InvalidInput(format!(
                "table id {} is already assigned to another table",
                entry.table_id
            )));
        }
        Ok(self
            .entries
            .insert(entry.key, (entry.table_id, entry.state))
            .map(|(table_id, _)| table_id))
    }

    pub fn remove(&mut self, key: &TableCatalogKey) -> Option<TableId> {
        self.entries.remove(key).map(|(table_id, _)| table_id)
    }

    pub fn state(&self, key: &TableCatalogKey) -> Option<TableState> {
        self.entries.get(key).map(|(_, state)| *state)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableIdentitySnapshotEntry {
    pub namespace: String,
    pub table: TableName,
    pub table_id: TableId,
    #[serde(default)]
    pub state: TableState,
}

impl TableIdentitySnapshotEntry {
    pub fn default_namespace(table: TableName, table_id: TableId) -> Self {
        Self {
            namespace: DEFAULT_TABLE_NAMESPACE.to_string(),
            table,
            table_id,
            state: TableState::Active,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableBackendLayout {
    RedbKeyspaceByTableId,
    SharedDocumentsByTableId,
    LibsqlReplicaSharedDocumentsByTableId,
}

impl TableBackendLayout {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RedbKeyspaceByTableId => "redb_keyspace_by_table_id",
            Self::SharedDocumentsByTableId => "shared_documents_by_table_id",
            Self::LibsqlReplicaSharedDocumentsByTableId => {
                "libsql_replica_shared_documents_by_table_id"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TableSummaryStatus {
    ExactDocumentCount,
    Unsupported,
}

impl TableSummaryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExactDocumentCount => "exact_document_count",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableIdentityDiagnostic {
    pub table_name: TableName,
    pub table_id: TableId,
    pub state: TableState,
    pub backend_layout: TableBackendLayout,
    pub document_count: Option<u64>,
    pub summary_status: TableSummaryStatus,
}

impl TableIdentityDiagnostic {
    pub fn from_snapshot_entry(
        identity: &TableIdentitySnapshotEntry,
        backend_layout: TableBackendLayout,
        document_count: Option<u64>,
    ) -> Self {
        Self {
            table_name: identity.table.clone(),
            table_id: identity.table_id.clone(),
            state: identity.state,
            backend_layout,
            document_count,
            summary_status: if document_count.is_some() {
                TableSummaryStatus::ExactDocumentCount
            } else {
                TableSummaryStatus::Unsupported
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_resolves_stable_id_for_active_name() {
        let table = TableName::new("messages").expect("table name should parse");
        let mut catalog = TenantTableCatalog::new();

        let first = catalog.resolve_or_create_default(table.clone());
        let second = catalog.resolve_or_create_default(table);

        assert_eq!(first, second);
    }

    #[test]
    fn table_lifecycle_state_machine_rejects_invalid_transitions() {
        assert_eq!(
            apply_table_lifecycle_transition(None, TableLifecycleTransition::StageHidden)
                .expect("stage hidden should be valid"),
            Some(TableState::Hidden)
        );
        assert!(
            apply_table_lifecycle_transition(
                Some(TableState::Active),
                TableLifecycleTransition::ActivateHidden,
            )
            .is_err()
        );
    }

    #[test]
    fn catalog_recreate_gets_new_id_after_remove() {
        let table = TableName::new("messages").expect("table name should parse");
        let key = TableCatalogKey::default_namespace(table);
        let mut catalog = TenantTableCatalog::new();

        let first = catalog.resolve_or_create(key.clone());
        let removed = catalog.remove(&key).expect("entry should exist");
        let second = catalog.resolve_or_create(key);

        assert_eq!(first, removed);
        assert_ne!(first, second);
    }

    #[test]
    fn catalog_rejects_duplicate_id_for_distinct_names() {
        let first = TableCatalogKey::default_namespace(
            TableName::new("messages").expect("table name should parse"),
        );
        let second = TableCatalogKey::default_namespace(
            TableName::new("users").expect("table name should parse"),
        );
        let table_id = TableId::new();
        let mut catalog = TenantTableCatalog::new();

        catalog
            .insert_existing(TableCatalogEntry::new(first, table_id.clone()))
            .expect("first insert should succeed");
        let error = catalog
            .insert_existing(TableCatalogEntry::new(second, table_id))
            .expect_err("duplicate table id should fail");

        assert!(
            error.to_string().contains("already assigned"),
            "duplicate table id should produce an actionable error: {error:?}"
        );
    }

    #[test]
    fn catalog_records_lifecycle_state() {
        let table = TableName::new("messages").expect("table name should parse");
        let key = TableCatalogKey::default_namespace(table);
        let table_id = TableId::new();
        let mut catalog = TenantTableCatalog::new();

        catalog
            .insert_existing(TableCatalogEntry::with_state(
                key.clone(),
                table_id,
                TableState::Deleting,
            ))
            .expect("deleting entry should insert");

        assert_eq!(catalog.state(&key), Some(TableState::Deleting));
    }
}
