use std::sync::Arc;

use neovex_core::{
    CommitEntry, Document, DocumentId, Error, Mutation, Result, TableName, TenantId,
};
use neovex_storage::TenantStore;
use tracing::warn;

use crate::subscriptions::SubscriptionUpdate;
use crate::tenant::TenantRuntime;

use super::{Service, documents_to_json, queries::evaluate_with_index};

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
        deleted_documents: &[Document],
    ) {
        let affected = runtime.subscriptions.affected(&commit.affected_tables());
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

    fn run_store_mutation<F>(&self, runtime: Arc<TenantRuntime>, mutate: F) -> Result<CommitEntry>
    where
        F: FnOnce(&TenantStore) -> Result<CommitEntry>,
    {
        let commit = mutate(&runtime.store)?;
        self.process_commit(runtime, &commit, &[]);
        Ok(commit)
    }

    fn run_store_mutation_once<F>(&self, runtime: Arc<TenantRuntime>, mutate: F) -> Result<bool>
    where
        F: FnOnce(&TenantStore) -> Result<Option<CommitEntry>>,
    {
        let Some(commit) = mutate(&runtime.store)? else {
            return Ok(false);
        };
        self.process_commit(runtime, &commit, &[]);
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
        self.process_commit(runtime, &commit, &[deleted_document]);
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
        self.process_commit(runtime, &commit, &[deleted_document]);
        Ok(true)
    }

    fn apply_mutation(
        &self,
        tenant_id: &TenantId,
        mutation: Mutation,
    ) -> Result<Option<DocumentId>> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let schema = runtime.schema();

        match mutation {
            Mutation::Insert { table, fields } => {
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
                if indexes.is_empty() {
                    self.run_store_mutation(runtime.clone(), |store| store.insert(&document))?;
                } else {
                    self.run_store_mutation(runtime.clone(), |store| {
                        store.insert_with_indexes(&document, &indexes)
                    })?;
                }
                Ok(Some(document_id))
            }
            Mutation::Update { table, id, patch } => {
                if let Some(table_schema) = schema.get_table(&table).cloned() {
                    if table_schema.indexes.is_empty() {
                        self.run_store_mutation(runtime.clone(), move |store| {
                            store.update_validated(&table, &id, &patch, |document| {
                                table_schema.validate(&document.fields)
                            })
                        })?;
                    } else {
                        let indexes = table_schema.indexes.clone();
                        self.run_store_mutation(runtime.clone(), move |store| {
                            store.update_with_indexes_validated(
                                &table,
                                &id,
                                &patch,
                                &indexes,
                                |document| table_schema.validate(&document.fields),
                            )
                        })?;
                    }
                } else {
                    self.run_store_mutation(runtime.clone(), move |store| {
                        store.update(&table, &id, &patch)
                    })?;
                }
                Ok(Some(id))
            }
            Mutation::Delete { table, id } => {
                let indexes = schema
                    .get_table(&table)
                    .map(|table_schema| table_schema.indexes.clone())
                    .unwrap_or_default();
                if indexes.is_empty() {
                    self.run_store_delete_mutation(runtime.clone(), |store| {
                        store.delete_returning_document(&table, &id)
                    })?;
                } else {
                    self.run_store_delete_mutation(runtime.clone(), |store| {
                        store.delete_with_indexes_returning_document(&table, &id, &indexes)
                    })?;
                }
                Ok(None)
            }
        }
    }

    pub(crate) fn execute_scheduled_mutation(
        &self,
        tenant_id: &TenantId,
        execution_id: &str,
        mutation: Mutation,
    ) -> Result<bool> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let _operation = runtime.enter_operation(tenant_id)?;
        let schema = runtime.schema();

        match mutation {
            Mutation::Insert { table, fields } => {
                let indexes = schema
                    .get_table(&table)
                    .map(|table_schema| {
                        table_schema.validate(&fields)?;
                        Ok(table_schema.indexes.clone())
                    })
                    .transpose()?
                    .unwrap_or_default();
                let document = Document::new(table, fields);
                if indexes.is_empty() {
                    self.run_store_mutation_once(runtime.clone(), |store| {
                        store.insert_once(&document, Some(execution_id))
                    })
                } else {
                    self.run_store_mutation_once(runtime.clone(), |store| {
                        store.insert_with_indexes_once(&document, &indexes, Some(execution_id))
                    })
                }
            }
            Mutation::Update { table, id, patch } => {
                if let Some(table_schema) = schema.get_table(&table).cloned() {
                    if table_schema.indexes.is_empty() {
                        self.run_store_mutation_once(runtime.clone(), move |store| {
                            store.update_validated_once(
                                &table,
                                &id,
                                &patch,
                                Some(execution_id),
                                |document| table_schema.validate(&document.fields),
                            )
                        })
                    } else {
                        let indexes = table_schema.indexes.clone();
                        self.run_store_mutation_once(runtime.clone(), move |store| {
                            store.update_with_indexes_validated_once(
                                &table,
                                &id,
                                &patch,
                                &indexes,
                                Some(execution_id),
                                |document| table_schema.validate(&document.fields),
                            )
                        })
                    }
                } else {
                    self.run_store_mutation_once(runtime.clone(), move |store| {
                        store.update_validated_once(&table, &id, &patch, Some(execution_id), |_| {
                            Ok(())
                        })
                    })
                }
            }
            Mutation::Delete { table, id } => {
                let indexes = schema
                    .get_table(&table)
                    .map(|table_schema| table_schema.indexes.clone())
                    .unwrap_or_default();
                if indexes.is_empty() {
                    self.run_store_delete_mutation_once(runtime.clone(), |store| {
                        store.delete_once_returning_document(&table, &id, Some(execution_id))
                    })
                } else {
                    self.run_store_delete_mutation_once(runtime.clone(), |store| {
                        store.delete_with_indexes_once_returning_document(
                            &table,
                            &id,
                            &indexes,
                            Some(execution_id),
                        )
                    })
                }
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
