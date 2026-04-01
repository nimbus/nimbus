use std::sync::Arc;

use neovex_core::{
    CommitEntry, Document, DocumentId, Error, Mutation, Result, Schema, TableName, TenantId,
};
use neovex_storage::TenantStore;
use tracing::warn;

use crate::subscriptions::SubscriptionUpdate;
use crate::tenant::TenantRuntime;

use super::{Service, documents_to_json, queries::evaluate_with_index};

#[derive(Clone, Copy)]
enum MutationExecutionMode<'a> {
    Immediate,
    Scheduled { execution_id: &'a str },
}

enum MutationExecutionResult {
    Immediate(Option<DocumentId>),
    Scheduled(bool),
}

impl Service {
    /// Inserts a document and fan-outs any resulting subscription updates.
    pub fn insert_document(
        &self,
        tenant_id: &TenantId,
        table: TableName,
        fields: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DocumentId> {
        self.apply_mutation(tenant_id, Mutation::Insert { table, fields })?
            .ok_or_else(|| Error::Internal("insert should return a document id".to_string()))
    }

    /// Inserts a document asynchronously and fan-outs any resulting subscription updates.
    pub async fn insert_document_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        fields: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DocumentId> {
        self.call_blocking(move |service| service.insert_document(&tenant_id, table, fields))
            .await
    }

    /// Updates a document and fan-outs any resulting subscription updates.
    pub fn update_document(
        &self,
        tenant_id: &TenantId,
        table: TableName,
        document_id: DocumentId,
        patch: serde_json::Map<String, serde_json::Value>,
    ) -> Result<DocumentId> {
        self.apply_mutation(
            tenant_id,
            Mutation::Update {
                table,
                id: document_id,
                patch,
            },
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
        self.call_blocking(move |service| {
            service.update_document(&tenant_id, table, document_id, patch)
        })
        .await
    }

    /// Deletes a document and fan-outs any resulting subscription updates.
    pub fn delete_document(
        &self,
        tenant_id: &TenantId,
        table: TableName,
        document_id: DocumentId,
    ) -> Result<()> {
        let _ = self.apply_mutation(
            tenant_id,
            Mutation::Delete {
                table,
                id: document_id,
            },
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
        self.call_blocking(move |service| service.delete_document(&tenant_id, table, document_id))
            .await
    }

    fn process_commit(
        &self,
        runtime: Arc<TenantRuntime>,
        commit: &CommitEntry,
        candidate_documents: &[Document],
        deleted_documents: &[Document],
    ) {
        runtime.invalidate_document_cache_for_commit(commit);
        let affected = runtime.subscriptions.affected(commit, candidate_documents);
        let mut failed = Vec::new();
        for subscription in affected {
            match evaluate_with_index(&runtime, &subscription.query) {
                Ok(documents) => {
                    let update = SubscriptionUpdate::Result {
                        subscription_id: subscription.id,
                        request_id: None,
                        commit: Some(commit.clone()),
                        deleted_documents: deleted_documents.to_vec(),
                        data: documents_to_json(documents),
                    };
                    if subscription.sender.send(update).is_err() {
                        failed.push(subscription.id);
                    }
                }
                Err(error) => {
                    warn!(
                        subscription_id = subscription.id,
                        error = %error,
                        "subscription re-evaluation failed"
                    );
                    if subscription
                        .sender
                        .send(SubscriptionUpdate::Error {
                            subscription_id: subscription.id,
                            request_id: None,
                            message: error.to_string(),
                        })
                        .is_err()
                    {
                        failed.push(subscription.id);
                    }
                }
            }
        }

        for subscription_id in failed {
            runtime.subscriptions.remove(subscription_id);
        }
    }

    fn run_store_mutation<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        candidate_documents: &[Document],
        mutate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&TenantStore) -> Result<CommitEntry>,
    {
        let commit = mutate(&runtime.store)?;
        self.process_commit(runtime, &commit, candidate_documents, &[]);
        Ok(commit)
    }

    fn run_store_mutation_once<F>(
        &self,
        runtime: Arc<TenantRuntime>,
        candidate_documents: &[Document],
        mutate: F,
    ) -> Result<bool>
    where
        F: FnOnce(&TenantStore) -> Result<Option<CommitEntry>>,
    {
        let Some(commit) = mutate(&runtime.store)? else {
            return Ok(false);
        };
        self.process_commit(runtime, &commit, candidate_documents, &[]);
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
        let (commit, deleted_document) = mutate(&runtime.store)?;
        self.process_commit(
            runtime,
            &commit,
            std::slice::from_ref(&deleted_document),
            std::slice::from_ref(&deleted_document),
        );
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
        let Some((commit, deleted_document)) = mutate(&runtime.store)? else {
            return Ok(false);
        };
        self.process_commit(
            runtime,
            &commit,
            std::slice::from_ref(&deleted_document),
            std::slice::from_ref(&deleted_document),
        );
        Ok(true)
    }

    fn apply_mutation_with_mode(
        &self,
        tenant_id: &TenantId,
        mode: MutationExecutionMode<'_>,
        mutation: Mutation,
    ) -> Result<MutationExecutionResult> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let schema = runtime.schema();

        match mutation {
            Mutation::Insert { table, fields } => {
                self.apply_insert_like(runtime.clone(), &schema, mode, table, fields)
            }
            Mutation::Update { table, id, patch } => {
                self.apply_update_like(runtime.clone(), &schema, mode, table, id, patch)
            }
            Mutation::Delete { table, id } => {
                self.apply_delete_like(runtime.clone(), &schema, mode, table, id)
            }
        }
    }

    fn apply_mutation(
        &self,
        tenant_id: &TenantId,
        mutation: Mutation,
    ) -> Result<Option<DocumentId>> {
        match self.apply_mutation_with_mode(
            tenant_id,
            MutationExecutionMode::Immediate,
            mutation,
        )? {
            MutationExecutionResult::Immediate(document_id) => Ok(document_id),
            MutationExecutionResult::Scheduled(_) => {
                unreachable!("immediate mutation execution should not return a scheduled result")
            }
        }
    }

    pub(crate) fn execute_scheduled_mutation(
        &self,
        tenant_id: &TenantId,
        execution_id: &str,
        mutation: Mutation,
    ) -> Result<bool> {
        match self.apply_mutation_with_mode(
            tenant_id,
            MutationExecutionMode::Scheduled { execution_id },
            mutation,
        )? {
            MutationExecutionResult::Scheduled(applied) => Ok(applied),
            MutationExecutionResult::Immediate(_) => {
                unreachable!("scheduled mutation execution should not return an immediate result")
            }
        }
    }

    fn apply_insert_like(
        &self,
        runtime: Arc<TenantRuntime>,
        schema: &Schema,
        mode: MutationExecutionMode<'_>,
        table: TableName,
        fields: serde_json::Map<String, serde_json::Value>,
    ) -> Result<MutationExecutionResult> {
        let indexes = schema
            .get_table(&table)
            .map(|table_schema| {
                table_schema.validate(&fields)?;
                Ok(table_schema.indexes.clone())
            })
            .transpose()?
            .unwrap_or_default();
        let document = Document::new(table, fields);
        let document_id = document.id;

        match mode {
            MutationExecutionMode::Immediate => {
                if indexes.is_empty() {
                    self.run_store_mutation(runtime, std::slice::from_ref(&document), |store| {
                        store.insert(&document)
                    })?;
                } else {
                    self.run_store_mutation(runtime, std::slice::from_ref(&document), |store| {
                        store.insert_with_indexes(&document, &indexes)
                    })?;
                }
                Ok(MutationExecutionResult::Immediate(Some(document_id)))
            }
            MutationExecutionMode::Scheduled { execution_id } => {
                let applied = if indexes.is_empty() {
                    self.run_store_mutation_once(
                        runtime,
                        std::slice::from_ref(&document),
                        |store| store.insert_once(&document, Some(execution_id)),
                    )?
                } else {
                    self.run_store_mutation_once(
                        runtime,
                        std::slice::from_ref(&document),
                        |store| {
                            store.insert_with_indexes_once(&document, &indexes, Some(execution_id))
                        },
                    )?
                };
                Ok(MutationExecutionResult::Scheduled(applied))
            }
        }
    }

    fn apply_update_like(
        &self,
        runtime: Arc<TenantRuntime>,
        schema: &Schema,
        mode: MutationExecutionMode<'_>,
        table: TableName,
        id: DocumentId,
        patch: serde_json::Map<String, serde_json::Value>,
    ) -> Result<MutationExecutionResult> {
        match schema.get_table(&table).cloned() {
            Some(table_schema) if table_schema.indexes.is_empty() => match mode {
                MutationExecutionMode::Immediate => {
                    self.run_store_mutation(runtime, &[], move |store| {
                        store.update_validated(&table, &id, &patch, |document| {
                            table_schema.validate(&document.fields)
                        })
                    })?;
                    Ok(MutationExecutionResult::Immediate(Some(id)))
                }
                MutationExecutionMode::Scheduled { execution_id } => {
                    let applied = self.run_store_mutation_once(runtime, &[], move |store| {
                        store.update_validated_once(
                            &table,
                            &id,
                            &patch,
                            Some(execution_id),
                            |document| table_schema.validate(&document.fields),
                        )
                    })?;
                    Ok(MutationExecutionResult::Scheduled(applied))
                }
            },
            Some(table_schema) => {
                let indexes = table_schema.indexes.clone();
                match mode {
                    MutationExecutionMode::Immediate => {
                        self.run_store_mutation(runtime, &[], move |store| {
                            store.update_with_indexes_validated(
                                &table,
                                &id,
                                &patch,
                                &indexes,
                                |document| table_schema.validate(&document.fields),
                            )
                        })?;
                        Ok(MutationExecutionResult::Immediate(Some(id)))
                    }
                    MutationExecutionMode::Scheduled { execution_id } => {
                        let applied = self.run_store_mutation_once(runtime, &[], move |store| {
                            store.update_with_indexes_validated_once(
                                &table,
                                &id,
                                &patch,
                                &indexes,
                                Some(execution_id),
                                |document| table_schema.validate(&document.fields),
                            )
                        })?;
                        Ok(MutationExecutionResult::Scheduled(applied))
                    }
                }
            }
            None => match mode {
                MutationExecutionMode::Immediate => {
                    self.run_store_mutation(runtime, &[], move |store| {
                        store.update(&table, &id, &patch)
                    })?;
                    Ok(MutationExecutionResult::Immediate(Some(id)))
                }
                MutationExecutionMode::Scheduled { execution_id } => {
                    let applied = self.run_store_mutation_once(runtime, &[], move |store| {
                        store.update_validated_once(&table, &id, &patch, Some(execution_id), |_| {
                            Ok(())
                        })
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
        mode: MutationExecutionMode<'_>,
        table: TableName,
        id: DocumentId,
    ) -> Result<MutationExecutionResult> {
        let indexes = schema
            .get_table(&table)
            .map(|table_schema| table_schema.indexes.clone())
            .unwrap_or_default();

        match mode {
            MutationExecutionMode::Immediate => {
                if indexes.is_empty() {
                    self.run_store_delete_mutation(runtime, |store| {
                        store.delete_returning_document(&table, &id)
                    })?;
                } else {
                    self.run_store_delete_mutation(runtime, |store| {
                        store.delete_with_indexes_returning_document(&table, &id, &indexes)
                    })?;
                }
                Ok(MutationExecutionResult::Immediate(None))
            }
            MutationExecutionMode::Scheduled { execution_id } => {
                let applied = if indexes.is_empty() {
                    self.run_store_delete_mutation_once(runtime, |store| {
                        store.delete_once_returning_document(&table, &id, Some(execution_id))
                    })?
                } else {
                    self.run_store_delete_mutation_once(runtime, |store| {
                        store.delete_with_indexes_once_returning_document(
                            &table,
                            &id,
                            &indexes,
                            Some(execution_id),
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
        self.call_blocking(move |service| {
            service.execute_scheduled_mutation(&tenant_id, &execution_id, mutation)
        })
        .await
    }
}
