use std::{future, sync::Arc};

use neovex_core::{DocumentId, Error, Mutation, PrincipalContext, Result, TableName, TenantId};

use crate::Service;

use super::types::{
    MutationExecutionMode, expect_immediate_document_id, expect_immediate_result,
    expect_immediate_unit, expect_scheduled_applied,
};

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
