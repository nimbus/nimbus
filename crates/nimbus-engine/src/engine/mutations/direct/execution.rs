use std::{future, sync::Arc, time::Instant};

use nimbus_core::{
    AccessAction, Document, DocumentId, Mutation, PrincipalContext, Result, Schema, TableName,
    TenantId, Timestamp,
};

use crate::engine::tenants::with_tenant_runtime_operation;
use crate::{Engine, tenant::TenantRuntime};

use super::super::caps::check_mutation_caps;
use super::super::enforce_mutation_authorization;
use super::super::prepared::PreparedCommit;
use super::store::DirectMutationProfile;
use super::types::{MutationExecutionMode, MutationExecutionResult, UpdateMutationRequest};

impl Engine {
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
        super::types::expect_immediate_result(
            self.apply_mutation_with_mode(
                tenant_id,
                MutationExecutionMode::Immediate,
                mutation,
                principal,
            )?,
            "immediate mutation execution should not return a scheduled result",
        )
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
        let commit_started_at = Instant::now();
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
            Some(document_id) => Document::with_id_at(document_id, table, fields, Timestamp(0)),
            None => Document::with_id_at(self.next_document_id(), table, fields, Timestamp(0)),
        };
        enforce_mutation_authorization(
            table_schema.as_ref(),
            AccessAction::Create,
            principal,
            Some(&document),
            None,
        )?;
        let document_id = document.id.clone();
        let scheduled_execution_id = match &mode {
            MutationExecutionMode::Immediate => None,
            MutationExecutionMode::Scheduled { execution_id } => Some(execution_id.as_str()),
        };
        let prepared_commit = PreparedCommit::for_direct_insert(
            runtime.durable_head(),
            document,
            scheduled_execution_id,
        );
        check_mutation_caps(&runtime, prepared_commit.usage())?;
        runtime.check_tenant_write_rate(self.now(), prepared_commit.usage().total_write_bytes())?;
        let profile = DirectMutationProfile::after_prepare(commit_started_at);

        match mode {
            MutationExecutionMode::Immediate => {
                self.run_store_mutation(
                    runtime,
                    prepared_commit,
                    profile,
                    |store, prepared, timestamp| {
                        store
                            .insert_with_indexes_once_at(
                                prepared.direct_insert_document()?,
                                direct_write_assignment(&indexes, None, timestamp),
                            )?
                            .ok_or_else(|| {
                                nimbus_core::Error::Internal(
                                    "direct insert should produce a commit".to_string(),
                                )
                            })
                    },
                )?;
                Ok(MutationExecutionResult::Immediate(Some(document_id)))
            }
            MutationExecutionMode::Scheduled { execution_id } => {
                let applied = self.run_store_mutation_once(
                    runtime,
                    prepared_commit,
                    profile,
                    |store, prepared, timestamp| {
                        store.insert_with_indexes_once_at(
                            prepared.direct_insert_document()?,
                            direct_write_assignment(
                                &indexes,
                                Some(execution_id.as_str()),
                                timestamp,
                            ),
                        )
                    },
                )?;
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
        let commit_started_at = Instant::now();
        let UpdateMutationRequest {
            table,
            id,
            patch,
            principal,
        } = request;
        let result_document_id = id.clone();
        let table_schema = schema.get_table(&table).cloned();
        let scheduled_execution_id = match &mode {
            MutationExecutionMode::Immediate => None,
            MutationExecutionMode::Scheduled { execution_id } => Some(execution_id.as_str()),
        };
        let prepared_commit = PreparedCommit::for_direct_update(
            runtime.durable_head(),
            table,
            id,
            patch,
            scheduled_execution_id,
        );
        check_mutation_caps(&runtime, prepared_commit.usage())?;
        runtime.check_tenant_write_rate(self.now(), prepared_commit.usage().total_write_bytes())?;
        let profile = DirectMutationProfile::after_prepare(commit_started_at);
        match table_schema {
            Some(table_schema) if table_schema.indexes.is_empty() => match mode {
                MutationExecutionMode::Immediate => {
                    let authorization_schema = table_schema.clone();
                    let principal = principal.clone();
                    self.run_store_mutation(
                        runtime,
                        prepared_commit,
                        profile,
                        move |store, prepared, timestamp| {
                            let (table, document_id, patch) = prepared.direct_update_parts()?;
                            store
                                .update_with_indexes_validated_once_at(
                                    table,
                                    document_id,
                                    patch,
                                    direct_write_assignment(&[], None, timestamp),
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
                                )?
                                .ok_or_else(|| {
                                    nimbus_core::Error::Internal(
                                        "direct update should produce a commit".to_string(),
                                    )
                                })
                        },
                    )?;
                    Ok(MutationExecutionResult::Immediate(Some(result_document_id)))
                }
                MutationExecutionMode::Scheduled { execution_id } => {
                    let authorization_schema = table_schema.clone();
                    let principal = principal.clone();
                    let applied = self.run_store_mutation_once(
                        runtime,
                        prepared_commit,
                        profile,
                        move |store, prepared, timestamp| {
                            let (table, document_id, patch) = prepared.direct_update_parts()?;
                            store.update_with_indexes_validated_once_at(
                                table,
                                document_id,
                                patch,
                                direct_write_assignment(
                                    &[],
                                    Some(execution_id.as_str()),
                                    timestamp,
                                ),
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
                        },
                    )?;
                    Ok(MutationExecutionResult::Scheduled(applied))
                }
            },
            Some(table_schema) => {
                let indexes = table_schema.indexes.clone();
                match mode {
                    MutationExecutionMode::Immediate => {
                        let authorization_schema = table_schema.clone();
                        let principal = principal.clone();
                        self.run_store_mutation(
                            runtime,
                            prepared_commit,
                            profile,
                            move |store, prepared, timestamp| {
                                let (table, document_id, patch) = prepared.direct_update_parts()?;
                                store
                                    .update_with_indexes_validated_once_at(
                                        table,
                                        document_id,
                                        patch,
                                        direct_write_assignment(&indexes, None, timestamp),
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
                                    )?
                                    .ok_or_else(|| {
                                        nimbus_core::Error::Internal(
                                            "direct indexed update should produce a commit"
                                                .to_string(),
                                        )
                                    })
                            },
                        )?;
                        Ok(MutationExecutionResult::Immediate(Some(result_document_id)))
                    }
                    MutationExecutionMode::Scheduled { execution_id } => {
                        let authorization_schema = table_schema.clone();
                        let principal = principal.clone();
                        let applied = self.run_store_mutation_once(
                            runtime,
                            prepared_commit,
                            profile,
                            move |store, prepared, timestamp| {
                                let (table, document_id, patch) = prepared.direct_update_parts()?;
                                store.update_with_indexes_validated_once_at(
                                    table,
                                    document_id,
                                    patch,
                                    direct_write_assignment(
                                        &indexes,
                                        Some(execution_id.as_str()),
                                        timestamp,
                                    ),
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
                            },
                        )?;
                        Ok(MutationExecutionResult::Scheduled(applied))
                    }
                }
            }
            None => match mode {
                MutationExecutionMode::Immediate => {
                    let principal = principal.clone();
                    self.run_store_mutation(
                        runtime,
                        prepared_commit,
                        profile,
                        move |store, prepared, timestamp| {
                            let (table, document_id, patch) = prepared.direct_update_parts()?;
                            store
                                .update_with_indexes_validated_once_at(
                                    table,
                                    document_id,
                                    patch,
                                    direct_write_assignment(&[], None, timestamp),
                                    move |existing, document| {
                                        enforce_mutation_authorization(
                                            None,
                                            AccessAction::Update,
                                            &principal,
                                            Some(document),
                                            Some(existing),
                                        )
                                    },
                                )?
                                .ok_or_else(|| {
                                    nimbus_core::Error::Internal(
                                        "direct update should produce a commit".to_string(),
                                    )
                                })
                        },
                    )?;
                    Ok(MutationExecutionResult::Immediate(Some(result_document_id)))
                }
                MutationExecutionMode::Scheduled { execution_id } => {
                    let principal = principal.clone();
                    let applied = self.run_store_mutation_once(
                        runtime,
                        prepared_commit,
                        profile,
                        move |store, prepared, timestamp| {
                            let (table, document_id, patch) = prepared.direct_update_parts()?;
                            store.update_with_indexes_validated_once_at(
                                table,
                                document_id,
                                patch,
                                direct_write_assignment(
                                    &[],
                                    Some(execution_id.as_str()),
                                    timestamp,
                                ),
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
                        },
                    )?;
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
        let commit_started_at = Instant::now();
        let table_schema = schema.get_table(&table).cloned();
        let indexes = table_schema
            .as_ref()
            .map(|table_schema| table_schema.indexes.clone())
            .unwrap_or_default();
        let scheduled_execution_id = match &mode {
            MutationExecutionMode::Immediate => None,
            MutationExecutionMode::Scheduled { execution_id } => Some(execution_id.as_str()),
        };
        let prepared_commit = PreparedCommit::for_direct_delete(
            runtime.durable_head(),
            table,
            id,
            scheduled_execution_id,
        );
        check_mutation_caps(&runtime, prepared_commit.usage())?;
        runtime.check_tenant_write_rate(self.now(), prepared_commit.usage().total_write_bytes())?;
        let profile = DirectMutationProfile::after_prepare(commit_started_at);

        match mode {
            MutationExecutionMode::Immediate => {
                if indexes.is_empty() {
                    let table_schema = table_schema.clone();
                    let principal = principal.clone();
                    self.run_store_delete_mutation(
                        runtime,
                        prepared_commit,
                        profile,
                        move |store, prepared, timestamp| {
                            let (table, document_id) = prepared.direct_delete_parts()?;
                            store
                                .delete_with_indexes_validated_once_at(
                                    table,
                                    document_id,
                                    direct_write_assignment(&[], None, timestamp),
                                    move |existing| {
                                        enforce_mutation_authorization(
                                            table_schema.as_ref(),
                                            AccessAction::Delete,
                                            &principal,
                                            None,
                                            Some(existing),
                                        )
                                    },
                                )?
                                .ok_or_else(|| {
                                    nimbus_core::Error::Internal(
                                        "direct delete should produce a commit".to_string(),
                                    )
                                })
                        },
                    )?;
                } else {
                    let table_schema = table_schema.clone();
                    let principal = principal.clone();
                    self.run_store_delete_mutation(
                        runtime,
                        prepared_commit,
                        profile,
                        move |store, prepared, timestamp| {
                            let (table, document_id) = prepared.direct_delete_parts()?;
                            store
                                .delete_with_indexes_validated_once_at(
                                    table,
                                    document_id,
                                    direct_write_assignment(&indexes, None, timestamp),
                                    move |existing| {
                                        enforce_mutation_authorization(
                                            table_schema.as_ref(),
                                            AccessAction::Delete,
                                            &principal,
                                            None,
                                            Some(existing),
                                        )
                                    },
                                )?
                                .ok_or_else(|| {
                                    nimbus_core::Error::Internal(
                                        "direct indexed delete should produce a commit".to_string(),
                                    )
                                })
                        },
                    )?;
                }
                Ok(MutationExecutionResult::Immediate(None))
            }
            MutationExecutionMode::Scheduled { execution_id } => {
                let applied = if indexes.is_empty() {
                    let table_schema = table_schema.clone();
                    let principal = principal.clone();
                    self.run_store_delete_mutation_once(
                        runtime,
                        prepared_commit,
                        profile,
                        move |store, prepared, timestamp| {
                            let (table, document_id) = prepared.direct_delete_parts()?;
                            store.delete_with_indexes_validated_once_at(
                                table,
                                document_id,
                                direct_write_assignment(
                                    &[],
                                    Some(execution_id.as_str()),
                                    timestamp,
                                ),
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
                        },
                    )?
                } else {
                    let table_schema = table_schema.clone();
                    let principal = principal.clone();
                    self.run_store_delete_mutation_once(
                        runtime,
                        prepared_commit,
                        profile,
                        move |store, prepared, timestamp| {
                            let (table, document_id) = prepared.direct_delete_parts()?;
                            store.delete_with_indexes_validated_once_at(
                                table,
                                document_id,
                                direct_write_assignment(
                                    &indexes,
                                    Some(execution_id.as_str()),
                                    timestamp,
                                ),
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
                        },
                    )?
                };
                Ok(MutationExecutionResult::Scheduled(applied))
            }
        }
    }
}

fn direct_write_assignment<'a>(
    indexes: &'a [nimbus_core::IndexDefinition],
    execution_id: Option<&'a str>,
    commit_timestamp: Timestamp,
) -> nimbus_storage::DirectWriteAssignment<'a> {
    nimbus_storage::DirectWriteAssignment {
        indexes,
        execution_id,
        commit_timestamp,
    }
}
