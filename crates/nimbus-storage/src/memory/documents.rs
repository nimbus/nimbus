use nimbus_core::{
    CommitEntry, Document, DocumentId, DocumentLocator, Error, IndexDefinition,
    ResourcePathBinding, Result, TableName, TenantEventKind, Timestamp, TriggerWriteOrigin,
    WriteOp, WriteOpType,
};
use serde_json::{Map, Value};

use crate::index::encode_index_tuple;
use crate::{ResolvedScheduleOp, ResolvedWrite};

use super::MemoryTenantStore;
use super::state::MemoryState;

#[derive(Clone, Copy)]
enum WriteExpectation {
    Point,
    Batch,
}

impl WriteExpectation {
    fn missing(self, id: &DocumentId) -> Error {
        match self {
            Self::Point => Error::DocumentNotFound(id.clone()),
            Self::Batch => changed_before_commit(id),
        }
    }

    fn mismatch(self, id: &DocumentId) -> Error {
        self.missing(id)
    }
}

fn changed_before_commit(id: &DocumentId) -> Error {
    Error::conflict(format!("document {id} changed before transaction commit"))
}

fn validate_index_values(document: &Document, indexes: &[IndexDefinition]) -> Result<()> {
    for index in indexes.iter().filter(|index| index.is_maintained()) {
        let values = index
            .fields
            .iter()
            .map(|field| document.fields.get(field).cloned())
            .collect::<Option<Vec<_>>>();
        if let Some(values) = values {
            let _ = encode_index_tuple(&values)?;
        }
    }
    Ok(())
}

impl MemoryState {
    fn apply_insert(
        &mut self,
        document: &Document,
        indexes: &[IndexDefinition],
        resource_path_binding: Option<&ResourcePathBinding>,
        trigger_write_origin: Option<&TriggerWriteOrigin>,
    ) -> Result<WriteOp> {
        validate_index_values(document, indexes)?;
        let table_id = self.resolve_or_create_table_id(&document.table)?;
        let documents = self.documents.entry(table_id.clone()).or_default();
        if documents.contains_key(&document.id) {
            return Err(changed_before_commit(&document.id));
        }
        documents.insert(document.id.clone(), document.clone());
        if let Some(binding) = resource_path_binding {
            self.upsert_resource_path_binding(binding)?;
        }
        Ok(WriteOp {
            table: document.table.clone(),
            table_id,
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: resource_path_binding.cloned(),
            trigger_write_origin: trigger_write_origin.cloned(),
            previous: None,
            current: Some(document.clone()),
        })
    }

    fn apply_update(
        &mut self,
        previous: &Document,
        current: &Document,
        indexes: &[IndexDefinition],
        resource_path_binding: Option<&ResourcePathBinding>,
        trigger_write_origin: Option<&TriggerWriteOrigin>,
        expectation: WriteExpectation,
    ) -> Result<WriteOp> {
        validate_index_values(current, indexes)?;
        let table_id = self
            .active_tables
            .get(&current.table)
            .cloned()
            .ok_or_else(|| expectation.missing(&current.id))?;
        let documents = self.documents.entry(table_id.clone()).or_default();
        let existing = documents
            .get(&current.id)
            .cloned()
            .ok_or_else(|| expectation.missing(&current.id))?;
        if existing != *previous {
            return Err(expectation.mismatch(&current.id));
        }
        documents.insert(current.id.clone(), current.clone());
        if let Some(binding) = resource_path_binding {
            self.upsert_resource_path_binding(binding)?;
        }
        Ok(WriteOp {
            table: current.table.clone(),
            table_id,
            op_type: WriteOpType::Update,
            doc_id: current.id.clone(),
            resource_path_binding: resource_path_binding.cloned(),
            trigger_write_origin: trigger_write_origin.cloned(),
            previous: Some(previous.clone()),
            current: Some(current.clone()),
        })
    }

    fn apply_delete(
        &mut self,
        previous: &Document,
        indexes: &[IndexDefinition],
        trigger_write_origin: Option<&TriggerWriteOrigin>,
        expectation: WriteExpectation,
    ) -> Result<WriteOp> {
        validate_index_values(previous, indexes)?;
        let table_id = self
            .active_tables
            .get(&previous.table)
            .cloned()
            .ok_or_else(|| expectation.missing(&previous.id))?;
        let documents = self.documents.entry(table_id.clone()).or_default();
        let existing = documents
            .get(&previous.id)
            .cloned()
            .ok_or_else(|| expectation.missing(&previous.id))?;
        if existing != *previous {
            return Err(expectation.mismatch(&previous.id));
        }
        documents.remove(&previous.id);
        let resource_path_binding = self.remove_resource_path_binding(&DocumentLocator::new(
            previous.table.clone(),
            previous.id.clone(),
        ));
        Ok(WriteOp {
            table: previous.table.clone(),
            table_id,
            op_type: WriteOpType::Delete,
            doc_id: previous.id.clone(),
            resource_path_binding,
            trigger_write_origin: trigger_write_origin.cloned(),
            previous: Some(previous.clone()),
            current: None,
        })
    }

    fn begin_scheduled_execution(
        &mut self,
        execution_id: Option<&str>,
        events: &mut Vec<TenantEventKind>,
    ) -> bool {
        let Some(execution_id) = execution_id else {
            return true;
        };
        if !self
            .scheduled_execution_ids
            .insert(execution_id.to_string())
        {
            return false;
        }
        events.push(TenantEventKind::ScheduledExecution {
            execution_id: execution_id.to_string(),
        });
        true
    }
}

impl MemoryTenantStore {
    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        Ok(self.read_state()?.get(table, id))
    }

    pub fn insert(&self, document: &Document) -> Result<CommitEntry> {
        self.insert_once(document, None)?
            .ok_or_else(|| Error::Internal("non-deduplicated insert should commit".to_string()))
    }

    pub fn insert_once(
        &self,
        document: &Document,
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        self.insert_with_indexes_once(document, &[], execution_id)
    }

    pub fn insert_with_indexes(
        &self,
        document: &Document,
        indexes: &[IndexDefinition],
    ) -> Result<CommitEntry> {
        self.insert_with_indexes_once(document, indexes, None)?
            .ok_or_else(|| {
                Error::Internal("non-deduplicated indexed insert should commit".to_string())
            })
    }

    pub fn insert_with_indexes_once(
        &self,
        document: &Document,
        indexes: &[IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        let timestamp = self.now();
        self.transact(|state| {
            let mut events = Vec::new();
            if !state.begin_scheduled_execution(execution_id, &mut events) {
                return Ok(None);
            }
            let write = state.apply_insert(document, indexes, None, None)?;
            state
                .append_events(timestamp, vec![write], events)
                .map(Some)
        })
    }

    pub fn update_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &Map<String, Value>,
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated_once(table, id, patch, None, validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated update should commit".to_string()))
    }

    pub fn update_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &Map<String, Value>,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_with_indexes_validated_once(table, id, patch, &[], execution_id, validate)
    }

    pub fn update_with_indexes_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &Map<String, Value>,
        indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_with_indexes_validated_once(table, id, patch, indexes, None, validate)?
            .ok_or_else(|| {
                Error::Internal("non-deduplicated indexed update should commit".to_string())
            })
    }

    pub fn update_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &Map<String, Value>,
        indexes: &[IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        let timestamp = self.now();
        self.transact(|state| {
            let mut events = Vec::new();
            if !state.begin_scheduled_execution(execution_id, &mut events) {
                return Ok(None);
            }
            let previous = state
                .get(table, id)
                .ok_or_else(|| Error::DocumentNotFound(id.clone()))?;
            let mut current = previous.clone();
            for (field, value) in patch {
                current.set_field(field.clone(), value.clone());
            }
            current.update_time = timestamp;
            validate(&previous, &current)?;
            let write = state.apply_update(
                &previous,
                &current,
                indexes,
                None,
                None,
                WriteExpectation::Point,
            )?;
            state
                .append_events(timestamp, vec![write], events)
                .map(Some)
        })
    }

    pub fn delete_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_once(table, id, None, validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated delete should commit".to_string()))
    }

    pub fn delete_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_with_indexes_validated_once(table, id, &[], execution_id, validate)
    }

    pub fn delete_with_indexes_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_with_indexes_validated_once(table, id, indexes, None, validate)?
            .ok_or_else(|| {
                Error::Internal("non-deduplicated indexed delete should commit".to_string())
            })
    }

    pub fn delete_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        let timestamp = self.now();
        self.transact(|state| {
            let mut events = Vec::new();
            if !state.begin_scheduled_execution(execution_id, &mut events) {
                return Ok(None);
            }
            let previous = state
                .get(table, id)
                .ok_or_else(|| Error::DocumentNotFound(id.clone()))?;
            validate(&previous)?;
            let write = state.apply_delete(&previous, indexes, None, WriteExpectation::Point)?;
            let commit = state.append_events(timestamp, vec![write], events)?;
            Ok(Some((commit, previous)))
        })
    }

    pub fn apply_execution_unit_batch_with_origin(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
        trigger_write_origin: Option<&TriggerWriteOrigin>,
        commit_timestamp: Option<Timestamp>,
    ) -> Result<Option<CommitEntry>> {
        if writes.is_empty() && schedule_ops.is_empty() {
            return Err(Error::Internal(
                "execution-unit batch must contain at least one change".to_string(),
            ));
        }
        let timestamp = commit_timestamp.unwrap_or_else(|| self.now());
        self.transact(|state| {
            let mut commit_writes = Vec::with_capacity(writes.len());
            for write in writes {
                let commit_write = match write {
                    ResolvedWrite::Insert {
                        document,
                        indexes,
                        resource_path_binding,
                    } => state.apply_insert(
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
                    } => state.apply_update(
                        previous,
                        current,
                        indexes,
                        resource_path_binding.as_ref(),
                        trigger_write_origin,
                        WriteExpectation::Batch,
                    )?,
                    ResolvedWrite::Delete { previous, indexes } => state.apply_delete(
                        previous,
                        indexes,
                        trigger_write_origin,
                        WriteExpectation::Batch,
                    )?,
                };
                commit_writes.push(commit_write);
            }
            state.apply_schedule_ops(schedule_ops)?;
            if commit_writes.is_empty() {
                Ok(None)
            } else {
                state
                    .append_events(timestamp, commit_writes, Vec::new())
                    .map(Some)
            }
        })
    }
}
