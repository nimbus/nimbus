use std::collections::{BTreeMap, BTreeSet, HashMap};

use nimbus_core::{
    CronJob, Document, DocumentId, DocumentLocator, DocumentPath, Error, IdSource,
    ResourcePathBinding, Result, ScheduledJob, ScheduledJobResult, Schema, SequenceNumber,
    SystemIdSource, TableId, TableName, TableState, TenantEventKind, TenantEventRecord, Timestamp,
    TriggerDeliveryCursor, TriggerInvocationKey, TriggerInvocationRecord, WriteOp,
};
use std::sync::Arc;

use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, deleting_table_namespace, hidden_table_namespace,
};
use crate::{JournalProgress, MaterializedJournalSnapshot};

#[derive(Clone)]
pub(super) struct MemoryTableIdentity {
    pub table: TableName,
    pub state: TableState,
}

#[derive(Clone)]
pub(super) struct MemoryState {
    pub revision: u64,
    id_source: Arc<dyn IdSource>,
    pub active_tables: BTreeMap<TableName, TableId>,
    pub table_identities: BTreeMap<TableId, MemoryTableIdentity>,
    pub documents: BTreeMap<TableId, BTreeMap<DocumentId, Document>>,
    pub schema: Schema,
    pub durable_journal: BTreeMap<u64, TenantEventRecord>,
    pub durable_head: SequenceNumber,
    pub applied_head: SequenceNumber,
    pub scheduled_execution_ids: BTreeSet<String>,
    pub resource_bindings: HashMap<DocumentLocator, ResourcePathBinding>,
    pub document_paths: HashMap<DocumentPath, DocumentLocator>,
    pub scheduled_jobs: BTreeMap<(Timestamp, DocumentId), ScheduledJob>,
    pub running_jobs: BTreeMap<DocumentId, ScheduledJob>,
    pub scheduled_job_results: BTreeMap<DocumentId, ScheduledJobResult>,
    pub cron_jobs: BTreeMap<String, CronJob>,
    pub trigger_delivery_cursor: TriggerDeliveryCursor,
    pub trigger_invocations: BTreeMap<TriggerInvocationKey, TriggerInvocationRecord>,
}

impl Default for MemoryState {
    fn default() -> Self {
        Self::with_id_source(Arc::new(SystemIdSource))
    }
}

impl MemoryState {
    pub fn with_id_source(id_source: Arc<dyn IdSource>) -> Self {
        Self {
            revision: 0,
            id_source,
            active_tables: BTreeMap::new(),
            table_identities: BTreeMap::new(),
            documents: BTreeMap::new(),
            schema: Schema::default(),
            durable_journal: BTreeMap::new(),
            durable_head: SequenceNumber(0),
            applied_head: SequenceNumber(0),
            scheduled_execution_ids: BTreeSet::new(),
            resource_bindings: HashMap::new(),
            document_paths: HashMap::new(),
            scheduled_jobs: BTreeMap::new(),
            running_jobs: BTreeMap::new(),
            scheduled_job_results: BTreeMap::new(),
            cron_jobs: BTreeMap::new(),
            trigger_delivery_cursor: TriggerDeliveryCursor::default(),
            trigger_invocations: BTreeMap::new(),
        }
    }

    pub fn durable_head(&self) -> SequenceNumber {
        self.durable_head
    }

    pub fn progress(&self) -> JournalProgress {
        JournalProgress {
            durable_head: self.durable_head(),
            applied_head: self.applied_head,
        }
    }

    pub fn resolve_or_create_table_id(&mut self, table: &TableName) -> Result<TableId> {
        if let Some(table_id) = self.active_tables.get(table) {
            return Ok(table_id.clone());
        }
        let table_id = loop {
            let candidate = self.id_source.next_table_id();
            if !self.table_identities.contains_key(&candidate) {
                break candidate;
            }
        };
        self.active_tables.insert(table.clone(), table_id.clone());
        self.table_identities.insert(
            table_id.clone(),
            MemoryTableIdentity {
                table: table.clone(),
                state: TableState::Active,
            },
        );
        Ok(table_id)
    }

    pub fn ensure_active_table_id(&mut self, table: &TableName, table_id: &TableId) -> Result<()> {
        if let Some(identity) = self.table_identities.get(table_id)
            && identity.table != *table
        {
            return Err(Error::conflict(format!(
                "table id {} is already assigned to logical table {}",
                table_id, identity.table
            )));
        }

        if self.active_tables.get(table) == Some(table_id) {
            return Ok(());
        }
        if let Some(previous_id) = self.active_tables.insert(table.clone(), table_id.clone())
            && previous_id != *table_id
            && let Some(previous) = self.table_identities.get_mut(&previous_id)
        {
            previous.state = TableState::Deleting;
        }
        self.table_identities.insert(
            table_id.clone(),
            MemoryTableIdentity {
                table: table.clone(),
                state: TableState::Active,
            },
        );
        Ok(())
    }

    pub fn table_id(&self, table: &TableName) -> Option<TableId> {
        self.active_tables.get(table).cloned()
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Option<Document> {
        let table_id = self.active_tables.get(table)?;
        self.documents.get(table_id)?.get(id).cloned()
    }

    pub fn append_events(
        &mut self,
        timestamp: Timestamp,
        writes: Vec<WriteOp>,
        mut events: Vec<TenantEventKind>,
    ) -> Result<nimbus_core::CommitEntry> {
        if !writes.is_empty()
            && !matches!(events.first(), Some(TenantEventKind::DocumentWrite { .. }))
        {
            events.insert(
                0,
                TenantEventKind::DocumentWrite {
                    writes: writes.clone(),
                },
            );
        }
        let sequence = SequenceNumber(self.durable_head().0.saturating_add(1));
        crate::commit_log::ensure_applied_prefix_precedes(self.applied_head, sequence)?;
        let record = TenantEventRecord::from_events(sequence, timestamp, events)?;
        self.durable_journal.insert(sequence.0, record);
        self.durable_head = sequence;
        self.applied_head = sequence;
        Ok(nimbus_core::CommitEntry {
            sequence,
            timestamp,
            writes,
        })
    }

    pub fn upsert_resource_path_binding(&mut self, binding: &ResourcePathBinding) -> Result<()> {
        if let Some(existing_locator) = self.document_paths.get(&binding.document_path)
            && existing_locator != &binding.locator
        {
            return Err(Error::AlreadyExists(format!(
                "document path already bound: {}",
                binding.document_path
            )));
        }
        if let Some(previous) = self
            .resource_bindings
            .insert(binding.locator.clone(), binding.clone())
        {
            self.document_paths.remove(&previous.document_path);
        }
        self.document_paths
            .insert(binding.document_path.clone(), binding.locator.clone());
        Ok(())
    }

    pub fn remove_resource_path_binding(
        &mut self,
        locator: &DocumentLocator,
    ) -> Option<ResourcePathBinding> {
        let binding = self.resource_bindings.remove(locator)?;
        self.document_paths.remove(&binding.document_path);
        Some(binding)
    }

    pub fn materialized_snapshot(&self) -> MaterializedJournalSnapshot {
        let mut table_identities = self
            .table_identities
            .iter()
            .map(|(table_id, identity)| crate::TableIdentitySnapshotEntry {
                namespace: match identity.state {
                    TableState::Active => DEFAULT_TABLE_NAMESPACE.to_string(),
                    TableState::Hidden => hidden_table_namespace(table_id),
                    TableState::Deleting => deleting_table_namespace(table_id),
                },
                table: identity.table.clone(),
                table_id: table_id.clone(),
                state: identity.state,
            })
            .collect::<Vec<_>>();
        table_identities.sort_by(|left, right| {
            (&left.namespace, &left.table, &left.table_id).cmp(&(
                &right.namespace,
                &right.table,
                &right.table_id,
            ))
        });
        let documents = self
            .active_tables
            .values()
            .filter_map(|table_id| self.documents.get(table_id))
            .flat_map(|documents| documents.values().cloned())
            .collect();
        MaterializedJournalSnapshot {
            version: crate::store::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: self.applied_head,
            durable_head: self.durable_head(),
            table_identities,
            schema: self.schema.clone(),
            documents,
            scheduled_execution_ids: self.scheduled_execution_ids.iter().cloned().collect(),
        }
    }
}
