use nimbus_core::{
    AccessAction, Document, DocumentId, Error, Mutation, Result, TableName, Timestamp,
};

use super::super::mutations::enforce_mutation_authorization;
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
        let table_schema = self.schema_snapshot.get_table(&table).cloned();
        let indexes = table_schema
            .as_ref()
            .map(|table_schema| {
                table_schema.validate(&fields)?;
                Ok(table_schema.indexes.clone())
            })
            .transpose()?
            .unwrap_or_default();
        let document = match document_id {
            Some(document_id) => Document::with_id(document_id, table.clone(), fields),
            None => Document::new(table.clone(), fields),
        };
        enforce_mutation_authorization(
            table_schema.as_ref(),
            AccessAction::Create,
            &self.principal,
            Some(&document),
            None,
        )?;
        self.stage_write(
            table,
            document.id.clone(),
            None,
            Some(document.clone()),
            indexes,
            None,
        )?;
        Ok(document.id)
    }

    pub fn update_document(
        &self,
        table: TableName,
        document_id: DocumentId,
        patch: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DocumentId> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let table_schema = self.schema_snapshot.get_table(&table).cloned();
        let indexes = table_schema
            .as_ref()
            .map(|table_schema| table_schema.indexes.clone())
            .unwrap_or_default();
        let existing = self
            .current_document(&table, &document_id)?
            .ok_or(Error::DocumentNotFound(document_id.clone()))?;
        let mut document = existing.clone();
        for (field, value) in patch {
            document.fields.insert(field, value);
        }
        if let Some(table_schema) = table_schema.as_ref() {
            table_schema.validate(&document.fields)?;
        }
        enforce_mutation_authorization(
            table_schema.as_ref(),
            AccessAction::Update,
            &self.principal,
            Some(&document),
            Some(&existing),
        )?;
        self.stage_write(
            table,
            document_id.clone(),
            Some(existing),
            Some(document),
            indexes,
            None,
        )?;
        Ok(document_id)
    }

    pub fn delete_document(&self, table: TableName, document_id: DocumentId) -> Result<()> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let table_schema = self.schema_snapshot.get_table(&table).cloned();
        let indexes = table_schema
            .as_ref()
            .map(|table_schema| table_schema.indexes.clone())
            .unwrap_or_default();
        let existing = self
            .current_document(&table, &document_id)?
            .ok_or(Error::DocumentNotFound(document_id.clone()))?;
        enforce_mutation_authorization(
            table_schema.as_ref(),
            AccessAction::Delete,
            &self.principal,
            None,
            Some(&existing),
        )?;
        self.stage_write(
            table,
            document_id.clone(),
            Some(existing),
            None,
            indexes,
            None,
        )?;
        Ok(())
    }

    pub fn schedule_mutation_after(
        &self,
        mutation: Mutation,
        delay_ms: u64,
    ) -> Result<nimbus_core::JobId> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let now = self.service.now();
        let job = nimbus_core::ScheduledJob {
            id: nimbus_core::JobId::new(),
            run_at: Timestamp(now.0.saturating_add(delay_ms)),
            mutation,
            created_at: now,
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
        let now = self.service.now();
        let job = nimbus_core::ScheduledJob {
            id: nimbus_core::JobId::new(),
            run_at: Timestamp(timestamp_ms.max(now.0)),
            mutation,
            created_at: now,
        };
        let job_id = job.id.clone();
        self.stage_scheduled_job(job)?;
        Ok(job_id)
    }

    pub fn cancel_scheduled_job(&self, job_id: nimbus_core::JobId) -> Result<()> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.stage_scheduled_job_cancellation(job_id)
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
        } else {
            state
                .write_dependencies
                .record_document(&table, document_id);
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
