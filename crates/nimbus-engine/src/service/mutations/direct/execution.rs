use std::{future, sync::Arc};

use nimbus_core::{
    AccessAction, Document, DocumentId, Mutation, PrincipalContext, Result, Schema, TableName,
    TenantId,
};

use crate::service::tenants::with_tenant_runtime_operation;
use crate::{Service, tenant::TenantRuntime};

use super::super::enforce_mutation_authorization;
use super::types::{MutationExecutionMode, MutationExecutionResult, UpdateMutationRequest};

impl Service {
    pub(super) fn apply_mutation_with_mode(
        &self,
        tenant_id: &TenantId,
        mode: MutationExecutionMode,
        mutation: Mutation,
        principal: &PrincipalContext,
    ) -> Result<MutationExecutionResult> {
        with_tenant_runtime_operation(self.get_existing_tenant(tenant_id)?, tenant_id, |runtime| {
            let schema = runtime.schema();
            match mutation {
                Mutation::Insert { table, id, fields } => self.apply_insert_like(
                    runtime.clone(),
                    &schema,
                    mode,
                    table,
                    id,
                    fields,
                    principal,
                ),
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
        })
    }

    pub(super) fn apply_mutation_with_principal(
        &self,
        tenant_id: &TenantId,
        mutation: Mutation,
        principal: &PrincipalContext,
    ) -> Result<Option<DocumentId>> {
        Ok(super::types::expect_immediate_result(
            self.apply_mutation_with_mode(
                tenant_id,
                MutationExecutionMode::Immediate,
                mutation,
                principal,
            )?,
            "immediate mutation execution should not return a scheduled result",
        ))
    }

    pub(super) async fn apply_mutation_with_mode_async_cancellable<Fut, Check>(
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

    #[expect(
        clippy::too_many_arguments,
        reason = "insert execution now threads the optional caller-provided document key through the shared mutation path"
    )]
    fn apply_insert_like(
        &self,
        runtime: Arc<TenantRuntime>,
        schema: &Schema,
        mode: MutationExecutionMode,
        table: TableName,
        document_id: Option<DocumentId>,
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
        let document = match document_id {
            Some(document_id) => Document::with_id(document_id, table, fields),
            None => Document::new(table, fields),
        };
        enforce_mutation_authorization(
            table_schema.as_ref(),
            AccessAction::Create,
            principal,
            Some(&document),
            None,
        )?;
        let document_id = document.id.clone();

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
                    let document_id = id.clone();
                    self.run_store_mutation(runtime, move |store| {
                        store.update_validated(
                            &table,
                            &document_id,
                            &patch,
                            move |existing, document| {
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
                        store.update_validated_once(
                            &table,
                            &id,
                            &patch,
                            Some(execution_id.as_str()),
                            move |existing, document| {
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
                        let document_id = id.clone();
                        self.run_store_mutation(runtime, move |store| {
                            store.update_with_indexes_validated(
                                &table,
                                &document_id,
                                &patch,
                                &indexes,
                                move |existing, document| {
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
                                move |existing, document| {
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
                    let document_id = id.clone();
                    self.run_store_mutation(runtime, move |store| {
                        store.update_validated(
                            &table,
                            &document_id,
                            &patch,
                            move |existing, document| {
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
                            move |existing, document| {
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
                    self.run_store_delete_mutation(runtime, move |store| {
                        store.delete_validated_returning_document(&table, &id, move |existing| {
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
                    self.run_store_delete_mutation(runtime, move |store| {
                        store.delete_with_indexes_validated_returning_document(
                            &table,
                            &id,
                            &indexes,
                            move |existing| {
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
                    self.run_store_delete_mutation_once(runtime, move |store| {
                        store.delete_validated_once(
                            &table,
                            &id,
                            Some(execution_id.as_str()),
                            move |existing| {
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
                    self.run_store_delete_mutation_once(runtime, move |store| {
                        store.delete_with_indexes_validated_once(
                            &table,
                            &id,
                            &indexes,
                            Some(execution_id.as_str()),
                            move |existing| {
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
}
