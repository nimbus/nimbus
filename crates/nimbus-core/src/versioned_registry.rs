use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    Error, HistoricalReadErrorKind, HistoricalReadSnapshot, IndexDefinition, IndexId,
    PolicySnapshotId, Result, SchemaChangeEvent, TableId, TableLifecycleEvent, TableName,
    TableSchema, TableState, TenantEventKind, TenantEventRecord, policy_revision_id,
};

/// Metadata bundle a historical read must resolve before reading document history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalReadShape {
    read_snapshot: HistoricalReadSnapshot,
    table: TableName,
    table_id: TableId,
    schema: Option<TableSchema>,
    queryable_indexes: Vec<IndexDefinition>,
    policy_snapshot: PolicySnapshotId,
    storage_format_generation: u16,
}

impl HistoricalReadShape {
    pub fn read_snapshot(&self) -> HistoricalReadSnapshot {
        self.read_snapshot
    }

    pub fn table(&self) -> &TableName {
        &self.table
    }

    pub fn table_id(&self) -> &TableId {
        &self.table_id
    }

    pub fn schema(&self) -> Option<&TableSchema> {
        self.schema.as_ref()
    }

    pub fn queryable_indexes(&self) -> &[IndexDefinition] {
        self.queryable_indexes.as_slice()
    }

    pub fn policy_snapshot(&self) -> &PolicySnapshotId {
        &self.policy_snapshot
    }

    pub fn storage_format_generation(&self) -> u16 {
        self.storage_format_generation
    }
}

/// Canonical event-derived registry model for historical table read shapes.
#[derive(Debug, Clone)]
pub struct VersionedRegistry {
    records: Vec<TenantEventRecord>,
    storage_format_generation: u16,
}

impl VersionedRegistry {
    pub fn from_records(records: impl IntoIterator<Item = TenantEventRecord>) -> Result<Self> {
        Self::from_records_with_format_generation(records, 1)
    }

    pub fn from_records_with_format_generation(
        records: impl IntoIterator<Item = TenantEventRecord>,
        storage_format_generation: u16,
    ) -> Result<Self> {
        if storage_format_generation == 0 {
            return Err(Error::historical_read(
                HistoricalReadErrorKind::FormatMismatch,
                "historical registry storage format generation cannot be zero",
            ));
        }

        let mut records = records.into_iter().collect::<Vec<_>>();
        records.sort_by_key(|record| record.sequence);
        let mut previous_sequence = None;
        for record in &records {
            record.validate_integrity()?;
            if previous_sequence == Some(record.sequence) {
                return Err(Error::Conflict(format!(
                    "duplicate tenant event sequence {} in historical registry",
                    record.sequence
                )));
            }
            previous_sequence = Some(record.sequence);
        }

        Ok(Self {
            records,
            storage_format_generation,
        })
    }

    pub fn read_shape_at(
        &self,
        table: &TableName,
        read_snapshot: HistoricalReadSnapshot,
    ) -> Result<Option<HistoricalReadShape>> {
        let mut state = RegistryState::default();
        for record in self
            .records
            .iter()
            .filter(|record| record.sequence <= read_snapshot.sequence().sequence())
        {
            state.apply_record(record)?;
        }
        state.read_shape(table, read_snapshot, self.storage_format_generation)
    }
}

#[derive(Debug, Clone, Default)]
struct RegistryState {
    tables: BTreeMap<(TableName, TableId), TableVersionState>,
}

impl RegistryState {
    fn apply_record(&mut self, record: &TenantEventRecord) -> Result<()> {
        for event in record.events() {
            self.apply_event(event)?;
        }
        Ok(())
    }

    fn apply_event(&mut self, event: &TenantEventKind) -> Result<()> {
        match event {
            TenantEventKind::DocumentWrite { writes } => {
                for write in writes {
                    self.ensure_table(&write.table, &write.table_id, TableState::Active);
                }
            }
            TenantEventKind::SchemaChange { change } => self.apply_schema_change(change)?,
            TenantEventKind::TableLifecycle { lifecycle } => self.apply_table_lifecycle(lifecycle),
            TenantEventKind::IndexLifecycle { index } => {
                let state = self.ensure_table(&index.table, &index.table_id, TableState::Active);
                if let Some(schema) = state.schema.as_mut() {
                    replace_index_definition(
                        schema,
                        index.index_id.clone(),
                        index.definition.clone(),
                    );
                }
            }
            TenantEventKind::ScheduledExecution { .. }
            | TenantEventKind::TriggerDelivery { .. }
            | TenantEventKind::Barrier { .. } => {}
        }
        Ok(())
    }

    fn apply_schema_change(&mut self, change: &SchemaChangeEvent) -> Result<()> {
        match change {
            SchemaChangeEvent::SetTable {
                table,
                table_id,
                current,
                ..
            } => {
                self.ensure_table(table, table_id, TableState::Active)
                    .schema = Some(current.clone());
            }
            SchemaChangeEvent::DeleteTable {
                table, table_id, ..
            } => {
                let table_id = match table_id {
                    Some(table_id) => Some(table_id.clone()),
                    None => self.active_table_id(table)?,
                };
                if let Some(table_id) = table_id
                    && let Some(state) = self.tables.get_mut(&(table.clone(), table_id))
                {
                    state.schema = None;
                }
            }
        }
        Ok(())
    }

    fn apply_table_lifecycle(&mut self, lifecycle: &TableLifecycleEvent) {
        match lifecycle {
            TableLifecycleEvent::StageHidden { table, table_id } => {
                self.ensure_table(table, table_id, TableState::Hidden).state = TableState::Hidden;
            }
            TableLifecycleEvent::ActivateHidden {
                table,
                table_id,
                replaced_table_id,
            } => {
                if let Some(replaced_table_id) = replaced_table_id {
                    self.ensure_table(table, replaced_table_id, TableState::Deleting)
                        .state = TableState::Deleting;
                } else if let Ok(Some(active_table_id)) = self.active_table_id(table) {
                    self.ensure_table(table, &active_table_id, TableState::Deleting)
                        .state = TableState::Deleting;
                }
                self.ensure_table(table, table_id, TableState::Active).state = TableState::Active;
            }
            TableLifecycleEvent::MarkDeleting { table, table_id } => {
                self.ensure_table(table, table_id, TableState::Deleting)
                    .state = TableState::Deleting;
            }
            TableLifecycleEvent::HardDelete { table, table_id } => {
                self.tables.remove(&(table.clone(), table_id.clone()));
            }
        }
    }

    fn ensure_table(
        &mut self,
        table: &TableName,
        table_id: &TableId,
        state: TableState,
    ) -> &mut TableVersionState {
        self.tables
            .entry((table.clone(), table_id.clone()))
            .or_insert_with(|| TableVersionState {
                table: table.clone(),
                table_id: table_id.clone(),
                state,
                schema: None,
            })
    }

    fn active_table_id(&self, table: &TableName) -> Result<Option<TableId>> {
        let active = self
            .tables
            .values()
            .filter(|state| &state.table == table && state.state == TableState::Active)
            .map(|state| state.table_id.clone())
            .collect::<Vec<_>>();
        match active.as_slice() {
            [] => Ok(None),
            [table_id] => Ok(Some(table_id.clone())),
            _ => Err(Error::Conflict(format!(
                "historical registry has multiple active table ids for logical table {}",
                table
            ))),
        }
    }

    fn active_table(&self, table: &TableName) -> Result<Option<&TableVersionState>> {
        let active = self
            .tables
            .values()
            .filter(|state| &state.table == table && state.state == TableState::Active)
            .collect::<Vec<_>>();
        match active.as_slice() {
            [] => Ok(None),
            [state] => Ok(Some(*state)),
            _ => Err(Error::Conflict(format!(
                "historical registry has multiple active table ids for logical table {}",
                table
            ))),
        }
    }

    fn read_shape(
        &self,
        table: &TableName,
        read_snapshot: HistoricalReadSnapshot,
        storage_format_generation: u16,
    ) -> Result<Option<HistoricalReadShape>> {
        let Some(active) = self.active_table(table)? else {
            return Ok(None);
        };
        let policy_revision = active
            .schema
            .as_ref()
            .map(|schema| schema.access_policy_revision())
            .unwrap_or_else(|| policy_revision_id(None))?;
        let mut queryable_indexes = active
            .schema
            .as_ref()
            .map(|schema| schema.queryable_indexes().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        queryable_indexes.sort_by(|left, right| left.id.cmp(&right.id));

        Ok(Some(HistoricalReadShape {
            read_snapshot,
            table: active.table.clone(),
            table_id: active.table_id.clone(),
            schema: active.schema.clone(),
            queryable_indexes,
            policy_snapshot: PolicySnapshotId::new(policy_revision)?,
            storage_format_generation,
        }))
    }
}

#[derive(Debug, Clone)]
struct TableVersionState {
    table: TableName,
    table_id: TableId,
    state: TableState,
    schema: Option<TableSchema>,
}

fn replace_index_definition(
    schema: &mut TableSchema,
    index_id: IndexId,
    definition: IndexDefinition,
) {
    if let Some(existing) = schema.indexes.iter_mut().find(|index| index.id == index_id) {
        *existing = definition;
    } else {
        schema.indexes.push(definition);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        AccessRule, CommitTimestamp, FieldSchema, FieldType, ReadTimestamp, SequenceNumber,
        TableAccessPolicy, Timestamp,
    };

    use super::*;

    fn table(name: &str) -> TableName {
        TableName::new(name).expect("table name should build")
    }

    fn snapshot(sequence: u64) -> HistoricalReadSnapshot {
        HistoricalReadSnapshot::new(
            ReadTimestamp::new(Timestamp(sequence * 100)),
            crate::CommitSequence::new(SequenceNumber(sequence)),
            CommitTimestamp::new(Timestamp(sequence * 100)),
        )
    }

    fn schema(table: &TableName, index: IndexDefinition) -> TableSchema {
        TableSchema {
            table: table.clone(),
            fields: vec![FieldSchema {
                name: "owner".to_string(),
                field_type: FieldType::String,
                required: false,
            }],
            indexes: vec![index],
            access_policy: None,
        }
    }

    fn authenticated_policy_schema(table: &TableName, index: IndexDefinition) -> TableSchema {
        TableSchema {
            access_policy: Some(TableAccessPolicy {
                read: AccessRule {
                    require_authenticated: true,
                    ..AccessRule::default()
                },
                ..TableAccessPolicy::default()
            }),
            ..schema(table, index)
        }
    }

    fn schema_record(sequence: u64, table_id: &TableId, schema: TableSchema) -> TenantEventRecord {
        TenantEventRecord::schema_change(
            SequenceNumber(sequence),
            Timestamp(sequence * 100),
            SchemaChangeEvent::SetTable {
                table: schema.table.clone(),
                table_id: table_id.clone(),
                previous: None,
                current: schema,
            },
        )
        .expect("schema event should build")
    }

    fn lifecycle_record(sequence: u64, lifecycle: TableLifecycleEvent) -> TenantEventRecord {
        TenantEventRecord::table_lifecycle(
            SequenceNumber(sequence),
            Timestamp(sequence * 100),
            lifecycle,
        )
        .expect("lifecycle event should build")
    }

    #[test]
    fn read_shape_resolves_schema_policy_and_indexes_as_of_snapshot() {
        let table = table("messages");
        let table_id = TableId::new();
        let first_index = IndexDefinition::new("by_owner", ["owner"]);
        let first_policy =
            PolicySnapshotId::new(policy_revision_id(None).expect("policy should hash")).unwrap();
        let second_index = IndexDefinition::new("by_owner_v2", ["owner"]);
        let second_schema = authenticated_policy_schema(&table, second_index.clone());
        let second_policy = PolicySnapshotId::new(
            second_schema
                .access_policy_revision()
                .expect("policy should hash"),
        )
        .unwrap();
        let registry = VersionedRegistry::from_records([
            schema_record(1, &table_id, schema(&table, first_index.clone())),
            schema_record(2, &table_id, second_schema),
        ])
        .unwrap();

        let at_first = registry
            .read_shape_at(&table, snapshot(1))
            .unwrap()
            .expect("table should exist");
        let at_second = registry
            .read_shape_at(&table, snapshot(2))
            .unwrap()
            .expect("table should exist");

        assert_eq!(at_first.table_id(), &table_id);
        assert_eq!(at_first.policy_snapshot(), &first_policy);
        assert_eq!(at_first.queryable_indexes(), &[first_index]);
        assert_eq!(at_second.policy_snapshot(), &second_policy);
        assert_eq!(at_second.queryable_indexes(), &[second_index]);
    }

    #[test]
    fn read_shape_does_not_leak_replaced_table_identity() {
        let table = table("tasks");
        let old_table_id = TableId::new();
        let replacement_table_id = TableId::new();
        let registry = VersionedRegistry::from_records([
            schema_record(
                1,
                &old_table_id,
                schema(&table, IndexDefinition::new("by_owner", ["owner"])),
            ),
            lifecycle_record(
                2,
                TableLifecycleEvent::StageHidden {
                    table: table.clone(),
                    table_id: replacement_table_id.clone(),
                },
            ),
            lifecycle_record(
                3,
                TableLifecycleEvent::ActivateHidden {
                    table: table.clone(),
                    table_id: replacement_table_id.clone(),
                    replaced_table_id: Some(old_table_id.clone()),
                },
            ),
        ])
        .unwrap();

        assert_eq!(
            registry
                .read_shape_at(&table, snapshot(1))
                .unwrap()
                .expect("old table should be active")
                .table_id(),
            &old_table_id
        );
        assert_eq!(
            registry
                .read_shape_at(&table, snapshot(2))
                .unwrap()
                .expect("old table should remain active while replacement is hidden")
                .table_id(),
            &old_table_id
        );
        assert_eq!(
            registry
                .read_shape_at(&table, snapshot(3))
                .unwrap()
                .expect("replacement table should be active")
                .table_id(),
            &replacement_table_id
        );
    }

    #[test]
    fn read_shape_returns_none_after_table_enters_deleting_state() {
        let table = table("events");
        let table_id = TableId::new();
        let registry = VersionedRegistry::from_records([
            schema_record(
                1,
                &table_id,
                schema(&table, IndexDefinition::new("by_owner", ["owner"])),
            ),
            lifecycle_record(
                2,
                TableLifecycleEvent::MarkDeleting {
                    table: table.clone(),
                    table_id: table_id.clone(),
                },
            ),
        ])
        .unwrap();

        assert!(
            registry
                .read_shape_at(&table, snapshot(1))
                .unwrap()
                .is_some()
        );
        assert!(
            registry
                .read_shape_at(&table, snapshot(2))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn read_shape_returns_none_before_create_and_after_hard_delete() {
        let table = table("archive");
        let table_id = TableId::new();
        let registry = VersionedRegistry::from_records([
            schema_record(
                2,
                &table_id,
                schema(&table, IndexDefinition::new("by_owner", ["owner"])),
            ),
            lifecycle_record(
                3,
                TableLifecycleEvent::MarkDeleting {
                    table: table.clone(),
                    table_id: table_id.clone(),
                },
            ),
            lifecycle_record(
                4,
                TableLifecycleEvent::HardDelete {
                    table: table.clone(),
                    table_id,
                },
            ),
        ])
        .unwrap();

        assert!(
            registry
                .read_shape_at(&table, snapshot(1))
                .unwrap()
                .is_none()
        );
        assert!(
            registry
                .read_shape_at(&table, snapshot(4))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn index_lifecycle_event_updates_queryable_index_snapshot() {
        let table = table("audit");
        let table_id = TableId::new();
        let pending_index =
            IndexDefinition::with_state("by_owner", ["owner"], crate::IndexState::Pending);
        let enabled_index = IndexDefinition {
            state: crate::IndexState::Enabled,
            ..pending_index.clone()
        };
        let index_event = TenantEventRecord::from_events(
            SequenceNumber(2),
            Timestamp(200),
            vec![TenantEventKind::IndexLifecycle {
                index: crate::IndexLifecycleEvent {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    index_id: enabled_index.id.clone(),
                    state: enabled_index.state,
                    definition: enabled_index.clone(),
                },
            }],
        )
        .expect("index lifecycle event should build");
        let registry = VersionedRegistry::from_records([
            schema_record(1, &table_id, schema(&table, pending_index)),
            index_event,
        ])
        .unwrap();

        assert!(
            registry
                .read_shape_at(&table, snapshot(1))
                .unwrap()
                .expect("table should exist")
                .queryable_indexes()
                .is_empty()
        );
        assert_eq!(
            registry
                .read_shape_at(&table, snapshot(2))
                .unwrap()
                .expect("table should exist")
                .queryable_indexes(),
            &[enabled_index]
        );
    }

    #[test]
    fn deleting_schema_keeps_schemaless_table_identity() {
        let table = table("logs");
        let table_id = TableId::new();
        let delete_schema = TenantEventRecord::schema_change(
            SequenceNumber(2),
            Timestamp(200),
            SchemaChangeEvent::DeleteTable {
                table: table.clone(),
                table_id: Some(table_id.clone()),
                previous: None,
            },
        )
        .expect("delete schema event should build");
        let registry = VersionedRegistry::from_records([
            schema_record(
                1,
                &table_id,
                schema(&table, IndexDefinition::new("by_owner", ["owner"])),
            ),
            delete_schema,
        ])
        .unwrap();

        let shape = registry
            .read_shape_at(&table, snapshot(2))
            .unwrap()
            .expect("table identity should remain active without schema");

        assert_eq!(shape.table_id(), &table_id);
        assert!(shape.schema().is_none());
        assert!(shape.queryable_indexes().is_empty());
        assert_eq!(
            shape.policy_snapshot(),
            &PolicySnapshotId::new(policy_revision_id(None).unwrap()).unwrap()
        );
    }

    #[test]
    fn registry_rejects_duplicate_event_sequences() {
        let table = table("dupes");
        let first = schema_record(
            1,
            &TableId::new(),
            schema(&table, IndexDefinition::new("by_owner", ["owner"])),
        );
        let second = TenantEventRecord::barrier(SequenceNumber(1), Timestamp(100), "same".into())
            .expect("barrier should build");

        let error = VersionedRegistry::from_records([first, second]).unwrap_err();

        assert!(matches!(error, Error::Conflict(message) if message.contains("duplicate")));
    }

    #[test]
    fn registry_rejects_unknown_format_generation() {
        let error = VersionedRegistry::from_records_with_format_generation([], 0).unwrap_err();

        assert_eq!(
            error.historical_read_kind(),
            Some(HistoricalReadErrorKind::FormatMismatch)
        );
    }
}
