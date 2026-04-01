use std::sync::Arc;

use neovex_core::{
    AccessAction, AccessRule, CommitEntry, Document, DocumentId, Error, Mutation, PrincipalContext,
    Result, Schema, TableName, TableSchema, TenantId,
};
use neovex_storage::TenantStore;
use tracing::warn;

use crate::subscriptions::SubscriptionUpdate;
use crate::tenant::TenantRuntime;

use super::{Service, documents_to_json, queries::evaluate_with_index_cancellable_for_principal};

#[derive(Clone, Copy)]
enum MutationExecutionMode<'a> {
    Immediate,
    Scheduled { execution_id: &'a str },
}

enum MutationExecutionResult {
    Immediate(Option<DocumentId>),
    Scheduled(bool),
}

struct UpdateMutationRequest<'a> {
    table: TableName,
    id: DocumentId,
    patch: serde_json::Map<String, serde_json::Value>,
    principal: &'a PrincipalContext,
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
        self.call_blocking(move |service| service.insert_document(&tenant_id, table, fields))
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
        self.call_blocking(move |service| {
            service.insert_document_with_principal(&tenant_id, table, fields, &principal)
        })
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
        self.call_blocking(move |service| {
            service.update_document(&tenant_id, table, document_id, patch)
        })
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
        self.call_blocking(move |service| {
            service.update_document_with_principal(
                &tenant_id,
                table,
                document_id,
                patch,
                &principal,
            )
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
        self.call_blocking(move |service| service.delete_document(&tenant_id, table, document_id))
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
        self.call_blocking(move |service| {
            service.delete_document_with_principal(&tenant_id, table, document_id, &principal)
        })
        .await
    }

    pub(crate) fn process_commit(
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
            let mut check_cancel = || Ok(());
            match evaluate_with_index_cancellable_for_principal(
                &runtime,
                &subscription.query,
                &subscription.principal,
                &mut check_cancel,
            ) {
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
        match self.apply_mutation_with_mode(
            tenant_id,
            MutationExecutionMode::Immediate,
            mutation,
            principal,
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
            &PrincipalContext::anonymous(),
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
                    self.run_store_mutation(runtime, &[], move |store| {
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
                    let applied = self.run_store_mutation_once(runtime, &[], move |store| {
                        store.update_validated_once(
                            &table,
                            &id,
                            &patch,
                            Some(execution_id),
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
                        self.run_store_mutation(runtime, &[], move |store| {
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
                        let applied = self.run_store_mutation_once(runtime, &[], move |store| {
                            store.update_with_indexes_validated_once(
                                &table,
                                &id,
                                &patch,
                                &indexes,
                                Some(execution_id),
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
                    self.run_store_mutation(runtime, &[], move |store| {
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
                    let applied = self.run_store_mutation_once(runtime, &[], move |store| {
                        store.update_validated_once(
                            &table,
                            &id,
                            &patch,
                            Some(execution_id),
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
        mode: MutationExecutionMode<'_>,
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
                        store.delete_validated_once(&table, &id, Some(execution_id), |existing| {
                            enforce_mutation_authorization(
                                table_schema.as_ref(),
                                AccessAction::Delete,
                                &principal,
                                None,
                                Some(existing),
                            )
                        })
                    })?
                } else {
                    let table_schema = table_schema.clone();
                    let principal = principal.clone();
                    self.run_store_delete_mutation_once(runtime, |store| {
                        store.delete_with_indexes_validated_once(
                            &table,
                            &id,
                            &indexes,
                            Some(execution_id),
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
        self.call_blocking(move |service| {
            service.execute_scheduled_mutation(&tenant_id, &execution_id, mutation)
        })
        .await
    }
}

fn mutation_access_rule(
    table_schema: Option<&TableSchema>,
    action: AccessAction,
) -> Option<&AccessRule> {
    table_schema
        .and_then(|table_schema| table_schema.access_policy.as_ref())
        .map(|policy| policy.rule_for(action))
        .filter(|rule| !rule.is_unrestricted())
}

pub(crate) fn enforce_mutation_authorization(
    table_schema: Option<&TableSchema>,
    action: AccessAction,
    principal: &PrincipalContext,
    candidate_document: Option<&Document>,
    existing_document: Option<&Document>,
) -> Result<()> {
    let Some(rule) = mutation_access_rule(table_schema, action) else {
        return Ok(());
    };

    if rule.allows(principal, candidate_document, existing_document)? {
        return Ok(());
    }

    Err(Error::PermissionDenied(match action {
        AccessAction::Create => "create access denied".to_string(),
        AccessAction::Update => "update access denied".to_string(),
        AccessAction::Delete => "delete access denied".to_string(),
        AccessAction::Read => "read access denied".to_string(),
    }))
}
