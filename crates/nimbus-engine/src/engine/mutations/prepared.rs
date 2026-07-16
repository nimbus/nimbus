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
    Patch(serde_json::Map<String, serde_json::Value>),
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
            Some(PreparedDocument::Patch(_)) => {
                return Err(Error::Internal(
                    "journal prepared write must contain a full current document".to_string(),
                ));
            }
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
    Direct,
    ExecutionUnit {
        writes: Vec<ResolvedWrite>,
        schedule_ops: Vec<ResolvedScheduleOp>,
        trigger_write_origin: Option<TriggerWriteOrigin>,
        deferred_server_timestamp_fields: HashMap<(TableName, DocumentId), HashSet<String>>,
    },
}

/// Engine-owned representation of a mutation after planning and before persistence.
#[derive(Debug, Clone)]
pub(in crate::engine) struct PreparedCommit {
    /// Sequence context observed while preparing the mutation. Path A records the durable
    /// head under its exclusive sequence guard and path B records the durable head before its
    /// lock-serialized typed call; neither uses the value for OCC. Path C is fully populated
    /// with the applied sequence of the opened read snapshot and uses it as the OCC pin.
    pub(in crate::engine) snapshot_sequence: SequenceNumber,
    /// Dependencies used for assign-time conflict detection. This is empty on paths A and B:
    /// both serialize through the per-tenant sequence lock, and path A additionally plans each
    /// batch in strict order against an overlay. Path C is fully populated with its read and
    /// write dependencies.
    pub(in crate::engine) read_set: DependencySet,
    /// Engine-visible write intents. Path A is fully populated, including stable table identity
    /// and both document images. Path B is intentionally sparse: storage resolves table identity
    /// and the previous image under its lock, while an update's current value is the incoming
    /// patch. Path C has full previous/current images but leaves table identity absent because
    /// the existing execution-unit storage call resolves it atomically.
    pub(in crate::engine) write_set: Vec<PreparedWrite>,
    /// Index work known before persistence. Paths A and B leave this empty because their existing
    /// storage paths resolve index effects under the storage lock. Path C partially populates it
    /// with each affected index definition; storage still materializes the concrete old/new keys.
    pub(in crate::engine) index_deltas: Vec<PreparedIndexDelta>,
    /// Persistence-ready path-specific effects. Path A carries the scheduled-execution marker
    /// alongside the fully populated `write_set`; path B has no serialized engine payload because
    /// its typed storage method owns validation and serialization; path C fully preserves the
    /// resolved writes, scheduler operations, and trigger origin consumed by its unchanged atomic
    /// storage call.
    pub(in crate::engine) serialized_effects: PreparedSerializedEffects,
    /// Resource usage frozen during prepare and checked before sequence assignment.
    pub(in crate::engine) usage: MutationUsage,
}

impl PreparedCommit {
    pub(in crate::engine) fn for_journal(
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

    pub(in crate::engine) fn for_direct_insert(
        snapshot_sequence: SequenceNumber,
        document: Document,
        scheduled_execution_id: Option<&str>,
    ) -> Self {
        let write = PreparedWrite {
            table: document.table.clone(),
            table_id: None,
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: None,
            current: Some(PreparedDocument::Full(document)),
        };
        Self::for_direct(snapshot_sequence, write, scheduled_execution_id)
    }

    pub(in crate::engine) fn for_direct_update(
        snapshot_sequence: SequenceNumber,
        table: TableName,
        doc_id: DocumentId,
        patch: serde_json::Map<String, serde_json::Value>,
        scheduled_execution_id: Option<&str>,
    ) -> Self {
        Self::for_direct(
            snapshot_sequence,
            PreparedWrite {
                table,
                table_id: None,
                op_type: WriteOpType::Update,
                doc_id,
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: Some(PreparedDocument::Patch(patch)),
            },
            scheduled_execution_id,
        )
    }

    pub(in crate::engine) fn for_direct_delete(
        snapshot_sequence: SequenceNumber,
        table: TableName,
        doc_id: DocumentId,
        scheduled_execution_id: Option<&str>,
    ) -> Self {
        Self::for_direct(
            snapshot_sequence,
            PreparedWrite {
                table,
                table_id: None,
                op_type: WriteOpType::Delete,
                doc_id,
                resource_path_binding: None,
                trigger_write_origin: None,
                previous: None,
                current: None,
            },
            scheduled_execution_id,
        )
    }

    fn for_direct(
        snapshot_sequence: SequenceNumber,
        write: PreparedWrite,
        scheduled_execution_id: Option<&str>,
    ) -> Self {
        let mut usage = MutationUsage::default();
        add_prepared_write_usage(&mut usage, &write);
        if let Some(execution_id) = scheduled_execution_id {
            usage.add_system_write(&execution_id);
        }
        Self {
            snapshot_sequence,
            read_set: DependencySet::default(),
            write_set: vec![write],
            index_deltas: Vec::new(),
            serialized_effects: PreparedSerializedEffects::Direct,
            usage,
        }
    }

    pub(in crate::engine) fn for_execution_unit(
        snapshot_sequence: SequenceNumber,
        read_set: DependencySet,
        writes: Vec<ResolvedWrite>,
        schedule_ops: Vec<ResolvedScheduleOp>,
        trigger_write_origin: Option<TriggerWriteOrigin>,
        deferred_server_timestamp_fields: HashMap<(TableName, DocumentId), HashSet<String>>,
        mut usage: MutationUsage,
    ) -> Self {
        for write in &writes {
            add_resolved_write_usage(&mut usage, write);
        }
        for schedule_op in &schedule_ops {
            match schedule_op {
                ResolvedScheduleOp::Insert { job } => usage.add_system_write(job),
                ResolvedScheduleOp::Cancel { job_id } => usage.add_system_write(job_id),
            }
        }
        let write_set = writes
            .iter()
            .map(|write| prepared_write_for_resolved(write, trigger_write_origin.as_ref()))
            .collect();
        let index_deltas = writes
            .iter()
            .flat_map(prepared_index_deltas_for_resolved)
            .collect();
        let prepared = Self {
            snapshot_sequence,
            read_set,
            write_set,
            index_deltas,
            // Scheduler operations and trigger origin are transaction-level side effects rather
            // than document writes, so they ride with the exact storage payload instead of being
            // forced into `write_set` and losing their established semantics.
            serialized_effects: PreparedSerializedEffects::ExecutionUnit {
                writes,
                schedule_ops,
                trigger_write_origin,
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
        prepared
    }

    pub(in crate::engine) fn into_record(
        mut self,
        sequence: SequenceNumber,
        timestamp: Timestamp,
    ) -> Result<TenantEventRecord> {
        self.stamp_for_assignment(timestamp)?;
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
    /// tenant sequence gate is held. Persisted/replayed images consume these values verbatim.
    pub(in crate::engine) fn stamp_for_assignment(&mut self, timestamp: Timestamp) -> Result<()> {
        for write in &mut self.write_set {
            stamp_prepared_write(write, timestamp);
        }
        if let PreparedSerializedEffects::ExecutionUnit {
            writes,
            schedule_ops,
            deferred_server_timestamp_fields,
            ..
        } = &mut self.serialized_effects
        {
            for write in writes {
                stamp_resolved_write(write, timestamp);
                let fields = resolved_write_key(write)
                    .and_then(|key| deferred_server_timestamp_fields.get(&key).cloned());
                if let Some(fields) = fields
                    && let Some(current) = resolved_write_current(write)
                {
                    for field in fields {
                        current.set_typed_field(
                            field,
                            nimbus_core::TypedScalarValue::Timestamp { value: timestamp },
                        );
                    }
                }
            }
            for schedule_op in schedule_ops {
                if let ResolvedScheduleOp::Insert { job } = schedule_op {
                    job.created_at = timestamp;
                }
            }
        }
        Ok(())
    }

    pub(in crate::engine) fn direct_insert_document(&self) -> Result<&Document> {
        match self.write_set.as_slice() {
            [
                PreparedWrite {
                    op_type: WriteOpType::Insert,
                    current: Some(PreparedDocument::Full(document)),
                    ..
                },
            ] if matches!(self.serialized_effects, PreparedSerializedEffects::Direct) => {
                Ok(document)
            }
            _ => Err(Error::Internal(
                "direct insert prepared commit has an invalid shape".to_string(),
            )),
        }
    }

    pub(in crate::engine) fn direct_update_parts(
        &self,
    ) -> Result<(
        &TableName,
        &DocumentId,
        &serde_json::Map<String, serde_json::Value>,
    )> {
        match self.write_set.as_slice() {
            [
                PreparedWrite {
                    table,
                    op_type: WriteOpType::Update,
                    doc_id,
                    current: Some(PreparedDocument::Patch(patch)),
                    ..
                },
            ] if matches!(self.serialized_effects, PreparedSerializedEffects::Direct) => {
                Ok((table, doc_id, patch))
            }
            _ => Err(Error::Internal(
                "direct update prepared commit has an invalid shape".to_string(),
            )),
        }
    }

    pub(in crate::engine) fn direct_delete_parts(&self) -> Result<(&TableName, &DocumentId)> {
        match self.write_set.as_slice() {
            [
                PreparedWrite {
                    table,
                    op_type: WriteOpType::Delete,
                    doc_id,
                    current: None,
                    ..
                },
            ] if matches!(self.serialized_effects, PreparedSerializedEffects::Direct) => {
                Ok((table, doc_id))
            }
            _ => Err(Error::Internal(
                "direct delete prepared commit has an invalid shape".to_string(),
            )),
        }
    }

    pub(in crate::engine) fn execution_unit_effects(
        &self,
    ) -> Result<(
        &[ResolvedWrite],
        &[ResolvedScheduleOp],
        Option<&TriggerWriteOrigin>,
    )> {
        match &self.serialized_effects {
            PreparedSerializedEffects::ExecutionUnit {
                writes,
                schedule_ops,
                trigger_write_origin,
                ..
            } => Ok((writes, schedule_ops, trigger_write_origin.as_ref())),
            _ => Err(Error::Internal(
                "execution-unit prepared commit has an invalid effect shape".to_string(),
            )),
        }
    }

    pub(in crate::engine) fn is_empty_execution_unit(&self) -> bool {
        matches!(
            &self.serialized_effects,
            PreparedSerializedEffects::ExecutionUnit {
                writes,
                schedule_ops,
                ..
            } if writes.is_empty() && schedule_ops.is_empty()
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
        Some(PreparedDocument::Patch(patch)) => usage.add_user_write(patch),
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

fn resolved_write_key(write: &ResolvedWrite) -> Option<(TableName, DocumentId)> {
    match write {
        ResolvedWrite::Insert { document, .. } => {
            Some((document.table.clone(), document.id.clone()))
        }
        ResolvedWrite::Update { current, .. } => Some((current.table.clone(), current.id.clone())),
        ResolvedWrite::Delete { .. } => None,
    }
}

fn resolved_write_current(write: &mut ResolvedWrite) -> Option<&mut Document> {
    match write {
        ResolvedWrite::Insert { document, .. } => Some(document),
        ResolvedWrite::Update { current, .. } => Some(current),
        ResolvedWrite::Delete { .. } => None,
    }
}

fn prepared_write_for_resolved(
    write: &ResolvedWrite,
    trigger_write_origin: Option<&TriggerWriteOrigin>,
) -> PreparedWrite {
    match write {
        ResolvedWrite::Insert {
            document,
            resource_path_binding,
            ..
        } => PreparedWrite {
            table: document.table.clone(),
            table_id: None,
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: resource_path_binding.clone(),
            trigger_write_origin: trigger_write_origin.cloned(),
            previous: None,
            current: Some(PreparedDocument::Full(document.clone())),
        },
        ResolvedWrite::Update {
            previous,
            current,
            resource_path_binding,
            ..
        } => PreparedWrite {
            table: current.table.clone(),
            table_id: None,
            op_type: WriteOpType::Update,
            doc_id: current.id.clone(),
            resource_path_binding: resource_path_binding.clone(),
            trigger_write_origin: trigger_write_origin.cloned(),
            previous: Some(previous.clone()),
            current: Some(PreparedDocument::Full(current.clone())),
        },
        ResolvedWrite::Delete { previous, .. } => PreparedWrite {
            table: previous.table.clone(),
            table_id: None,
            op_type: WriteOpType::Delete,
            doc_id: previous.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: trigger_write_origin.cloned(),
            previous: Some(previous.clone()),
            current: None,
        },
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
    fn direct_prepared_commits_capture_sparse_insert_update_and_delete_intents() {
        let insert_document = document("direct-insert", "insert");
        let insert = PreparedCommit::for_direct_insert(
            SequenceNumber(50),
            insert_document.clone(),
            Some("scheduled-direct"),
        );
        let patch = serde_json::Map::from_iter([("value".to_string(), json!("updated"))]);
        let update = PreparedCommit::for_direct_update(
            SequenceNumber(51),
            table(),
            DocumentId::from_key("direct-update").expect("document id should parse"),
            patch.clone(),
            None,
        );
        let delete = PreparedCommit::for_direct_delete(
            SequenceNumber(52),
            table(),
            DocumentId::from_key("direct-delete").expect("document id should parse"),
            None,
        );

        assert_eq!(insert.direct_insert_document().unwrap(), &insert_document);
        assert_eq!(insert.write_set[0].table_id, None);
        assert_eq!(insert.write_set[0].previous, None);
        assert!(insert.read_set.is_empty());
        assert!(insert.index_deltas.is_empty());
        assert_eq!(insert.usage.documents_written, 1);
        assert!(insert.usage.write_bytes > 0);
        assert_eq!(insert.usage.system_documents_written, 1);
        assert!(insert.usage.system_write_bytes > 0);

        let (update_table, update_id, update_patch) = update.direct_update_parts().unwrap();
        assert_eq!(update_table, &table());
        assert_eq!(update_id.as_str(), "direct-update");
        assert_eq!(update_patch, &patch);
        assert_eq!(update.write_set[0].previous, None);
        assert!(update.read_set.is_empty());

        let (delete_table, delete_id) = delete.direct_delete_parts().unwrap();
        assert_eq!(delete_table, &table());
        assert_eq!(delete_id.as_str(), "direct-delete");
        assert_eq!(delete.write_set[0].previous, None);
        assert_eq!(delete.write_set[0].current, None);
        assert!(delete.read_set.is_empty());
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
            vec![ResolvedScheduleOp::Insert {
                job: scheduled_job.clone(),
            }],
            None,
            HashMap::new(),
            read_usage,
        );

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
        let (writes, schedule_ops, origin) = prepared.execution_unit_effects().unwrap();
        assert_eq!(writes.len(), 1);
        assert!(matches!(writes[0], ResolvedWrite::Update { .. }));
        assert!(matches!(
            schedule_ops,
            [ResolvedScheduleOp::Insert { job }] if job == &scheduled_job
        ));
        assert_eq!(origin, None);
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
            .stamp_for_assignment(Timestamp(600))
            .expect("assignment stamping should succeed");
        let (_, assigned_schedule_ops, _) = assigned
            .execution_unit_effects()
            .expect("assigned execution-unit effects should remain available");
        assert!(matches!(
            assigned_schedule_ops,
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
