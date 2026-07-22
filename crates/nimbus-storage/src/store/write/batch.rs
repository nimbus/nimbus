use nimbus_core::{CommitEntry, Error, Result, Timestamp};

use super::super::{ResolvedScheduleOp, ResolvedWrite, TenantStore};
use super::scheduled::apply_schedule_ops;

impl TenantStore {
    pub fn apply_prepared_write_batch(
        &self,
        record: &nimbus_core::TenantEventRecord,
        schedule_ops: &[ResolvedScheduleOp],
        scheduled_execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        if record.writes.is_empty() {
            return Err(Error::Internal(
                "prepared write batch must contain at least one document write".to_string(),
            ));
        }
        let committed = self.execute_write(|transaction| {
            if !transaction.begin_scheduled_execution(scheduled_execution_id)? {
                return Ok(false);
            }
            transaction.apply_prepared_record(record)?;
            apply_schedule_ops(transaction.write_txn()?, schedule_ops)?;
            transaction.set_prepared_record(record.clone());
            Ok(true)
        })?;
        Ok(committed.value.then_some(committed.commit).flatten())
    }

    pub fn apply_resolved_write_batch(&self, writes: &[ResolvedWrite]) -> Result<CommitEntry> {
        self.apply_execution_unit_batch(writes, &[])?
            .ok_or_else(|| {
                Error::Internal("resolved write batch must contain at least one write".to_string())
            })
    }

    pub fn apply_execution_unit_batch(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
    ) -> Result<Option<CommitEntry>> {
        self.apply_execution_unit_batch_with_origin(writes, schedule_ops, None, None)
    }

    pub fn apply_execution_unit_batch_with_origin(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
        trigger_write_origin: Option<&nimbus_core::TriggerWriteOrigin>,
        commit_timestamp: Option<Timestamp>,
    ) -> Result<Option<CommitEntry>> {
        if writes.is_empty() && schedule_ops.is_empty() {
            return Err(Error::Internal(
                "execution-unit batch must contain at least one change".to_string(),
            ));
        }

        let committed =
            self.execute_write_with_commit_timestamp(commit_timestamp, |transaction| {
                for write in writes {
                    match write {
                        ResolvedWrite::Insert {
                            document,
                            indexes,
                            resource_path_binding,
                        } => transaction.apply_document_insert(
                            document,
                            indexes,
                            resource_path_binding.as_ref(),
                            trigger_write_origin,
                        )?,
                        ResolvedWrite::Update {
                            previous,
                            current,
                            indexes,
                            resource_path_binding,
                        } => transaction.apply_batch_document_update(
                            previous,
                            current,
                            indexes,
                            resource_path_binding.as_ref(),
                            trigger_write_origin,
                        )?,
                        ResolvedWrite::Delete { previous, indexes } => transaction
                            .apply_batch_document_delete(previous, indexes, trigger_write_origin)?,
                    }
                }

                apply_schedule_ops(transaction.write_txn()?, schedule_ops)?;
                Ok(())
            })?;
        Ok(committed.commit)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nimbus_core::{
        Document, DocumentId, DocumentLocator, DocumentPath, FieldSchema, FieldType,
        IndexDefinition, ManualWallClock, ResourcePathBinding, SequenceNumber, TableName,
        TableSchema, Timestamp, WriteOp, WriteOpType,
    };
    use serde_json::json;

    use crate::simulation::{
        FaultOccurrence, FaultPoint, NoopFaultInjector, ScriptedFaultInjector,
    };
    use crate::store::INDEXES;

    use super::*;

    fn schema(table: &TableName) -> TableSchema {
        TableSchema {
            table: table.clone(),
            fields: vec![
                FieldSchema {
                    name: "owner".to_string(),
                    field_type: FieldType::String,
                    required: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    field_type: FieldType::String,
                    required: true,
                },
            ],
            indexes: vec![IndexDefinition {
                id: nimbus_core::IndexId::new(),
                state: nimbus_core::IndexState::Enabled,
                name: "by_body".to_string(),
                fields: vec!["body".to_string()],
            }],
            access_policy: None,
        }
    }

    fn document(table: &TableName, id: &str, body: &str) -> Document {
        Document::with_id(
            DocumentId::from_key(id).expect("id should parse"),
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!(body)),
            ]),
        )
    }

    fn fixed_document(table: &TableName, id: &str, body: &str, timestamp: Timestamp) -> Document {
        let mut document = document(table, id, body);
        document.creation_time = timestamp;
        document.update_time = timestamp;
        document
    }

    #[derive(Debug, PartialEq)]
    struct WriteOpShape {
        table: TableName,
        op_type: WriteOpType,
        doc_id: DocumentId,
        has_table_id: bool,
        has_resource_path_binding: bool,
        has_trigger_write_origin: bool,
        previous: Option<Document>,
        current: Option<Document>,
    }

    fn write_op_shape(write: &WriteOp) -> WriteOpShape {
        WriteOpShape {
            table: write.table.clone(),
            op_type: write.op_type,
            doc_id: write.doc_id.clone(),
            has_table_id: !write.table_id.as_str().is_empty(),
            has_resource_path_binding: write.resource_path_binding.is_some(),
            has_trigger_write_origin: write.trigger_write_origin.is_some(),
            previous: write.previous.clone(),
            current: write.current.clone(),
        }
    }

    fn only_write(commit: &CommitEntry) -> &WriteOp {
        assert_eq!(
            commit.writes.len(),
            1,
            "test commits should contain exactly one document write"
        );
        &commit.writes[0]
    }

    #[test]
    fn failed_batch_rolls_back_document_indexes_bindings_and_commit_log() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let table = TableName::new("tasks_atomic_batch").expect("table should parse");
        let schema = schema(&table);
        store
            .replace_table_schema(&schema)
            .expect("schema should persist");

        let existing = document(&table, "existing", "existing");
        store
            .insert_with_indexes(&existing, &schema.indexes)
            .expect("seed document should insert");

        let pending = document(&table, "pending", "alpha");
        let binding = ResourcePathBinding::new(
            DocumentLocator::new(table.clone(), pending.id.clone()),
            DocumentPath::from_segments(["cities", "SF"]).expect("path should parse"),
        );
        let failed = store
            .apply_execution_unit_batch(
                &[
                    ResolvedWrite::Insert {
                        document: pending.clone(),
                        indexes: schema.indexes.clone(),
                        resource_path_binding: Some(binding.clone()),
                    },
                    ResolvedWrite::Insert {
                        document: existing.clone(),
                        indexes: schema.indexes.clone(),
                        resource_path_binding: None,
                    },
                ],
                &[],
            )
            .expect_err("conflicting sibling write should fail the batch");

        assert!(matches!(failed, Error::Conflict { .. }));
        assert!(
            store
                .get(&table, &pending.id)
                .expect("document lookup should succeed")
                .is_none(),
            "failed batch must not leave the document behind"
        );
        assert!(
            store
                .index_scan_eq(&table, "by_body", &json!("alpha"))
                .expect("index scan should succeed")
                .is_empty(),
            "failed batch must not leave index entries behind"
        );
        assert!(
            store
                .resource_path_binding(&binding.locator)
                .expect("binding lookup should succeed")
                .is_none(),
            "failed batch must not leave path metadata behind"
        );
        assert_eq!(
            store
                .latest_sequence()
                .expect("latest sequence should remain readable"),
            SequenceNumber(2),
            "failed batch must not append a commit log entry"
        );
    }

    #[test]
    fn failed_point_delete_rolls_back_document_indexes_bindings_and_commit_log() {
        let clock = Arc::new(ManualWallClock::new(Timestamp(10_000)));
        let faults = Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
            point: FaultPoint::StorageCommitBeforeVisibility,
            visit: 3,
        }]));
        let store = TenantStore::create_in_memory_with_simulation(clock, faults)
            .expect("store should open");
        let table = TableName::new("tasks_atomic_point").expect("table should parse");
        let schema = schema(&table);
        store
            .replace_table_schema(&schema)
            .expect("schema should persist");

        let existing = document(&table, "existing", "before");
        let binding = ResourcePathBinding::new(
            DocumentLocator::new(table.clone(), existing.id.clone()),
            DocumentPath::from_segments(["projects", "alpha", "tasks", "existing"])
                .expect("path should parse"),
        );
        store
            .apply_execution_unit_batch(
                &[ResolvedWrite::Insert {
                    document: existing.clone(),
                    indexes: schema.indexes.clone(),
                    resource_path_binding: Some(binding.clone()),
                }],
                &[],
            )
            .expect("bound insert batch should succeed")
            .expect("bound insert should emit a commit");

        let failed = store
            .delete_with_indexes(&table, &existing.id, &schema.indexes)
            .expect_err("commit fault should abort the point delete");

        assert!(
            matches!(failed, Error::Internal(ref message) if message.contains("storage_commit_before_visibility")),
            "expected injected commit fault, got {failed:?}"
        );
        assert!(
            store
                .get(&table, &existing.id)
                .expect("document lookup should succeed")
                .is_some(),
            "failed point delete must not remove the document"
        );
        assert_eq!(
            store
                .index_scan_eq(&table, "by_body", &json!("before"))
                .expect("index scan should succeed")
                .len(),
            1,
            "failed point delete must not remove index entries"
        );
        assert_eq!(
            store
                .resource_path_binding(&binding.locator)
                .expect("binding lookup should succeed"),
            Some(binding),
            "failed point delete must not remove path metadata"
        );
        assert_eq!(
            store
                .latest_sequence()
                .expect("latest sequence should remain readable"),
            SequenceNumber(2),
            "failed point delete must not append a commit log entry"
        );
    }

    #[test]
    fn with_index_point_update_stamps_update_time() {
        let clock = Arc::new(ManualWallClock::new(Timestamp(10_000)));
        let store = TenantStore::create_in_memory_with_simulation(
            clock.clone(),
            Arc::new(NoopFaultInjector),
        )
        .expect("store should open");
        let table = TableName::new("tasks_index_update_time").expect("table should parse");
        let schema = schema(&table);
        store
            .replace_table_schema(&schema)
            .expect("schema should persist");

        let existing = fixed_document(&table, "indexed", "before", Timestamp(1_000));
        store
            .insert_with_indexes(&existing, &schema.indexes)
            .expect("seed document should insert");

        clock.set(Timestamp(20_000));
        let patch = serde_json::Map::from_iter([("body".to_string(), json!("after"))]);
        let commit = store
            .update_with_indexes(&table, &existing.id, &patch, &schema.indexes)
            .expect("indexed update should commit");
        let updated = store
            .get(&table, &existing.id)
            .expect("document lookup should succeed")
            .expect("document should exist");

        assert_eq!(updated.update_time, Timestamp(20_000));
        assert_eq!(
            only_write(&commit)
                .current
                .as_ref()
                .expect("update should record current")
                .update_time,
            Timestamp(20_000),
            "with-index update WriteOp must carry the stamped document"
        );
    }

    #[test]
    fn with_index_point_delete_removes_resource_path_binding() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let table = TableName::new("tasks_index_delete_binding").expect("table should parse");
        let schema = schema(&table);
        store
            .replace_table_schema(&schema)
            .expect("schema should persist");

        let existing = document(&table, "bound", "before");
        let binding = ResourcePathBinding::new(
            DocumentLocator::new(table.clone(), existing.id.clone()),
            DocumentPath::from_segments(["projects", "alpha", "tasks", "bound"])
                .expect("path should parse"),
        );
        store
            .apply_execution_unit_batch(
                &[ResolvedWrite::Insert {
                    document: existing.clone(),
                    indexes: schema.indexes.clone(),
                    resource_path_binding: Some(binding.clone()),
                }],
                &[],
            )
            .expect("bound insert batch should succeed")
            .expect("bound insert should emit a commit");

        let (commit, removed) = store
            .delete_with_indexes_returning_document(&table, &existing.id, &schema.indexes)
            .expect("indexed delete should commit");

        assert_eq!(removed, existing);
        assert_eq!(
            store
                .resource_path_binding(&binding.locator)
                .expect("binding lookup should succeed"),
            None,
            "with-index point delete must remove locator path metadata"
        );
        assert_eq!(
            store
                .locator_for_document_path(&binding.document_path)
                .expect("path lookup should succeed"),
            None,
            "with-index point delete must remove reverse path metadata"
        );
        assert_eq!(
            only_write(&commit).resource_path_binding,
            Some(binding),
            "delete WriteOp must report the removed resource path binding"
        );
    }

    #[test]
    fn point_and_batch_updates_emit_identical_write_op_shapes() {
        let table = TableName::new("tasks_write_op_shape").expect("table should parse");
        let schema = schema(&table);
        let previous = fixed_document(&table, "shape", "before", Timestamp(1_000));
        let mut current = previous.clone();
        current.set_field("body", json!("after"));
        current.update_time = Timestamp(2_000);
        let patch = serde_json::Map::from_iter([("body".to_string(), json!("after"))]);

        let no_index_clock = Arc::new(ManualWallClock::new(Timestamp(2_000)));
        let no_index_store = TenantStore::create_in_memory_with_simulation(
            no_index_clock,
            Arc::new(NoopFaultInjector),
        )
        .expect("store should open");
        no_index_store
            .insert(&previous)
            .expect("no-index seed insert should commit");
        let no_index_commit = no_index_store
            .update(&table, &previous.id, &patch)
            .expect("no-index update should commit");

        let with_index_clock = Arc::new(ManualWallClock::new(Timestamp(2_000)));
        let with_index_store = TenantStore::create_in_memory_with_simulation(
            with_index_clock,
            Arc::new(NoopFaultInjector),
        )
        .expect("store should open");
        with_index_store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        with_index_store
            .insert_with_indexes(&previous, &schema.indexes)
            .expect("with-index seed insert should commit");
        let with_index_commit = with_index_store
            .update_with_indexes(&table, &previous.id, &patch, &schema.indexes)
            .expect("with-index update should commit");

        let batch_store = TenantStore::create_in_memory().expect("store should open");
        batch_store
            .replace_table_schema(&schema)
            .expect("schema should persist");
        batch_store
            .insert_with_indexes(&previous, &schema.indexes)
            .expect("batch seed insert should commit");
        let batch_commit = batch_store
            .apply_resolved_write_batch(&[ResolvedWrite::Update {
                previous: previous.clone(),
                current,
                indexes: schema.indexes.clone(),
                resource_path_binding: None,
            }])
            .expect("batch update should commit");

        let no_index_shape = write_op_shape(only_write(&no_index_commit));
        assert_eq!(
            no_index_shape,
            write_op_shape(only_write(&with_index_commit)),
            "no-index and with-index point updates should emit the same WriteOp shape"
        );
        assert_eq!(
            no_index_shape,
            write_op_shape(only_write(&batch_commit)),
            "point and batch updates should emit the same WriteOp shape"
        );
    }

    #[test]
    fn batch_insert_skips_physical_entries_for_non_maintained_index() {
        use redb::ReadableTable;

        let store = TenantStore::create_in_memory().expect("store should open");
        let table = TableName::new("tasks_index_state").expect("table should parse");
        let mut schema = schema(&table);
        // Stage a not-yet-backfilled (Pending) index alongside the Enabled one.
        schema.indexes.push(IndexDefinition {
            id: nimbus_core::IndexId::new(),
            state: nimbus_core::IndexState::Pending,
            name: "by_owner_pending".to_string(),
            fields: vec!["owner".to_string()],
        });
        store
            .replace_table_schema(&schema)
            .expect("schema should persist");

        let inserted = document(&table, "doc-state", "alpha");
        store
            .apply_resolved_write_batch(&[ResolvedWrite::Insert {
                document: inserted.clone(),
                indexes: schema.indexes.clone(),
                resource_path_binding: None,
            }])
            .expect("batch insert should commit");

        // The document carries both `owner` and `body`, so an unfiltered batch
        // path would write two physical entries. Only the Enabled `by_body`
        // index may persist one; the Pending `by_owner_pending` must not, matching
        // the interactive write path's is_maintained() filter.
        let read_txn = store.db.begin_read().expect("read txn should open");
        let index_table = read_txn.open_table(INDEXES).expect("INDEXES should open");
        let physical_entries = index_table
            .iter()
            .expect("index iteration should succeed")
            .count();
        assert_eq!(
            physical_entries, 1,
            "batch path must persist only the maintained (Enabled) index entry"
        );

        // The Enabled index resolves and returns the document.
        assert_eq!(
            store
                .index_scan_eq(&table, "by_body", &json!("alpha"))
                .expect("enabled index scan should succeed")
                .len(),
            1,
            "Enabled index must answer the query"
        );
    }

    #[test]
    fn update_without_resource_path_binding_keeps_existing_binding() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let table = TableName::new("tasks_bound_update").expect("table should parse");
        let schema = schema(&table);
        store
            .replace_table_schema(&schema)
            .expect("schema should persist");

        let previous = document(&table, "bound", "before");
        let binding = ResourcePathBinding::new(
            DocumentLocator::new(table.clone(), previous.id.clone()),
            DocumentPath::from_segments(["projects", "alpha", "tasks", "bound"])
                .expect("path should parse"),
        );
        store
            .apply_execution_unit_batch(
                &[ResolvedWrite::Insert {
                    document: previous.clone(),
                    indexes: schema.indexes.clone(),
                    resource_path_binding: Some(binding.clone()),
                }],
                &[],
            )
            .expect("bound insert batch should succeed")
            .expect("bound insert should emit a commit");

        let current = document(&table, "bound", "after");
        store
            .apply_execution_unit_batch(
                &[ResolvedWrite::Update {
                    previous,
                    current,
                    indexes: schema.indexes,
                    resource_path_binding: None,
                }],
                &[],
            )
            .expect("update batch should succeed")
            .expect("update should emit a commit");

        assert_eq!(
            store
                .resource_path_binding(&binding.locator)
                .expect("binding lookup should succeed"),
            Some(binding.clone()),
            "an update without a replacement binding must not remove locator-stable path metadata"
        );
        assert_eq!(
            store
                .locator_for_document_path(&binding.document_path)
                .expect("path lookup should succeed"),
            Some(binding.locator)
        );
    }
}
