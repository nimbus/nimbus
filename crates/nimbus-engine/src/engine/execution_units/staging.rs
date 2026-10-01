use std::time::Duration;

use nimbus_core::{Document, DocumentId, Error, Mutation, Result, TableName, Timestamp};

use super::super::mutations::{PreparedWriteOp, prepare_write_op};
use super::MutationExecutionUnit;
use super::state::{StagedSchedulerEntry, StagedWriteEntry};

impl MutationExecutionUnit {
    pub fn insert_document(
        &self,
        table: TableName,
        fields: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DocumentId> {
        self.insert_document_with_id(table, None, fields)
    }

    pub fn insert_document_with_id(
        &self,
        table: TableName,
        document_id: Option<DocumentId>,
        fields: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DocumentId> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let document_id = document_id.unwrap_or_else(|| self.engine.next_document_id());
        let document = Document::with_id_at(document_id, table.clone(), fields, Timestamp(0));
        let prepared = prepare_write_op(
            self.schema_snapshot.get_table(&table),
            &self.principal,
            None,
            Some(document),
            None,
            None,
        )?;
        let document_id = self.stage_prepared_write(table.clone(), prepared)?;
        self.active_state()?
            .deferred_server_timestamp_fields
            .remove(&(table, document_id.clone()));
        Ok(document_id)
    }

    pub fn update_document(
        &self,
        table: TableName,
        document_id: DocumentId,
        patch: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DocumentId> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let existing = self
            .current_document(&table, &document_id)?
            .ok_or(Error::DocumentNotFound(document_id.clone()))?;
        let mut document = existing.clone();
        let patched_fields = patch.keys().cloned().collect::<Vec<_>>();
        for (field, value) in patch {
            document.fields.insert(field, value);
        }
        let prepared = prepare_write_op(
            self.schema_snapshot.get_table(&table),
            &self.principal,
            Some(existing),
            Some(document),
            self.current_resource_path_binding(&table, &document_id)?,
            None,
        )?;
        self.stage_prepared_write(table.clone(), prepared)?;
        if let Some(fields) = self
            .active_state()?
            .deferred_server_timestamp_fields
            .get_mut(&(table, document_id.clone()))
        {
            fields.retain(|field| !patched_fields.contains(field));
        }
        Ok(document_id)
    }

    pub fn delete_document(&self, table: TableName, document_id: DocumentId) -> Result<()> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let existing = self
            .current_document(&table, &document_id)?
            .ok_or(Error::DocumentNotFound(document_id.clone()))?;
        let prepared = prepare_write_op(
            self.schema_snapshot.get_table(&table),
            &self.principal,
            Some(existing),
            None,
            None,
            None,
        )?;
        self.stage_prepared_write(table, prepared)?;
        Ok(())
    }

    pub fn schedule_mutation_after(
        &self,
        mutation: Mutation,
        delay_ms: u64,
    ) -> Result<nimbus_core::JobId> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let now = self.engine.now();
        let job = nimbus_core::ScheduledJob {
            id: self.engine.next_document_id(),
            run_at: now.saturating_add_duration(Duration::from_millis(delay_ms)),
            mutation,
            created_at: Timestamp(0),
        };
        let job_id = job.id.clone();
        self.stage_scheduled_job(job)?;
        Ok(job_id)
    }

    pub fn schedule_mutation_at(
        &self,
        mutation: Mutation,
        timestamp_ms: u64,
    ) -> Result<nimbus_core::JobId> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let job = nimbus_core::ScheduledJob {
            id: self.engine.next_document_id(),
            run_at: Timestamp(timestamp_ms),
            mutation,
            created_at: Timestamp(0),
        };
        let job_id = job.id.clone();
        self.stage_scheduled_job(job)?;
        Ok(job_id)
    }

    pub fn cancel_scheduled_job(&self, job_id: nimbus_core::JobId) -> Result<()> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.stage_scheduled_job_cancellation(job_id)
    }

    /// Stages one write that `prepare_write_op` built and returns its id.
    pub(super) fn stage_prepared_write(
        &self,
        table: TableName,
        prepared: PreparedWriteOp<'_>,
    ) -> Result<DocumentId> {
        let document_id = prepared.document()?.id.clone();
        let PreparedWriteOp {
            previous,
            current,
            indexes,
            resource_path_binding,
            ..
        } = prepared;
        self.stage_write(
            table,
            document_id.clone(),
            previous,
            current,
            indexes.to_vec(),
            resource_path_binding,
        )?;
        Ok(document_id)
    }

    pub(super) fn stage_write(
        &self,
        table: TableName,
        document_id: DocumentId,
        original: Option<Document>,
        current: Option<Document>,
        indexes: Vec<nimbus_core::IndexDefinition>,
        resource_path_binding: Option<nimbus_core::ResourcePathBinding>,
    ) -> Result<()> {
        let table_id = self.snapshot.table_id(&table)?;
        let mut state = self.active_state()?;
        let key = (table.clone(), document_id.clone());
        if !state.staged_writes.contains_key(&key) {
            state.write_order.push(key.clone());
        }

        let entry = state
            .staged_writes
            .entry(key.clone())
            .or_insert_with(|| StagedWriteEntry {
                original: original.clone(),
                current: None,
                indexes: indexes.clone(),
                resource_path_binding: resource_path_binding.clone(),
            });
        entry.current = current;
        entry.indexes = indexes;
        if entry.current.is_none() {
            entry.resource_path_binding = None;
        } else if let Some(resource_path_binding) = resource_path_binding {
            entry.resource_path_binding = Some(resource_path_binding);
        }

        if entry.original == entry.current {
            state.staged_writes.remove(&key);
            state.write_order.retain(|existing| existing != &key);
            state.deferred_server_timestamp_fields.remove(&key);
        } else {
            if entry.current.is_none() {
                state.deferred_server_timestamp_fields.remove(&key);
            }
            match table_id.as_ref() {
                Some(table_id) => {
                    state
                        .write_dependencies
                        .record_document(&table, table_id, document_id);
                }
                None => state.write_dependencies.record_missing_table(&table),
            }
        }
        Ok(())
    }

    fn stage_scheduled_job(&self, job: nimbus_core::ScheduledJob) -> Result<()> {
        let mut state = self.active_state()?;
        let job_id = job.id.clone();
        if !state.staged_scheduler_jobs.contains_key(&job_id) {
            state.scheduler_order.push(job_id.clone());
        }
        state
            .staged_scheduler_jobs
            .insert(job_id, StagedSchedulerEntry::Insert(job));
        Ok(())
    }

    fn stage_scheduled_job_cancellation(&self, job_id: nimbus_core::JobId) -> Result<()> {
        let mut state = self.active_state()?;
        match state.staged_scheduler_jobs.get(&job_id).cloned() {
            Some(StagedSchedulerEntry::Insert(_)) => {
                state
                    .staged_scheduler_jobs
                    .insert(job_id, StagedSchedulerEntry::NoOp);
                Ok(())
            }
            Some(StagedSchedulerEntry::CancelExisting | StagedSchedulerEntry::NoOp) => {
                Err(Error::ScheduledJobNotFound(job_id))
            }
            None => {
                state.scheduler_order.push(job_id.clone());
                state
                    .staged_scheduler_jobs
                    .insert(job_id, StagedSchedulerEntry::CancelExisting);
                Ok(())
            }
        }
    }
}
