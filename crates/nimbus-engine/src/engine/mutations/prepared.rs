use std::collections::{HashMap, HashSet};

use nimbus_core::{
    DependencySet, Document, DocumentId, Error, IndexDefinition, ResourcePathBinding, Result,
    SequenceNumber, TableId, TableName, TenantEventRecord, Timestamp, TriggerWriteOrigin, WriteOp,
    WriteOpType,
};
use nimbus_storage::{ResolvedScheduleOp, ResolvedWrite};

use super::caps::MutationUsage;

/// The document material known to the engine before persistence.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::engine) enum PreparedDocument {
    Full(Document),
}

/// One engine-layer write intent, with unavailable storage-owned details left absent.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::engine) struct PreparedWrite {
    pub(in crate::engine) table: TableName,
    pub(in crate::engine) table_id: Option<TableId>,
    pub(in crate::engine) op_type: WriteOpType,
    pub(in crate::engine) doc_id: DocumentId,
    pub(in crate::engine) resource_path_binding: Option<ResourcePathBinding>,
    pub(in crate::engine) trigger_write_origin: Option<TriggerWriteOrigin>,
    pub(in crate::engine) previous: Option<Document>,
    pub(in crate::engine) current: Option<PreparedDocument>,
}

impl PreparedWrite {
    fn from_complete(write: WriteOp) -> Self {
        Self {
            table: write.table,
            table_id: Some(write.table_id),
            op_type: write.op_type,
            doc_id: write.doc_id,
            resource_path_binding: write.resource_path_binding,
            trigger_write_origin: write.trigger_write_origin,
            previous: write.previous,
            current: write.current.map(PreparedDocument::Full),
        }
    }

    fn into_complete(self) -> Result<WriteOp> {
        let table_id = self.table_id.ok_or_else(|| {
            Error::Internal(
                "journal prepared write must contain a stable table identity".to_string(),
            )
        })?;
        let current = match self.current {
            Some(PreparedDocument::Full(document)) => Some(document),
            None => None,
        };
        Ok(WriteOp {
            table: self.table,
            table_id,
            op_type: self.op_type,
            doc_id: self.doc_id,
            resource_path_binding: self.resource_path_binding,
            trigger_write_origin: self.trigger_write_origin,
            previous: self.previous,
            current,
        })
    }
}

/// Index maintenance input known before storage materializes concrete key changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::engine) struct PreparedIndexDelta {
    pub(in crate::engine) table: TableName,
    pub(in crate::engine) doc_id: DocumentId,
    pub(in crate::engine) index: IndexDefinition,
}

/// Exact path-specific inputs that must survive until the existing persistence call.
#[derive(Debug, Clone)]
pub(in crate::engine) enum PreparedSerializedEffects {
    Journal {
        scheduled_execution_id: Option<String>,
    },
    Direct {
        record: TenantEventRecord,
        writes: Vec<ResolvedWrite>,
        scheduled_execution_id: Option<String>,
    },
    ExecutionUnit {
        record: Option<TenantEventRecord>,
        schedule_ops: Vec<ResolvedScheduleOp>,
        deferred_server_timestamp_fields: HashMap<(TableName, DocumentId), HashSet<String>>,
    },
}

/// Engine-owned representation of a mutation after planning and before persistence.
#[derive(Debug, Clone)]
pub(crate) struct PreparedCommit {
    /// Sequence context observed while preparing the mutation. Every path uses the opened
    /// snapshot's applied sequence as its OCC pin.
    pub(crate) snapshot_sequence: SequenceNumber,
    /// Dependencies used for assign-time conflict detection against committed and assigned,
    /// unpublished writes in the in-memory window.
    pub(crate) read_set: DependencySet,
    /// Engine-visible write intents. Paths A and C are fully populated, including stable table
    /// identity and both document images. Path B is completed by the execution-unit prepare.
    pub(in crate::engine) write_set: Vec<PreparedWrite>,
    /// Index work selected during prepare. Storage materializes the concrete old/new keys while
    /// atomically applying the already-resolved record.
    pub(in crate::engine) index_deltas: Vec<PreparedIndexDelta>,
    /// Persistence-ready path-specific effects. Path A carries a fully serialized placeholder
    /// record and the scheduled-execution marker. Path B is completed by the execution-unit
    /// prepare. Path C preserves the inputs used to serialize its journal record.
    pub(in crate::engine) serialized_effects: PreparedSerializedEffects,
    /// Resource usage frozen during prepare and checked before sequence assignment.
    pub(in crate::engine) usage: MutationUsage,
}

impl PreparedCommit {
    pub(crate) fn accounted_bytes(&self) -> u64 {
        self.usage
            .total_write_bytes()
            .saturating_add(u64::try_from(std::mem::size_of_val(self)).unwrap_or(u64::MAX))
    }

    pub(crate) fn scheduled_execution_id(&self) -> Option<&str> {
        match &self.serialized_effects {
            PreparedSerializedEffects::Journal {
                scheduled_execution_id,
            } => scheduled_execution_id.as_deref(),
            _ => None,
        }
    }

    pub(crate) fn is_empty_journal(&self) -> bool {
        self.write_set.is_empty()
            && matches!(
                self.serialized_effects,
                PreparedSerializedEffects::Journal { .. }
            )
    }

    pub(crate) fn for_journal(
        snapshot_sequence: SequenceNumber,
        writes: Vec<WriteOp>,
        scheduled_execution_id: Option<String>,
    ) -> Self {
        let mut usage = MutationUsage::default();
        for write in &writes {
            usage.add_user_write(write);
        }
        if let Some(execution_id) = scheduled_execution_id.as_ref() {
            usage.add_system_write(execution_id);
        }
        Self {
            snapshot_sequence,
            read_set: DependencySet::default(),
            write_set: writes
                .into_iter()
                .map(PreparedWrite::from_complete)
                .collect(),
            index_deltas: Vec::new(),
            serialized_effects: PreparedSerializedEffects::Journal {
                scheduled_execution_id,
            },
            usage,
        }
    }

    pub(in crate::engine) fn for_direct(
        snapshot_sequence: SequenceNumber,
        read_set: DependencySet,
        write: WriteOp,
        indexes: Vec<IndexDefinition>,
        scheduled_execution_id: Option<&str>,
    ) -> Result<Self> {
        let prepared_write = PreparedWrite::from_complete(write.clone());
        let mut usage = MutationUsage::default();
        add_prepared_write_usage(&mut usage, &prepared_write);
        if let Some(execution_id) = scheduled_execution_id {
            usage.add_system_write(&execution_id);
        }
        let resolved_write = match (&write.previous, &write.current) {
            (None, Some(document)) => ResolvedWrite::Insert {
                document: document.clone(),
                indexes: indexes.clone(),
                resource_path_binding: write.resource_path_binding.clone(),
            },
            (Some(previous), Some(current)) => ResolvedWrite::Update {
                previous: previous.clone(),
                current: current.clone(),
                indexes: indexes.clone(),
                resource_path_binding: write.resource_path_binding.clone(),
            },
            (Some(previous), None) => ResolvedWrite::Delete {
                previous: previous.clone(),
                indexes: indexes.clone(),
            },
            (None, None) => {
                return Err(Error::Internal(
                    "direct prepared write must include a previous or current document".to_string(),
                ));
            }
        };
        let record = TenantEventRecord::new(
            SequenceNumber(0),
            Timestamp(0),
            vec![write],
            scheduled_execution_id.map(str::to_string),
        )?;
        Ok(Self {
            snapshot_sequence,
            read_set,
            write_set: vec![prepared_write.clone()],
            index_deltas: indexes
                .into_iter()
                .map(|index| PreparedIndexDelta {
                    table: prepared_write.table.clone(),
                    doc_id: prepared_write.doc_id.clone(),
                    index,
                })
                .collect(),
            serialized_effects: PreparedSerializedEffects::Direct {
                record,
                writes: vec![resolved_write],
                scheduled_execution_id: scheduled_execution_id.map(str::to_string),
            },
            usage,
        })
    }

    pub(in crate::engine) fn for_execution_unit(
        snapshot_sequence: SequenceNumber,
        read_set: DependencySet,
        writes: Vec<ResolvedWrite>,
        record: Option<TenantEventRecord>,
        schedule_ops: Vec<ResolvedScheduleOp>,
        deferred_server_timestamp_fields: HashMap<(TableName, DocumentId), HashSet<String>>,
        mut usage: MutationUsage,
    ) -> Result<Self> {
        for write in &writes {
            add_resolved_write_usage(&mut usage, write);
        }
        for schedule_op in &schedule_ops {
            match schedule_op {
                ResolvedScheduleOp::Insert { job } => usage.add_system_write(job),
                ResolvedScheduleOp::Cancel { job_id } => usage.add_system_write(job_id),
            }
        }
        let write_set: Vec<PreparedWrite> = record
            .as_ref()
            .map(|record| {
                record
                    .writes
                    .iter()
                    .cloned()
                    .map(PreparedWrite::from_complete)
                    .collect()
            })
            .unwrap_or_default();
        if write_set.len() != writes.len() {
            return Err(Error::Internal(
                "execution-unit prepared record diverged from resolved writes".to_string(),
            ));
        }
        let index_deltas = writes
            .iter()
            .flat_map(prepared_index_deltas_for_resolved)
            .collect();
        let prepared = Self {
            snapshot_sequence,
            read_set,
            write_set,
            index_deltas,
            // Scheduler operations remain transaction-level side effects. Record-bearing units
            // carry their complete placeholder journal record; schedule-only units deliberately
            // carry `None` because the durable contract emits no TenantEventRecord for them.
            serialized_effects: PreparedSerializedEffects::ExecutionUnit {
                record,
                schedule_ops,
                deferred_server_timestamp_fields,
            },
            usage,
        };
        debug_assert!(prepared.index_deltas.iter().all(|delta| {
            prepared
                .write_set
                .iter()
                .any(|write| write.table == delta.table && write.doc_id == delta.doc_id)
        }));
        Ok(prepared)
    }

    pub(crate) fn into_record(
        mut self,
        sequence: SequenceNumber,
        timestamp: Timestamp,
    ) -> Result<TenantEventRecord> {
        self.stamp_for_assignment(sequence, timestamp)?;
        let PreparedSerializedEffects::Journal {
            scheduled_execution_id,
        } = self.serialized_effects
        else {
            return Err(Error::Internal(
                "only a journal prepared commit can become a tenant event record".to_string(),
            ));
        };
        let writes = self
            .write_set
            .into_iter()
            .map(PreparedWrite::into_complete)
            .collect::<Result<Vec<_>>>()?;
        TenantEventRecord::new(sequence, timestamp, writes, scheduled_execution_id)
    }

    /// Applies the one authoritative lifecycle timestamp after validation and while the
    /// tenant committer owns assignment. Persisted/replayed images consume these values verbatim.
    pub(in crate::engine) fn stamp_for_assignment(
        &mut self,
        sequence: SequenceNumber,
        timestamp: Timestamp,
    ) -> Result<()> {
        for write in &mut self.write_set {
            stamp_prepared_write(write, timestamp);
        }
        if let PreparedSerializedEffects::ExecutionUnit {
            record,
            schedule_ops,
            deferred_server_timestamp_fields,
            ..
        } = &mut self.serialized_effects
        {
            if let Some(record) = record {
                for write in &mut record.writes {
                    let fields = deferred_server_timestamp_fields
                        .get(&(write.table.clone(), write.doc_id.clone()))
                        .cloned();
                    if let Some(fields) = fields
                        && let Some(current) = write.current.as_mut()
                    {
                        for field in fields {
                            current.set_typed_field(
                                field,
                                nimbus_core::TypedScalarValue::Timestamp { value: timestamp },
                            );
                        }
                    }
                }
                // Lifecycle/server timestamps, sequence, and the integrity hash
                // are the only record fields that depend on assignment. The event
                // and document shape was serialized during caller-side prepare.
                record.assign_prepared_document_record(sequence, timestamp)?;
            }
            for schedule_op in schedule_ops {
                if let ResolvedScheduleOp::Insert { job } = schedule_op {
                    job.created_at = timestamp;
                }
            }
        }
        if let PreparedSerializedEffects::Direct { record, writes, .. } =
            &mut self.serialized_effects
        {
            for write in writes {
                stamp_resolved_write(write, timestamp);
            }
            record.assign_prepared_document_record(sequence, timestamp)?;
        }
        Ok(())
    }

    pub(in crate::engine) fn direct_effects(
        &self,
    ) -> Result<(&TenantEventRecord, &[ResolvedWrite], Option<&str>)> {
        match &self.serialized_effects {
            PreparedSerializedEffects::Direct {
                record,
                writes,
                scheduled_execution_id,
            } => Ok((record, writes, scheduled_execution_id.as_deref())),
            _ => Err(Error::Internal(
                "direct prepared commit has an invalid effect shape".to_string(),
            )),
        }
    }

    pub(in crate::engine) fn execution_unit_effects(
        &self,
    ) -> Result<(Option<&TenantEventRecord>, &[ResolvedScheduleOp])> {
        match &self.serialized_effects {
            PreparedSerializedEffects::ExecutionUnit {
                record,
                schedule_ops,
                ..
            } => Ok((record.as_ref(), schedule_ops)),
            _ => Err(Error::Internal(
                "execution-unit prepared commit has an invalid effect shape".to_string(),
            )),
        }
    }

    pub(in crate::engine) fn is_empty_execution_unit(&self) -> bool {
        matches!(
            &self.serialized_effects,
            PreparedSerializedEffects::ExecutionUnit {
                record,
                schedule_ops,
                ..
            } if record.is_none() && schedule_ops.is_empty()
        )
    }

    pub(in crate::engine) fn has_scheduled_insert(&self) -> bool {
        matches!(
            &self.serialized_effects,
            PreparedSerializedEffects::ExecutionUnit { schedule_ops, .. }
                if schedule_ops
                    .iter()
                    .any(|operation| matches!(operation, ResolvedScheduleOp::Insert { .. }))
        )
    }

    pub(in crate::engine) fn usage(&self) -> MutationUsage {
        self.usage
    }
}

fn add_prepared_write_usage(usage: &mut MutationUsage, write: &PreparedWrite) {
    match &write.current {
        Some(PreparedDocument::Full(document)) => usage.add_user_write(document),
        None => usage.add_user_write(&(write.table.as_str(), write.doc_id.as_str(), write.op_type)),
    }
}

fn add_resolved_write_usage(usage: &mut MutationUsage, write: &ResolvedWrite) {
    match write {
        ResolvedWrite::Insert { document, .. } => usage.add_user_write(document),
        ResolvedWrite::Update { current, .. } => usage.add_user_write(current),
        ResolvedWrite::Delete { previous, .. } => usage.add_user_write(&(
            previous.table.as_str(),
            previous.id.as_str(),
            WriteOpType::Delete,
        )),
    }
}

fn stamp_prepared_write(write: &mut PreparedWrite, timestamp: Timestamp) {
    let Some(PreparedDocument::Full(current)) = write.current.as_mut() else {
        return;
    };
    match write.op_type {
        WriteOpType::Insert => {
            current.creation_time = timestamp;
            current.update_time = timestamp;
        }
        WriteOpType::Update => {
            if let Some(previous) = write.previous.as_ref() {
                current.creation_time = previous.creation_time;
            }
            current.update_time = timestamp;
        }
        WriteOpType::Delete => {}
    }
}

fn stamp_resolved_write(write: &mut ResolvedWrite, timestamp: Timestamp) {
    match write {
        ResolvedWrite::Insert { document, .. } => {
            document.creation_time = timestamp;
            document.update_time = timestamp;
        }
        ResolvedWrite::Update {
            previous, current, ..
        } => {
            current.creation_time = previous.creation_time;
            current.update_time = timestamp;
        }
        ResolvedWrite::Delete { .. } => {}
    }
}

fn prepared_index_deltas_for_resolved(
    write: &ResolvedWrite,
) -> impl Iterator<Item = PreparedIndexDelta> + '_ {
    let (table, doc_id, indexes) = match write {
        ResolvedWrite::Insert {
            document, indexes, ..
        } => (&document.table, &document.id, indexes),
        ResolvedWrite::Update {
            current, indexes, ..
        } => (&current.table, &current.id, indexes),
        ResolvedWrite::Delete { previous, indexes } => (&previous.table, &previous.id, indexes),
    };
    indexes.iter().cloned().map(|index| PreparedIndexDelta {
        table: table.clone(),
        doc_id: doc_id.clone(),
        index,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use nimbus_core::{IndexId, IndexState, Mutation, ScheduledJob, TenantEventKind};
    use serde_json::json;

    use super::*;

    fn table() -> TableName {
        TableName::new("prepared_tasks").expect("table should parse")
    }

    fn document(id: &str, value: &str) -> Document {
        Document {
            id: DocumentId::from_key(id).expect("document id should parse"),
            table: table(),
            creation_time: Timestamp(100),
            update_time: Timestamp(101),
            fields: serde_json::Map::from_iter([("value".to_string(), json!(value))]),
            typed_fields: BTreeMap::new(),
        }
    }

    fn table_id() -> TableId {
        TableId::try_from("prepared-table-id".to_string()).expect("table id should parse")
    }

    fn index() -> IndexDefinition {
        IndexDefinition {
            id: IndexId::try_from("prepared-index-id".to_string()).expect("index id should parse"),
            name: "by_value".to_string(),
            fields: vec!["value".to_string()],
            state: IndexState::Enabled,
        }
    }

    #[test]
    fn journal_prepared_commit_captures_complete_write_and_empty_conflict_metadata() {
        let current = document("journal-doc", "current");
        let write = WriteOp {
            table: table(),
            table_id: table_id(),
            op_type: WriteOpType::Insert,
            doc_id: current.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(current.clone()),
        };

        let prepared = PreparedCommit::for_journal(
            SequenceNumber(40),
            vec![write],
            Some("scheduled-40".to_string()),
        );

        assert_eq!(prepared.snapshot_sequence, SequenceNumber(40));
        assert!(prepared.read_set.is_empty());
        assert!(prepared.index_deltas.is_empty());
        assert_eq!(prepared.write_set.len(), 1);
        assert_eq!(prepared.usage.documents_written, 1);
        assert!(prepared.usage.write_bytes > 0);
        assert_eq!(prepared.usage.system_documents_written, 1);
        assert_eq!(prepared.write_set[0].table_id, Some(table_id()));
        assert_eq!(
            prepared.write_set[0].current,
            Some(PreparedDocument::Full(current))
        );
        assert!(matches!(
            prepared.serialized_effects,
            PreparedSerializedEffects::Journal {
                scheduled_execution_id: Some(ref execution_id)
            } if execution_id == "scheduled-40"
        ));
    }

    #[test]
    fn direct_prepared_commit_captures_full_serialized_record_and_index_work() {
        let insert_document = document("direct-insert", "insert");
        let write = WriteOp {
            table: table(),
            table_id: table_id(),
            op_type: WriteOpType::Insert,
            doc_id: insert_document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(insert_document.clone()),
        };
        let mut dependencies = DependencySet::default();
        dependencies.record_document(&table(), &table_id(), insert_document.id.clone());
        let mut insert = PreparedCommit::for_direct(
            SequenceNumber(50),
            dependencies.clone(),
            write,
            vec![index()],
            Some("scheduled-direct"),
        )
        .expect("direct prepare should serialize");

        assert_eq!(insert.write_set[0].table_id, Some(table_id()));
        assert_eq!(insert.write_set[0].previous, None);
        assert_eq!(insert.read_set, dependencies);
        assert_eq!(insert.index_deltas.len(), 1);
        assert_eq!(insert.usage.documents_written, 1);
        assert!(insert.usage.write_bytes > 0);
        assert_eq!(insert.usage.system_documents_written, 1);
        assert!(insert.usage.system_write_bytes > 0);
        let (placeholder, _, scheduled) = insert.direct_effects().unwrap();
        assert_eq!(placeholder.sequence, SequenceNumber(0));
        assert_eq!(placeholder.timestamp, Timestamp(0));
        assert_eq!(scheduled, Some("scheduled-direct"));

        insert
            .stamp_for_assignment(SequenceNumber(51), Timestamp(500))
            .expect("assignment should stamp the placeholder record");
        let (record, _, _) = insert.direct_effects().unwrap();
        assert_eq!(record.sequence, SequenceNumber(51));
        assert_eq!(record.timestamp, Timestamp(500));
        assert_eq!(
            record.writes[0].current.as_ref().unwrap().update_time,
            Timestamp(500)
        );
        record.validate_integrity().unwrap();
    }

    #[test]
    fn execution_unit_prepared_commit_preserves_dependencies_indexes_and_effects() {
        let previous = document("execution-unit-doc", "previous");
        let mut current = previous.clone();
        current.fields.insert("value".to_string(), json!("current"));
        let mut read_set = DependencySet::default();
        read_set.record_document(&table(), &table_id(), previous.id.clone());
        let scheduled_job = ScheduledJob {
            id: DocumentId::from_key("scheduled-job").expect("job id should parse"),
            run_at: Timestamp(500),
            mutation: Mutation::Delete {
                table: table(),
                id: previous.id.clone(),
            },
            created_at: Timestamp(400),
        };
        let resolved_write = ResolvedWrite::Update {
            previous: previous.clone(),
            current: current.clone(),
            indexes: vec![index()],
            resource_path_binding: None,
        };
        let record = TenantEventRecord::new(
            SequenceNumber(0),
            Timestamp(0),
            vec![WriteOp {
                table: table(),
                table_id: table_id(),
                op_type: WriteOpType::Update,
                doc_id: previous.id.clone(),
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: Some(previous.clone()),
                current: Some(current.clone()),
            }],
            None,
        )
        .expect("execution-unit record should serialize during prepare");

        let read_usage = MutationUsage {
            read_bytes: 123,
            documents_scanned: 4,
            index_range_calls: 2,
            ..MutationUsage::default()
        };
        let prepared = PreparedCommit::for_execution_unit(
            SequenceNumber(60),
            read_set.clone(),
            vec![resolved_write],
            Some(record),
            vec![ResolvedScheduleOp::Insert {
                job: scheduled_job.clone(),
            }],
            HashMap::new(),
            read_usage,
        )
        .expect("execution-unit prepare should succeed");

        assert_eq!(prepared.snapshot_sequence, SequenceNumber(60));
        assert_eq!(prepared.read_set, read_set);
        assert_eq!(prepared.write_set.len(), 1);
        assert_eq!(prepared.write_set[0].previous, Some(previous));
        assert_eq!(
            prepared.write_set[0].current,
            Some(PreparedDocument::Full(current))
        );
        assert_eq!(
            prepared.index_deltas,
            vec![PreparedIndexDelta {
                table: table(),
                doc_id: DocumentId::from_key("execution-unit-doc")
                    .expect("document id should parse"),
                index: index(),
            }]
        );
        let (record, schedule_ops) = prepared.execution_unit_effects().unwrap();
        let record = record.expect("document unit should carry a prepared record");
        assert_eq!(record.sequence, SequenceNumber(0));
        assert_eq!(record.timestamp, Timestamp(0));
        assert_eq!(record.writes.len(), 1);
        assert!(matches!(
            schedule_ops,
            [ResolvedScheduleOp::Insert { job }] if job == &scheduled_job
        ));
        assert!(prepared.has_scheduled_insert());
        assert_eq!(prepared.usage.read_bytes, 123);
        assert_eq!(prepared.usage.documents_scanned, 4);
        assert_eq!(prepared.usage.index_range_calls, 2);
        assert_eq!(prepared.usage.documents_written, 1);
        assert!(prepared.usage.write_bytes > 0);
        assert_eq!(prepared.usage.system_documents_written, 1);
        assert!(prepared.usage.system_write_bytes > 0);

        let mut assigned = prepared.clone();
        assigned
            .stamp_for_assignment(SequenceNumber(61), Timestamp(600))
            .expect("assignment stamping should succeed");
        let (assigned_record, assigned_schedule_ops) = assigned
            .execution_unit_effects()
            .expect("assigned execution-unit effects should remain available");
        let assigned_record = assigned_record.expect("assigned record should remain available");
        assert_eq!(assigned_record.sequence, SequenceNumber(61));
        assert_eq!(assigned_record.timestamp, Timestamp(600));
        assert_eq!(
            assigned_record.writes[0]
                .current
                .as_ref()
                .expect("update should retain current image")
                .update_time,
            Timestamp(600)
        );
        assigned_record.validate_integrity().unwrap();
        assert!(matches!(
            assigned_schedule_ops,
            [ResolvedScheduleOp::Insert { job }] if job.created_at == Timestamp(600)
        ));
    }

    #[test]
    fn schedule_only_execution_unit_has_no_prepared_record_by_contract() {
        let job = ScheduledJob {
            id: DocumentId::from_key("schedule-only-job").expect("job id should parse"),
            run_at: Timestamp(500),
            mutation: Mutation::Delete {
                table: table(),
                id: DocumentId::from_key("schedule-only-target").expect("document id should parse"),
            },
            created_at: Timestamp(0),
        };
        let mut prepared = PreparedCommit::for_execution_unit(
            SequenceNumber(60),
            DependencySet::default(),
            vec![],
            None,
            vec![ResolvedScheduleOp::Insert { job }],
            HashMap::new(),
            MutationUsage::default(),
        )
        .expect("schedule-only prepare should succeed");

        let (record, schedule_ops) = prepared.execution_unit_effects().unwrap();
        assert!(record.is_none());
        assert_eq!(schedule_ops.len(), 1);
        assert!(!prepared.is_empty_execution_unit());

        prepared
            .stamp_for_assignment(SequenceNumber(61), Timestamp(600))
            .expect("schedule-only assignment should stamp its job");
        let (record, schedule_ops) = prepared.execution_unit_effects().unwrap();
        assert!(record.is_none());
        assert!(matches!(
            schedule_ops,
            [ResolvedScheduleOp::Insert { job }] if job.created_at == Timestamp(600)
        ));
    }

    #[test]
    fn into_record_matches_the_former_journal_record_shape() {
        let current = document("record-doc", "current");
        let writes = vec![WriteOp {
            table: table(),
            table_id: table_id(),
            op_type: WriteOpType::Insert,
            doc_id: current.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(current),
        }];
        let sequence = SequenceNumber(70);
        let timestamp = Timestamp(700);
        let scheduled_execution_id = Some("scheduled-70".to_string());

        let record = PreparedCommit::for_journal(
            SequenceNumber(69),
            writes.clone(),
            scheduled_execution_id.clone(),
        )
        .into_record(sequence, timestamp)
        .expect("journal prepared commit should become a record");
        let mut stamped_writes = writes.clone();
        let stamped_document = stamped_writes[0]
            .current
            .as_mut()
            .expect("insert should carry its current document");
        stamped_document.creation_time = timestamp;
        stamped_document.update_time = timestamp;
        let former_shape = TenantEventRecord::compatibility_document_record(
            sequence,
            timestamp,
            stamped_writes.clone(),
            scheduled_execution_id.clone(),
        )
        .expect("former journal record shape should construct");

        assert_eq!(record, former_shape);
        assert_eq!(record.sequence, sequence);
        assert_eq!(record.timestamp, timestamp);
        assert_eq!(record.writes, stamped_writes);
        assert_eq!(record.scheduled_execution_id, scheduled_execution_id);
        assert!(matches!(
            record.events.as_slice(),
            [TenantEventKind::DocumentWrite { .. }, TenantEventKind::ScheduledExecution { execution_id }]
                if execution_id == "scheduled-70"
        ));
        record
            .validate_integrity()
            .expect("prepared record integrity should validate");
    }
}
