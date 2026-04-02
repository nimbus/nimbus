use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use neovex_core::{
    AccessAction, CommitEntry, DependencySet, Document, DocumentId, Error, PaginatedQuery,
    PaginatedWindowDependency, PrincipalContext, Query, Result, Schema, SequenceNumber, TableName,
    Timestamp, commit_intersects_dependency_set,
};
use neovex_storage::{ResolvedScheduleOp, ResolvedWrite, TenantReadSnapshot};

use crate::evaluator::{
    decode_cursor, evaluate_paginated_with_docs_cancellable_and_predicate,
    evaluate_query_with_docs_cancellable_and_predicate,
};
use crate::tenant::TenantRuntime;

use super::Service;
use super::mutations::enforce_mutation_authorization;
use super::queries::ReadAuthorization;

#[derive(Debug, Clone)]
struct StagedWriteEntry {
    original: Option<Document>,
    current: Option<Document>,
    indexes: Vec<neovex_core::IndexDefinition>,
}

#[derive(Debug, Clone)]
enum StagedSchedulerEntry {
    Insert(neovex_core::ScheduledJob),
    CancelExisting,
    NoOp,
}

#[derive(Debug, Default)]
struct MutationExecutionUnitState {
    lifecycle: ExecutionUnitLifecycle,
    read_dependencies: DependencySet,
    write_dependencies: DependencySet,
    staged_writes: HashMap<(TableName, DocumentId), StagedWriteEntry>,
    write_order: Vec<(TableName, DocumentId)>,
    staged_scheduler_jobs: HashMap<neovex_core::JobId, StagedSchedulerEntry>,
    scheduler_order: Vec<neovex_core::JobId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ExecutionUnitLifecycle {
    #[default]
    Active,
    Finalizing,
    Finalized,
}

struct FinalizationGuard<'a> {
    unit: &'a MutationExecutionUnit,
}

impl Drop for FinalizationGuard<'_> {
    fn drop(&mut self) {
        self.unit.finish_finalization();
    }
}

pub struct MutationExecutionUnit {
    service: Arc<Service>,
    runtime: Arc<TenantRuntime>,
    tenant_id: neovex_core::TenantId,
    principal: PrincipalContext,
    schema_snapshot: Arc<Schema>,
    snapshot: TenantReadSnapshot,
    snapshot_sequence: SequenceNumber,
    state: Mutex<MutationExecutionUnitState>,
}

impl Service {
    pub fn begin_mutation_execution_unit(
        self: &Arc<Self>,
        tenant_id: neovex_core::TenantId,
        principal: PrincipalContext,
    ) -> Result<Arc<MutationExecutionUnit>> {
        let runtime = self.get_existing_tenant(&tenant_id)?;
        let snapshot = runtime.store.read_snapshot()?;
        let snapshot_sequence = snapshot.applied_sequence()?;
        let schema_snapshot = runtime.schema();
        Ok(Arc::new(MutationExecutionUnit {
            service: self.clone(),
            runtime,
            tenant_id,
            principal,
            schema_snapshot,
            snapshot,
            snapshot_sequence,
            state: Mutex::new(MutationExecutionUnitState::default()),
        }))
    }
}

impl MutationExecutionUnit {
    pub fn snapshot_sequence(&self) -> SequenceNumber {
        self.snapshot_sequence
    }

    pub fn read_dependencies(&self) -> DependencySet {
        self.state
            .lock()
            .expect("mutation execution unit lock should not be poisoned")
            .read_dependencies
            .clone()
    }

    pub fn write_dependencies(&self) -> DependencySet {
        self.state
            .lock()
            .expect("mutation execution unit lock should not be poisoned")
            .write_dependencies
            .clone()
    }

    pub fn get_document(
        &self,
        table: &TableName,
        document_id: DocumentId,
    ) -> Result<Option<Document>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let table_schema = self.schema_snapshot.get_table(table);
        let authorization = ReadAuthorization::for_table(table_schema, &self.principal)?;
        if authorization.impossible {
            return Ok(None);
        }

        let document = self.current_document(table, document_id)?;
        self.state
            .lock()
            .expect("mutation execution unit lock should not be poisoned")
            .read_dependencies
            .record_document(table, document_id);

        match document {
            Some(document) if authorization.allows_document(&self.principal, &document)? => {
                Ok(Some(document))
            }
            Some(_) | None => Ok(None),
        }
    }

    pub fn query_documents_cancellable(
        &self,
        query: &Query,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let table_schema = self.schema_snapshot.get_table(&query.table);
        let authorization = ReadAuthorization::for_table(table_schema, &self.principal)?;
        if authorization.impossible {
            return Ok(Vec::new());
        }

        let merged_query = authorization.merge_query(query);
        self.record_query_dependency(&merged_query)?;
        let documents = self.materialize_table_view(&query.table, check_cancel)?;
        let mut include_document =
            |document: &Document| authorization.allows_document(&self.principal, document);
        let result = evaluate_query_with_docs_cancellable_and_predicate(
            documents,
            &merged_query,
            check_cancel,
            &mut include_document,
        )?;
        if let Some(limit) = query.limit {
            self.record_limited_window_dependency(&merged_query, limit, &result)?;
        }
        Ok(result)
    }

    pub fn paginate_documents_cancellable(
        &self,
        query: &PaginatedQuery,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<neovex_core::Page> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let table_schema = self.schema_snapshot.get_table(&query.query.table);
        let authorization = ReadAuthorization::for_table(table_schema, &self.principal)?;
        if authorization.impossible {
            let empty = neovex_core::Page {
                data: Vec::new(),
                has_more: false,
                next_cursor: None,
            };
            self.record_paginated_window_dependency(query, &empty)?;
            return Ok(empty);
        }

        let merged_query = authorization.merge_query(&query.query);
        let merged_paginated = PaginatedQuery {
            query: merged_query.clone(),
            page_size: query.page_size,
            after: query.after.clone(),
        };
        let documents = self.materialize_table_view(&query.query.table, check_cancel)?;
        let mut include_document =
            |document: &Document| authorization.allows_document(&self.principal, document);
        let page = evaluate_paginated_with_docs_cancellable_and_predicate(
            documents,
            &merged_paginated,
            check_cancel,
            &mut include_document,
        )?;
        self.record_paginated_window_dependency(&merged_paginated, &page)?;
        Ok(page)
    }

    pub fn insert_document(
        &self,
        table: TableName,
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
        let document = Document::new(table.clone(), fields);
        enforce_mutation_authorization(
            table_schema.as_ref(),
            AccessAction::Create,
            &self.principal,
            Some(&document),
            None,
        )?;
        self.stage_write(table, document.id, None, Some(document.clone()), indexes)?;
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
            .current_document(&table, document_id)?
            .ok_or(Error::DocumentNotFound(document_id))?;
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
        self.stage_write(table, document_id, Some(existing), Some(document), indexes)?;
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
            .current_document(&table, document_id)?
            .ok_or(Error::DocumentNotFound(document_id))?;
        enforce_mutation_authorization(
            table_schema.as_ref(),
            AccessAction::Delete,
            &self.principal,
            None,
            Some(&existing),
        )?;
        self.stage_write(table, document_id, Some(existing), None, indexes)?;
        Ok(())
    }

    pub fn schedule_mutation_after(
        &self,
        mutation: neovex_core::Mutation,
        delay_ms: u64,
    ) -> Result<neovex_core::JobId> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let now = self.service.now();
        let job = neovex_core::ScheduledJob {
            id: neovex_core::JobId::new(),
            run_at: Timestamp(now.0.saturating_add(delay_ms)),
            mutation,
            created_at: now,
        };
        let job_id = job.id;
        self.stage_scheduled_job(job)?;
        Ok(job_id)
    }

    pub fn schedule_mutation_at(
        &self,
        mutation: neovex_core::Mutation,
        timestamp_ms: u64,
    ) -> Result<neovex_core::JobId> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let now = self.service.now();
        let job = neovex_core::ScheduledJob {
            id: neovex_core::JobId::new(),
            run_at: Timestamp(timestamp_ms.max(now.0)),
            mutation,
            created_at: now,
        };
        let job_id = job.id;
        self.stage_scheduled_job(job)?;
        Ok(job_id)
    }

    pub fn cancel_scheduled_job(&self, job_id: neovex_core::JobId) -> Result<()> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        self.stage_scheduled_job_cancellation(job_id)
    }

    pub fn commit(&self) -> Result<Option<CommitEntry>> {
        let _operation = self.runtime.enter_operation(&self.tenant_id)?;
        let finalization_guard = FinalizationGuard { unit: self };
        let (writes, schedule_ops, conflict_dependencies) = {
            let mut state = self.active_state()?;
            state.lifecycle = ExecutionUnitLifecycle::Finalizing;
            let writes = self.build_resolved_writes(&state);
            let schedule_ops = self.build_resolved_schedule_ops(&state);
            let mut conflict_dependencies = state.read_dependencies.clone();
            conflict_dependencies.extend(&state.write_dependencies);
            (writes, schedule_ops, conflict_dependencies)
        };
        if writes.is_empty() && schedule_ops.is_empty() {
            return Ok(None);
        }

        let result = (|| -> Result<Option<CommitEntry>> {
            self.ensure_schema_unchanged(&conflict_dependencies)?;
            self.ensure_no_conflicts(&conflict_dependencies)?;

            let commit = {
                let _sequence_guard = self.runtime.lock_mutation_sequence();
                self.runtime
                    .store
                    .apply_execution_unit_batch(&writes, &schedule_ops)?
            };
            Ok(commit)
        })();
        drop(finalization_guard);
        let commit = result?;

        if let Some(commit) = &commit {
            self.runtime.mark_durable_head(commit.sequence);
            self.runtime.invalidate_document_cache_for_commit(commit);
            self.runtime.mark_applied_head(commit.sequence);
            self.service.process_commit(self.runtime.clone(), commit);
        }
        if schedule_ops
            .iter()
            .any(|operation| matches!(operation, ResolvedScheduleOp::Insert { .. }))
        {
            self.service.wake_scheduler();
        }
        Ok(commit)
    }

    fn current_document(
        &self,
        table: &TableName,
        document_id: DocumentId,
    ) -> Result<Option<Document>> {
        let state = self.active_state()?;
        if let Some(entry) = state.staged_writes.get(&(table.clone(), document_id)) {
            return Ok(entry.current.clone());
        }
        drop(state);
        self.snapshot.get(table, &document_id)
    }

    fn materialize_table_view(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.ensure_active()?;
        let mut documents =
            self.snapshot
                .scan_table_matching_cancellable(table, check_cancel, |_document| Ok(true))?;
        let state = self.active_state()?;
        let staged_ids = state
            .staged_writes
            .iter()
            .filter_map(|((entry_table, document_id), _)| {
                (entry_table == table).then_some(*document_id)
            })
            .collect::<HashSet<_>>();
        if !staged_ids.is_empty() {
            documents.retain(|document| !staged_ids.contains(&document.id));
        }
        for ((entry_table, _document_id), entry) in &state.staged_writes {
            if entry_table != table {
                continue;
            }
            if let Some(document) = entry.current.clone() {
                documents.push(document);
            }
        }
        Ok(documents)
    }

    fn stage_write(
        &self,
        table: TableName,
        document_id: DocumentId,
        original: Option<Document>,
        current: Option<Document>,
        indexes: Vec<neovex_core::IndexDefinition>,
    ) -> Result<()> {
        let mut state = self.active_state()?;
        let key = (table.clone(), document_id);
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
            });
        entry.current = current;
        entry.indexes = indexes;

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

    fn stage_scheduled_job(&self, job: neovex_core::ScheduledJob) -> Result<()> {
        let mut state = self.active_state()?;
        let job_id = job.id;
        if !state.staged_scheduler_jobs.contains_key(&job_id) {
            state.scheduler_order.push(job_id);
        }
        state
            .staged_scheduler_jobs
            .insert(job_id, StagedSchedulerEntry::Insert(job));
        Ok(())
    }

    fn stage_scheduled_job_cancellation(&self, job_id: neovex_core::JobId) -> Result<()> {
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
                state.scheduler_order.push(job_id);
                state
                    .staged_scheduler_jobs
                    .insert(job_id, StagedSchedulerEntry::CancelExisting);
                Ok(())
            }
        }
    }

    fn record_query_dependency(&self, query: &Query) -> Result<()> {
        let mut state = self.active_state()?;
        if query.filters.is_empty() {
            state.read_dependencies.record_table(&query.table);
        } else {
            state
                .read_dependencies
                .record_predicate(neovex_core::PredicateDependency {
                    table: query.table.clone(),
                    filters: query.filters.clone(),
                });
        }
        Ok(())
    }

    fn record_limited_window_dependency(
        &self,
        query: &Query,
        limit: usize,
        documents: &[Document],
    ) -> Result<()> {
        if query.order.is_none() {
            return Ok(());
        }
        self.active_state()?
            .read_dependencies
            .record_paginated_window(PaginatedWindowDependency {
                table: query.table.clone(),
                filters: query.filters.clone(),
                order: query.order.clone(),
                start_sort_values: Vec::new(),
                start_doc_id: None,
                end_sort_values: documents
                    .last()
                    .map(|document| match query.order.as_ref() {
                        Some(order) => vec![document.get_field(&order.field).cloned()],
                        None => Vec::new(),
                    })
                    .unwrap_or_default(),
                end_doc_id: documents.last().map(|document| document.id),
                result_count: documents.len(),
                page_size: limit,
            });
        Ok(())
    }

    fn record_paginated_window_dependency(
        &self,
        paginated: &PaginatedQuery,
        page: &neovex_core::Page,
    ) -> Result<()> {
        let (start_sort_values, start_doc_id) = paginated
            .after
            .as_ref()
            .map(|cursor| decode_cursor(cursor, &paginated.query))
            .transpose()?
            .map_or((Vec::new(), None), |(sort_values, document_id)| {
                (sort_values, Some(document_id))
            });
        let end_document = page
            .data
            .last()
            .and_then(|value| value.get("_id").and_then(serde_json::Value::as_str))
            .and_then(|value| value.parse::<DocumentId>().ok());
        let end_sort_values = page
            .data
            .last()
            .map(|value| match paginated.query.order.as_ref() {
                Some(order) => vec![value.get(&order.field).cloned()],
                None => Vec::new(),
            })
            .unwrap_or_default();
        self.active_state()?
            .read_dependencies
            .record_paginated_window(PaginatedWindowDependency {
                table: paginated.query.table.clone(),
                filters: paginated.query.filters.clone(),
                order: paginated.query.order.clone(),
                start_sort_values,
                start_doc_id,
                end_sort_values,
                end_doc_id: end_document,
                result_count: page.data.len(),
                page_size: paginated.page_size,
            });
        Ok(())
    }

    fn active_state(&self) -> Result<std::sync::MutexGuard<'_, MutationExecutionUnitState>> {
        let state = self
            .state
            .lock()
            .expect("mutation execution unit lock should not be poisoned");
        Self::ensure_active_lifecycle(state.lifecycle)?;
        Ok(state)
    }

    fn ensure_active(&self) -> Result<()> {
        let state = self
            .state
            .lock()
            .expect("mutation execution unit lock should not be poisoned");
        Self::ensure_active_lifecycle(state.lifecycle)
    }

    fn ensure_active_lifecycle(lifecycle: ExecutionUnitLifecycle) -> Result<()> {
        match lifecycle {
            ExecutionUnitLifecycle::Active => Ok(()),
            ExecutionUnitLifecycle::Finalizing => Err(Error::InvalidInput(
                "mutation execution unit is finalizing; start a new execution unit".to_string(),
            )),
            ExecutionUnitLifecycle::Finalized => Err(Error::InvalidInput(
                "mutation execution unit is finalized; start a new execution unit".to_string(),
            )),
        }
    }

    fn finish_finalization(&self) {
        let mut state = self
            .state
            .lock()
            .expect("mutation execution unit lock should not be poisoned");
        // Execution units are single-use: successful, conflicting, and no-op
        // commit attempts all retire the staged state permanently.
        state.lifecycle = ExecutionUnitLifecycle::Finalized;
        state.staged_writes.clear();
        state.write_order.clear();
        state.staged_scheduler_jobs.clear();
        state.scheduler_order.clear();
        state.read_dependencies = DependencySet::default();
        state.write_dependencies = DependencySet::default();
    }

    fn build_resolved_writes(&self, state: &MutationExecutionUnitState) -> Vec<ResolvedWrite> {
        state
            .write_order
            .iter()
            .filter_map(|key| state.staged_writes.get(key))
            .filter_map(|entry| match (&entry.original, &entry.current) {
                (None, Some(current)) => Some(ResolvedWrite::Insert {
                    document: current.clone(),
                    indexes: entry.indexes.clone(),
                }),
                (Some(previous), Some(current)) => Some(ResolvedWrite::Update {
                    previous: previous.clone(),
                    current: current.clone(),
                    indexes: entry.indexes.clone(),
                }),
                (Some(previous), None) => Some(ResolvedWrite::Delete {
                    previous: previous.clone(),
                    indexes: entry.indexes.clone(),
                }),
                (None, None) => None,
            })
            .collect()
    }

    fn build_resolved_schedule_ops(
        &self,
        state: &MutationExecutionUnitState,
    ) -> Vec<ResolvedScheduleOp> {
        state
            .scheduler_order
            .iter()
            .filter_map(|job_id| match state.staged_scheduler_jobs.get(job_id) {
                Some(StagedSchedulerEntry::Insert(job)) => {
                    Some(ResolvedScheduleOp::Insert { job: job.clone() })
                }
                Some(StagedSchedulerEntry::CancelExisting) => {
                    Some(ResolvedScheduleOp::Cancel { job_id: *job_id })
                }
                Some(StagedSchedulerEntry::NoOp) | None => None,
            })
            .collect()
    }

    fn ensure_schema_unchanged(&self, dependencies: &DependencySet) -> Result<()> {
        let current_schema = self.runtime.schema();
        for table in touched_tables(dependencies) {
            if current_schema.get_table(&table) != self.schema_snapshot.get_table(&table) {
                return Err(Error::Conflict(format!(
                    "table schema changed during transaction: {}",
                    table
                )));
            }
        }
        Ok(())
    }

    fn ensure_no_conflicts(&self, dependencies: &DependencySet) -> Result<()> {
        if dependencies.is_empty() {
            return Ok(());
        }

        let commits = self
            .runtime
            .store
            .read_commit_log_from(SequenceNumber(self.snapshot_sequence.0.saturating_add(1)))?;
        for commit in commits {
            if commit_intersects_dependency_set(&commit, dependencies, &[], |table, document_id| {
                self.runtime.store.get(table, &document_id)
            }) {
                return Err(Error::Conflict(
                    "transaction conflict detected; retry the mutation".to_string(),
                ));
            }
        }
        Ok(())
    }
}

fn touched_tables(dependencies: &DependencySet) -> Vec<TableName> {
    let mut tables = dependencies.tables.iter().cloned().collect::<Vec<_>>();
    for (table, _) in &dependencies.documents {
        if !tables.iter().any(|candidate| candidate == table) {
            tables.push(table.clone());
        }
    }
    for dependency in &dependencies.index_ranges {
        if !tables
            .iter()
            .any(|candidate| candidate == &dependency.table)
        {
            tables.push(dependency.table.clone());
        }
    }
    for dependency in &dependencies.predicates {
        if !tables
            .iter()
            .any(|candidate| candidate == &dependency.table)
        {
            tables.push(dependency.table.clone());
        }
    }
    for dependency in &dependencies.paginated_windows {
        if !tables
            .iter()
            .any(|candidate| candidate == &dependency.table)
        {
            tables.push(dependency.table.clone());
        }
    }
    tables
}
