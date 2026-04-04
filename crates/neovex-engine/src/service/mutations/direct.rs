use std::{future, sync::Arc};

use neovex_core::{
    AccessAction, CommitEntry, Document, DocumentId, Error, Mutation, PrincipalContext, Result,
    Schema, TableName, TenantId,
};
use neovex_storage::TenantStore;

use crate::{Service, tenant::TenantRuntime};

use super::enforce_mutation_authorization;

#[derive(Clone)]
pub(crate) enum MutationExecutionMode {
    Immediate,
    Scheduled { execution_id: String },
}

pub(crate) enum MutationExecutionResult {
    Immediate(Option<DocumentId>),
    Scheduled(bool),
}

struct UpdateMutationRequest<'a> {
    table: TableName,
    id: DocumentId,
    patch: serde_json::Map<String, serde_json::Value>,
    principal: &'a PrincipalContext,
}

fn expect_immediate_result(
    result: MutationExecutionResult,
    scheduled_message: &'static str,
) -> Option<DocumentId> {
    match result {
        MutationExecutionResult::Immediate(document_id) => document_id,
        MutationExecutionResult::Scheduled(_) => unreachable!("{scheduled_message}"),
    }
}

fn expect_scheduled_applied(
    result: MutationExecutionResult,
    immediate_message: &'static str,
) -> bool {
    match result {
        MutationExecutionResult::Scheduled(applied) => applied,
        MutationExecutionResult::Immediate(_) => unreachable!("{immediate_message}"),
    }
}

fn expect_immediate_document_id(
    document_id: Option<DocumentId>,
    missing_message: &'static str,
) -> Result<DocumentId> {
    document_id.ok_or_else(|| Error::Internal(missing_message.to_string()))
}

fn expect_immediate_unit(
    document_id: Option<DocumentId>,
    unexpected_message: &'static str,
) -> Result<()> {
    match document_id {
        None => Ok(()),
        Some(_) => Err(Error::Internal(unexpected_message.to_string())),
    }
}

impl Service {
    /// Inserts a document and fan-outs any resulting subscription updates.
    pub fn insert_document(
        &self,
        tenant_id: &TenantId,
        table: TableName,
        fields: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DocumentId> {
        self.insert_document_with_principal(
            tenant_id,
            table,
            fields,
            &PrincipalContext::anonymous(),
        )
    }

    /// Inserts a document for the provided principal and fan-outs any resulting subscription updates.
    pub fn insert_document_with_principal(
        &self,
        tenant_id: &TenantId,
        table: TableName,
        fields: serde_json::Map<String, serde_json::Value>,
        principal: &PrincipalContext,
    ) -> Result<DocumentId> {
        self.apply_mutation_with_principal(
            tenant_id,
            Mutation::Insert { table, fields },
            principal,
        )?
        .ok_or_else(|| Error::Internal("insert should return a document id".to_string()))
    }

    /// Inserts a document asynchronously and fan-outs any resulting subscription updates.
    pub async fn insert_document_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        fields: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DocumentId> {
        self.insert_document_async_cancellable(tenant_id, table, fields, future::pending(), || {
            Ok(())
        })
        .await
    }

    /// Inserts a document asynchronously for the provided principal.
    pub async fn insert_document_async_with_principal(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        fields: serde_json::Map<String, serde_json::Value>,
        principal: PrincipalContext,
    ) -> Result<DocumentId> {
        self.insert_document_async_cancellable_with_principal(
            tenant_id,
            table,
            fields,
            principal,
            future::pending(),
            || Ok(()),
        )
        .await
    }

    pub async fn insert_document_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        fields: serde_json::Map<String, serde_json::Value>,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<DocumentId>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.insert_document_async_cancellable_with_principal(
            tenant_id,
            table,
            fields,
            PrincipalContext::anonymous(),
            cancel_wait,
            check_cancel,
        )
        .await
    }

    pub async fn insert_document_async_cancellable_with_principal<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        fields: serde_json::Map<String, serde_json::Value>,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<DocumentId>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let document_id = expect_immediate_result(
            self.apply_mutation_with_mode_async_cancellable(
                tenant_id,
                MutationExecutionMode::Immediate,
                Mutation::Insert { table, fields },
                principal,
                cancel_wait,
                check_cancel,
            )
            .await?,
            "immediate async insert should not produce a scheduled mutation result",
        );
        expect_immediate_document_id(document_id, "insert should return a document id")
    }

    /// Updates a document and fan-outs any resulting subscription updates.
    pub fn update_document(
        &self,
        tenant_id: &TenantId,
        table: TableName,
        document_id: DocumentId,
        patch: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DocumentId> {
        self.update_document_with_principal(
            tenant_id,
            table,
            document_id,
            patch,
            &PrincipalContext::anonymous(),
        )
    }

    /// Updates a document for the provided principal and fan-outs any resulting subscription updates.
    pub fn update_document_with_principal(
        &self,
        tenant_id: &TenantId,
        table: TableName,
        document_id: DocumentId,
        patch: serde_json::Map<String, serde_json::Value>,
        principal: &PrincipalContext,
    ) -> Result<DocumentId> {
        self.apply_mutation_with_principal(
            tenant_id,
            Mutation::Update {
                table,
                id: document_id,
                patch,
            },
            principal,
        )?
        .ok_or_else(|| Error::Internal("update should return a document id".to_string()))
    }

    /// Updates a document asynchronously and fan-outs any resulting subscription updates.
    pub async fn update_document_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        patch: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DocumentId> {
        self.update_document_async_cancellable(
            tenant_id,
            table,
            document_id,
            patch,
            future::pending(),
            || Ok(()),
        )
        .await
    }

    /// Updates a document asynchronously for the provided principal.
    pub async fn update_document_async_with_principal(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        patch: serde_json::Map<String, serde_json::Value>,
        principal: PrincipalContext,
    ) -> Result<DocumentId> {
        self.update_document_async_cancellable_with_principal(
            tenant_id,
            table,
            document_id,
            patch,
            principal,
            future::pending(),
            || Ok(()),
        )
        .await
    }

    pub async fn update_document_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        patch: serde_json::Map<String, serde_json::Value>,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<DocumentId>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.update_document_async_cancellable_with_principal(
            tenant_id,
            table,
            document_id,
            patch,
            PrincipalContext::anonymous(),
            cancel_wait,
            check_cancel,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_document_async_cancellable_with_principal<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        patch: serde_json::Map<String, serde_json::Value>,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<DocumentId>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let document_id = expect_immediate_result(
            self.apply_mutation_with_mode_async_cancellable(
                tenant_id,
                MutationExecutionMode::Immediate,
                Mutation::Update {
                    table,
                    id: document_id,
                    patch,
                },
                principal,
                cancel_wait,
                check_cancel,
            )
            .await?,
            "immediate async update should not produce a scheduled mutation result",
        );
        expect_immediate_document_id(document_id, "update should return a document id")
    }

    /// Deletes a document and fan-outs any resulting subscription updates.
    pub fn delete_document(
        &self,
        tenant_id: &TenantId,
        table: TableName,
        document_id: DocumentId,
    ) -> Result<()> {
        self.delete_document_with_principal(
            tenant_id,
            table,
            document_id,
            &PrincipalContext::anonymous(),
        )?;
        Ok(())
    }

    /// Deletes a document for the provided principal and fan-outs any resulting subscription updates.
    pub fn delete_document_with_principal(
        &self,
        tenant_id: &TenantId,
        table: TableName,
        document_id: DocumentId,
        principal: &PrincipalContext,
    ) -> Result<()> {
        let _ = self.apply_mutation_with_principal(
            tenant_id,
            Mutation::Delete {
                table,
                id: document_id,
            },
            principal,
        )?;
        Ok(())
    }

    /// Deletes a document asynchronously and fan-outs any resulting subscription updates.
    pub async fn delete_document_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
    ) -> Result<()> {
        self.delete_document_async_cancellable(
            tenant_id,
            table,
            document_id,
            future::pending(),
            || Ok(()),
        )
        .await
    }

    /// Deletes a document asynchronously for the provided principal.
    pub async fn delete_document_async_with_principal(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        principal: PrincipalContext,
    ) -> Result<()> {
        self.delete_document_async_cancellable_with_principal(
            tenant_id,
            table,
            document_id,
            principal,
            future::pending(),
            || Ok(()),
        )
        .await
    }

    pub async fn delete_document_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<()>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.delete_document_async_cancellable_with_principal(
            tenant_id,
            table,
            document_id,
            PrincipalContext::anonymous(),
            cancel_wait,
            check_cancel,
        )
        .await
    }

    pub async fn delete_document_async_cancellable_with_principal<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<()>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let document_id = expect_immediate_result(
            self.apply_mutation_with_mode_async_cancellable(
                tenant_id,
                MutationExecutionMode::Immediate,
                Mutation::Delete {
                    table,
                    id: document_id,
                },
                principal,
                cancel_wait,
                check_cancel,
            )
            .await?,
            "immediate async delete should not produce a scheduled mutation result",
        );
        expect_immediate_unit(document_id, "delete should not return a document id")
    }

    fn run_store_mutation<F>(&self, runtime: Arc<TenantRuntime>, mutate: F) -> Result<CommitEntry>
    where
        F: FnOnce(&TenantStore) -> Result<CommitEntry>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            mutate(&runtime.store)?
        };
        runtime.mark_durable_head(commit.sequence);
        runtime.invalidate_document_cache_for_commit(&commit);
        runtime.mark_applied_head(commit.sequence);
        self.process_commit(runtime, &commit);
        Ok(commit)
    }

    fn run_store_mutation_once<F>(&self, runtime: Arc<TenantRuntime>, mutate: F) -> Result<bool>
    where
        F: FnOnce(&TenantStore) -> Result<Option<CommitEntry>>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            mutate(&runtime.store)?
        };
        let Some(commit) = commit else {
            return Ok(false);
        };
        runtime.mark_durable_head(commit.sequence);
        runtime.invalidate_document_cache_for_commit(&commit);
        runtime.mark_applied_head(commit.sequence);
        self.process_commit(runtime, &commit);
        Ok(true)
    }

    fn run_store_delete_mutation<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mutate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&TenantStore) -> Result<(CommitEntry, Document)>,
    {
        let (commit, _deleted_document) = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            mutate(&runtime.store)?
        };
        runtime.mark_durable_head(commit.sequence);
        runtime.invalidate_document_cache_for_commit(&commit);
        runtime.mark_applied_head(commit.sequence);
        self.process_commit(runtime, &commit);
        Ok(commit)
    }

    fn run_store_delete_mutation_once<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        mutate: F,
    ) -> Result<bool>
    where
        F: FnOnce(&TenantStore) -> Result<Option<(CommitEntry, Document)>>,
    {
        let commit = {
            let _sequence_guard = runtime.lock_mutation_sequence();
            mutate(&runtime.store)?
        };
        let Some((commit, _deleted_document)) = commit else {
            return Ok(false);
        };
        runtime.mark_durable_head(commit.sequence);
        runtime.invalidate_document_cache_for_commit(&commit);
        runtime.mark_applied_head(commit.sequence);
        self.process_commit(runtime, &commit);
        Ok(true)
    }

    fn apply_mutation_with_mode(
        &self,
        tenant_id: &TenantId,
        mode: MutationExecutionMode,
        mutation: Mutation,
        principal: &PrincipalContext,
    ) -> Result<MutationExecutionResult> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let schema = runtime.schema();

        match mutation {
            Mutation::Insert { table, fields } => {
                self.apply_insert_like(runtime.clone(), &schema, mode, table, fields, principal)
            }
            Mutation::Update { table, id, patch } => self.apply_update_like(
                runtime.clone(),
                &schema,
                mode,
                UpdateMutationRequest {
                    table,
                    id,
                    patch,
                    principal,
                },
            ),
            Mutation::Delete { table, id } => {
                self.apply_delete_like(runtime.clone(), &schema, mode, table, id, principal)
            }
        }
    }

    fn apply_mutation_with_principal(
        &self,
        tenant_id: &TenantId,
        mutation: Mutation,
        principal: &PrincipalContext,
    ) -> Result<Option<DocumentId>> {
        Ok(expect_immediate_result(
            self.apply_mutation_with_mode(
                tenant_id,
                MutationExecutionMode::Immediate,
                mutation,
                principal,
            )?,
            "immediate mutation execution should not return a scheduled result",
        ))
    }

    async fn apply_mutation_with_mode_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        mode: MutationExecutionMode,
        mutation: Mutation,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<MutationExecutionResult>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        check_cancel()?;
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        self.submit_journaled_async_mutation(
            runtime,
            &tenant_id,
            mode,
            mutation,
            principal,
            cancel_wait,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) fn execute_scheduled_mutation(
        &self,
        tenant_id: &TenantId,
        execution_id: &str,
        mutation: Mutation,
    ) -> Result<bool> {
        Ok(expect_scheduled_applied(
            self.apply_mutation_with_mode(
                tenant_id,
                MutationExecutionMode::Scheduled {
                    execution_id: execution_id.to_string(),
                },
                mutation,
                &PrincipalContext::anonymous(),
            )?,
            "scheduled mutation execution should not return an immediate result",
        ))
    }

    fn apply_insert_like(
        &self,
        runtime: Arc<TenantRuntime>,
        schema: &Schema,
        mode: MutationExecutionMode,
        table: TableName,
        fields: serde_json::Map<String, serde_json::Value>,
        principal: &PrincipalContext,
    ) -> Result<MutationExecutionResult> {
        let table_schema = schema.get_table(&table).cloned();
        let indexes = table_schema
            .as_ref()
            .map(|table_schema| {
                table_schema.validate(&fields)?;
                Ok(table_schema.indexes.clone())
            })
            .transpose()?
            .unwrap_or_default();
        let document = Document::new(table, fields);
        enforce_mutation_authorization(
            table_schema.as_ref(),
            AccessAction::Create,
            principal,
            Some(&document),
            None,
        )?;
        let document_id = document.id;

        match mode {
            MutationExecutionMode::Immediate => {
                if indexes.is_empty() {
                    self.run_store_mutation(runtime, |store| store.insert(&document))?;
                } else {
                    self.run_store_mutation(runtime, |store| {
                        store.insert_with_indexes(&document, &indexes)
                    })?;
                }
                Ok(MutationExecutionResult::Immediate(Some(document_id)))
            }
            MutationExecutionMode::Scheduled { execution_id } => {
                let applied = if indexes.is_empty() {
                    self.run_store_mutation_once(runtime, |store| {
                        store.insert_once(&document, Some(execution_id.as_str()))
                    })?
                } else {
                    self.run_store_mutation_once(runtime, |store| {
                        store.insert_with_indexes_once(
                            &document,
                            &indexes,
                            Some(execution_id.as_str()),
                        )
                    })?
                };
                Ok(MutationExecutionResult::Scheduled(applied))
            }
        }
    }

    fn apply_update_like(
        &self,
        runtime: Arc<TenantRuntime>,
        schema: &Schema,
        mode: MutationExecutionMode,
        request: UpdateMutationRequest<'_>,
    ) -> Result<MutationExecutionResult> {
        let UpdateMutationRequest {
            table,
            id,
            patch,
            principal,
        } = request;
        match schema.get_table(&table).cloned() {
            Some(table_schema) if table_schema.indexes.is_empty() => match mode {
                MutationExecutionMode::Immediate => {
                    let authorization_schema = table_schema.clone();
                    let principal = principal.clone();
                    self.run_store_mutation(runtime, move |store| {
                        store.update_validated(&table, &id, &patch, |existing, document| {
                            table_schema.validate(&document.fields)?;
                            enforce_mutation_authorization(
                                Some(&authorization_schema),
                                AccessAction::Update,
                                &principal,
                                Some(document),
                                Some(existing),
                            )
                        })
                    })?;
                    Ok(MutationExecutionResult::Immediate(Some(id)))
                }
                MutationExecutionMode::Scheduled { execution_id } => {
                    let authorization_schema = table_schema.clone();
                    let principal = principal.clone();
                    let applied = self.run_store_mutation_once(runtime, move |store| {
                        store.update_validated_once(
                            &table,
                            &id,
                            &patch,
                            Some(execution_id.as_str()),
                            |existing, document| {
                                table_schema.validate(&document.fields)?;
                                enforce_mutation_authorization(
                                    Some(&authorization_schema),
                                    AccessAction::Update,
                                    &principal,
                                    Some(document),
                                    Some(existing),
                                )
                            },
                        )
                    })?;
                    Ok(MutationExecutionResult::Scheduled(applied))
                }
            },
            Some(table_schema) => {
                let indexes = table_schema.indexes.clone();
                match mode {
                    MutationExecutionMode::Immediate => {
                        let authorization_schema = table_schema.clone();
                        let principal = principal.clone();
                        self.run_store_mutation(runtime, move |store| {
                            store.update_with_indexes_validated(
                                &table,
                                &id,
                                &patch,
                                &indexes,
                                |existing, document| {
                                    table_schema.validate(&document.fields)?;
                                    enforce_mutation_authorization(
                                        Some(&authorization_schema),
                                        AccessAction::Update,
                                        &principal,
                                        Some(document),
                                        Some(existing),
                                    )
                                },
                            )
                        })?;
                        Ok(MutationExecutionResult::Immediate(Some(id)))
                    }
                    MutationExecutionMode::Scheduled { execution_id } => {
                        let authorization_schema = table_schema.clone();
                        let principal = principal.clone();
                        let applied = self.run_store_mutation_once(runtime, move |store| {
                            store.update_with_indexes_validated_once(
                                &table,
                                &id,
                                &patch,
                                &indexes,
                                Some(execution_id.as_str()),
                                |existing, document| {
                                    table_schema.validate(&document.fields)?;
                                    enforce_mutation_authorization(
                                        Some(&authorization_schema),
                                        AccessAction::Update,
                                        &principal,
                                        Some(document),
                                        Some(existing),
                                    )
                                },
                            )
                        })?;
                        Ok(MutationExecutionResult::Scheduled(applied))
                    }
                }
            }
            None => match mode {
                MutationExecutionMode::Immediate => {
                    let principal = principal.clone();
                    self.run_store_mutation(runtime, move |store| {
                        store.update_validated(&table, &id, &patch, |existing, document| {
                            enforce_mutation_authorization(
                                None,
                                AccessAction::Update,
                                &principal,
                                Some(document),
                                Some(existing),
                            )
                        })
                    })?;
                    Ok(MutationExecutionResult::Immediate(Some(id)))
                }
                MutationExecutionMode::Scheduled { execution_id } => {
                    let principal = principal.clone();
                    let applied = self.run_store_mutation_once(runtime, move |store| {
                        store.update_validated_once(
                            &table,
                            &id,
                            &patch,
                            Some(execution_id.as_str()),
                            |existing, document| {
                                enforce_mutation_authorization(
                                    None,
                                    AccessAction::Update,
                                    &principal,
                                    Some(document),
                                    Some(existing),
                                )
                            },
                        )
                    })?;
                    Ok(MutationExecutionResult::Scheduled(applied))
                }
            },
        }
    }

    fn apply_delete_like(
        &self,
        runtime: Arc<TenantRuntime>,
        schema: &Schema,
        mode: MutationExecutionMode,
        table: TableName,
        id: DocumentId,
        principal: &PrincipalContext,
    ) -> Result<MutationExecutionResult> {
        let table_schema = schema.get_table(&table).cloned();
        let indexes = table_schema
            .as_ref()
            .map(|table_schema| table_schema.indexes.clone())
            .unwrap_or_default();

        match mode {
            MutationExecutionMode::Immediate => {
                if indexes.is_empty() {
                    let table_schema = table_schema.clone();
                    let principal = principal.clone();
                    self.run_store_delete_mutation(runtime, |store| {
                        store.delete_validated_returning_document(&table, &id, |existing| {
                            enforce_mutation_authorization(
                                table_schema.as_ref(),
                                AccessAction::Delete,
                                &principal,
                                None,
                                Some(existing),
                            )
                        })
                    })?;
                } else {
                    let table_schema = table_schema.clone();
                    let principal = principal.clone();
                    self.run_store_delete_mutation(runtime, |store| {
                        store.delete_with_indexes_validated_returning_document(
                            &table,
                            &id,
                            &indexes,
                            |existing| {
                                enforce_mutation_authorization(
                                    table_schema.as_ref(),
                                    AccessAction::Delete,
                                    &principal,
                                    None,
                                    Some(existing),
                                )
                            },
                        )
                    })?;
                }
                Ok(MutationExecutionResult::Immediate(None))
            }
            MutationExecutionMode::Scheduled { execution_id } => {
                let applied = if indexes.is_empty() {
                    let table_schema = table_schema.clone();
                    let principal = principal.clone();
                    self.run_store_delete_mutation_once(runtime, |store| {
                        store.delete_validated_once(
                            &table,
                            &id,
                            Some(execution_id.as_str()),
                            |existing| {
                                enforce_mutation_authorization(
                                    table_schema.as_ref(),
                                    AccessAction::Delete,
                                    &principal,
                                    None,
                                    Some(existing),
                                )
                            },
                        )
                    })?
                } else {
                    let table_schema = table_schema.clone();
                    let principal = principal.clone();
                    self.run_store_delete_mutation_once(runtime, |store| {
                        store.delete_with_indexes_validated_once(
                            &table,
                            &id,
                            &indexes,
                            Some(execution_id.as_str()),
                            |existing| {
                                enforce_mutation_authorization(
                                    table_schema.as_ref(),
                                    AccessAction::Delete,
                                    &principal,
                                    None,
                                    Some(existing),
                                )
                            },
                        )
                    })?
                };
                Ok(MutationExecutionResult::Scheduled(applied))
            }
        }
    }

    pub(crate) async fn execute_scheduled_mutation_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        execution_id: String,
        mutation: Mutation,
    ) -> Result<bool> {
        self.execute_scheduled_mutation_async_cancellable(
            tenant_id,
            execution_id,
            mutation,
            future::pending(),
            || Ok(()),
        )
        .await
    }

    pub(crate) async fn execute_scheduled_mutation_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        execution_id: String,
        mutation: Mutation,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<bool>
    where
        Fut: future::Future<Output = ()> + Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        Ok(expect_scheduled_applied(
            self.apply_mutation_with_mode_async_cancellable(
                tenant_id,
                MutationExecutionMode::Scheduled { execution_id },
                mutation,
                PrincipalContext::anonymous(),
                cancel_wait,
                check_cancel,
            )
            .await?,
            "scheduled async mutation execution should not return an immediate result",
        ))
    }
}
