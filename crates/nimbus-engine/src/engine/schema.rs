use std::collections::BTreeSet;
use std::sync::Arc;

use nimbus_core::{Error, Result, Schema, TableName, TableSchema, TenantId, policy_revision_id};

use crate::engine::execution_units::{CommitFaultClient, labels};
use crate::tenant::TenantRuntime;

use super::Engine;

const POLICY_REVISION_CHANGED_MESSAGE: &str = "authorization policy changed; resubscribe";

impl Engine {
    /// Stores a table schema for a tenant.
    pub fn set_table_schema(&self, tenant_id: &TenantId, table_schema: TableSchema) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let table = table_schema.table.clone();
        let runtime_for_commit = runtime.clone();
        let tenant_id_for_commit = tenant_id.clone();
        let commit_faults = self.commit_faults.clone();
        runtime.submit_internal_committer(move || {
            apply_set_table_schema(
                &runtime_for_commit,
                &tenant_id_for_commit,
                table_schema,
                &commit_faults,
            )
        })?;
        self.notify_table_schema_change_observers(tenant_id, &table);
        Ok(())
    }

    /// Stores a table schema for a tenant asynchronously.
    pub async fn set_table_schema_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table_schema: TableSchema,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let table = table_schema.table.clone();
        let tenant_id_for_task = tenant_id.clone();
        let runtime_for_task = runtime.clone();
        let commit_faults = self.commit_faults.clone();
        runtime
            .submit_internal_committer_async(move || {
                apply_set_table_schema(
                    &runtime_for_task,
                    &tenant_id_for_task,
                    table_schema,
                    &commit_faults,
                )
            })
            .await?;
        self.notify_table_schema_change_observers(&tenant_id, &table);
        Ok(())
    }

    /// Returns the full tenant schema.
    pub fn get_schema(&self, tenant_id: &TenantId) -> Result<Schema> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        Ok(runtime.schema().as_ref().clone())
    }

    /// Returns the full tenant schema asynchronously.
    pub async fn get_schema_async(self: &Arc<Self>, tenant_id: TenantId) -> Result<Schema> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        Ok(runtime.schema().as_ref().clone())
    }

    /// Returns a single table schema for a tenant.
    pub fn get_table_schema(&self, tenant_id: &TenantId, table: &TableName) -> Result<TableSchema> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        runtime
            .schema()
            .get_table(table)
            .cloned()
            .ok_or(Error::SchemaNotFound(table.clone()))
    }

    /// Returns a single table schema for a tenant asynchronously.
    pub async fn get_table_schema_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
    ) -> Result<TableSchema> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        runtime
            .schema()
            .get_table(&table)
            .cloned()
            .ok_or(Error::SchemaNotFound(table))
    }

    /// Deletes a single table schema for a tenant.
    pub fn delete_table_schema(&self, tenant_id: &TenantId, table: &TableName) -> Result<()> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let runtime_for_commit = runtime.clone();
        let tenant_id_for_commit = tenant_id.clone();
        let table_for_commit = table.clone();
        let commit_faults = self.commit_faults.clone();
        runtime.submit_internal_committer(move || {
            apply_delete_table_schema(
                &runtime_for_commit,
                &tenant_id_for_commit,
                &table_for_commit,
                &commit_faults,
            )
        })?;
        self.notify_table_schema_change_observers(tenant_id, table);
        Ok(())
    }

    /// Deletes a single table schema for a tenant asynchronously.
    pub async fn delete_table_schema_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
    ) -> Result<()> {
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let tenant_id_for_task = tenant_id.clone();
        let runtime_for_task = runtime.clone();
        let table_for_task = table.clone();
        let commit_faults = self.commit_faults.clone();
        runtime
            .submit_internal_committer_async(move || {
                apply_delete_table_schema(
                    &runtime_for_task,
                    &tenant_id_for_task,
                    &table_for_task,
                    &commit_faults,
                )
            })
            .await?;
        self.notify_table_schema_change_observers(&tenant_id, &table);
        Ok(())
    }
    pub(super) async fn refresh_loaded_schema_from_store_async(
        &self,
        runtime: &Arc<TenantRuntime>,
    ) -> Result<()> {
        let next_schema = runtime
            .store()
            .load_schema_async(runtime.read_storage())
            .await?;
        for table in apply_loaded_schema_snapshot(runtime, next_schema)? {
            self.notify_table_schema_change_observers(runtime.tenant_id(), &table);
        }
        Ok(())
    }
}

fn apply_loaded_schema_snapshot(
    runtime: &Arc<TenantRuntime>,
    next_schema: Schema,
) -> Result<Vec<TableName>> {
    let previous_schema = runtime.schema();
    if previous_schema.as_ref() == &next_schema {
        return Ok(Vec::new());
    }

    let mut changed_policy_tables = Vec::new();
    let mut changed_schema_tables = Vec::new();
    let table_names = previous_schema
        .tables
        .keys()
        .chain(next_schema.tables.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for table in table_names {
        let previous_revision = effective_policy_revision(previous_schema.get_table(&table))?;
        let next_revision = effective_policy_revision(next_schema.get_table(&table))?;
        if previous_schema.get_table(&table) != next_schema.get_table(&table) {
            changed_schema_tables.push(table.clone());
        }
        if previous_revision != next_revision {
            changed_policy_tables.push((table, next_revision));
        }
    }

    runtime.replace_schema_snapshot(Arc::new(next_schema));

    if !changed_policy_tables.is_empty() {
        runtime.clear_document_cache();
        for (table, revision) in changed_policy_tables {
            runtime
                .subscription_registry()
                .terminate_policy_revision_mismatches(
                    &table,
                    &revision,
                    POLICY_REVISION_CHANGED_MESSAGE,
                );
        }
    }

    Ok(changed_schema_tables)
}

fn apply_set_table_schema(
    runtime: &Arc<TenantRuntime>,
    tenant_id: &TenantId,
    table_schema: TableSchema,
    commit_faults: &CommitFaultClient,
) -> Result<()> {
    let _operation = runtime.enter_operation(tenant_id)?;
    let previous_durable_head = runtime.durable_head();
    let table = table_schema.table.clone();
    let mut table_schema = table_schema;
    let previous_schema = runtime.schema();
    table_schema.reconcile_index_metadata(previous_schema.get_table(&table));
    table_schema.validate_indexes()?;
    table_schema.validate_access_policy()?;
    let previous_policy_revision = previous_schema
        .get_table(&table)
        .map(TableSchema::access_policy_revision)
        .transpose()?;
    let next_policy_revision = table_schema.access_policy_revision()?;
    let policy_revision_changed =
        previous_policy_revision.as_deref() != Some(next_policy_revision.as_str());
    let pending_policy_termination = policy_revision_changed.then(|| {
        runtime
            .subscription_registry()
            .begin_policy_revision_mismatches(&table, &next_policy_revision)
    });

    if let Err(error) = runtime.store().replace_table_schema(&table_schema) {
        if let Some(pending) = pending_policy_termination {
            runtime
                .subscription_registry()
                .restore_policy_revision_mismatches(pending);
        }
        return Err(error);
    }
    let journal_progress = match runtime.store().journal_progress() {
        Ok(journal_progress) => journal_progress,
        Err(error) => {
            if let Some(pending) = pending_policy_termination {
                runtime
                    .subscription_registry()
                    .finish_policy_revision_mismatches(pending, POLICY_REVISION_CHANGED_MESSAGE);
            }
            return Err(error);
        }
    };
    stage_assigned_schema_record(runtime, previous_durable_head, journal_progress)?;
    pause_assigned_schema_before_visibility(commit_faults)?;

    let mut schema = previous_schema;
    Arc::make_mut(&mut schema)
        .tables
        .insert(table.clone(), table_schema);
    runtime.replace_schema_snapshot(schema);

    if let Some(pending) = pending_policy_termination {
        runtime.clear_document_cache();
        runtime
            .subscription_registry()
            .finish_policy_revision_mismatches(pending, POLICY_REVISION_CHANGED_MESSAGE);
    }
    runtime.sync_mutation_journal_progress_in_actor(journal_progress);
    Ok(())
}

fn apply_delete_table_schema(
    runtime: &Arc<TenantRuntime>,
    tenant_id: &TenantId,
    table: &TableName,
    commit_faults: &CommitFaultClient,
) -> Result<()> {
    let _operation = runtime.enter_operation(tenant_id)?;
    let previous_durable_head = runtime.durable_head();
    let previous_schema = runtime.schema();
    let previous_policy_revision = previous_schema
        .get_table(table)
        .map(TableSchema::access_policy_revision)
        .transpose()?;

    let removed_policy_revision = policy_revision_id(None)?;
    let policy_revision_changed =
        previous_policy_revision.as_deref() != Some(removed_policy_revision.as_str());
    let pending_policy_termination = policy_revision_changed.then(|| {
        runtime
            .subscription_registry()
            .begin_policy_revision_mismatches(table, &removed_policy_revision)
    });

    if let Err(error) = runtime.store().delete_table_schema(table) {
        if let Some(pending) = pending_policy_termination {
            runtime
                .subscription_registry()
                .restore_policy_revision_mismatches(pending);
        }
        return Err(error);
    }
    let journal_progress = match runtime.store().journal_progress() {
        Ok(journal_progress) => journal_progress,
        Err(error) => {
            if let Some(pending) = pending_policy_termination {
                runtime
                    .subscription_registry()
                    .finish_policy_revision_mismatches(pending, POLICY_REVISION_CHANGED_MESSAGE);
            }
            return Err(error);
        }
    };
    stage_assigned_schema_record(runtime, previous_durable_head, journal_progress)?;
    pause_assigned_schema_before_visibility(commit_faults)?;

    let mut schema = previous_schema;
    Arc::make_mut(&mut schema).tables.remove(table);
    runtime.replace_schema_snapshot(schema);

    if let Some(pending) = pending_policy_termination {
        runtime.clear_document_cache();
        runtime
            .subscription_registry()
            .finish_policy_revision_mismatches(pending, POLICY_REVISION_CHANGED_MESSAGE);
    }
    runtime.sync_mutation_journal_progress_in_actor(journal_progress);
    Ok(())
}

fn stage_assigned_schema_record(
    runtime: &TenantRuntime,
    previous_durable_head: nimbus_core::SequenceNumber,
    progress: nimbus_storage::JournalProgress,
) -> Result<()> {
    let at_most_one_local_record = progress.durable_head.0
        <= previous_durable_head.0.saturating_add(1)
        && progress.applied_head == progress.durable_head;
    if runtime.store().has_process_local_sequence_authority()
        && progress.durable_head > previous_durable_head
        && at_most_one_local_record
    {
        let record = runtime
            .store()
            .read_durable_journal_from(progress.durable_head)?
            .into_iter()
            .find(|record| record.sequence == progress.durable_head)
            .ok_or_else(|| {
                Error::Internal(format!(
                    "assigned schema record {} was not readable from the durable journal",
                    progress.durable_head
                ))
            })?;
        runtime.stage_zero_write_record_in_write_log(&record);
    }
    Ok(())
}

fn pause_assigned_schema_before_visibility(commit_faults: &CommitFaultClient) -> Result<()> {
    if !commit_faults.is_armed(labels::SCHEMA_ASSIGNED_BEFORE_VISIBLE) {
        return Ok(());
    }
    commit_faults
        .wait(labels::SCHEMA_ASSIGNED_BEFORE_VISIBLE)
        .into_result()
}

fn effective_policy_revision(table_schema: Option<&TableSchema>) -> Result<String> {
    match table_schema {
        Some(table_schema) => table_schema.access_policy_revision(),
        None => policy_revision_id(None),
    }
}
