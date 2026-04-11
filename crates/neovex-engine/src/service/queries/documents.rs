use std::future::{Future, pending};
use std::sync::Arc;

use futures::FutureExt;
use neovex_core::{
    Document, DocumentId, Error, PrincipalContext, Query, Result, TableName, TenantId,
};

use super::authorization::ReadAuthorization;
use super::materialized::wait_for_latest_applied_visibility_blocking;
use crate::service::Service;

fn full_table_query(table: TableName) -> Query {
    Query {
        table,
        filters: Vec::new(),
        order: None,
        limit: None,
    }
}

impl Service {
    /// Lists documents in a logical table.
    pub fn list_documents(&self, tenant_id: &TenantId, table: &TableName) -> Result<Vec<Document>> {
        self.list_documents_with_principal(tenant_id, table, &PrincipalContext::anonymous())
    }

    /// Lists documents in a logical table for the provided principal.
    pub fn list_documents_with_principal(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        principal: &PrincipalContext,
    ) -> Result<Vec<Document>> {
        self.query_documents_with_principal_cancellable(
            tenant_id,
            &full_table_query(table.clone()),
            principal,
            &mut || Ok(()),
        )
    }

    /// Lists documents in a logical table asynchronously.
    pub async fn list_documents_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
    ) -> Result<Vec<Document>> {
        self.list_documents_async_with_principal(tenant_id, table, PrincipalContext::anonymous())
            .await
    }

    /// Lists documents in a logical table asynchronously for the provided principal.
    pub async fn list_documents_async_with_principal(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        principal: PrincipalContext,
    ) -> Result<Vec<Document>> {
        self.query_documents_async_cancellable_with_principal(
            tenant_id,
            full_table_query(table),
            principal,
            pending(),
            || Ok(()),
        )
        .await
    }

    /// Lists documents in a logical table asynchronously with cooperative cancellation.
    pub async fn list_documents_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Vec<Document>>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.list_documents_async_cancellable_with_principal(
            tenant_id,
            table,
            PrincipalContext::anonymous(),
            cancel_wait,
            check_cancel,
        )
        .await
    }

    /// Lists documents asynchronously for a principal with cooperative cancellation.
    pub async fn list_documents_async_cancellable_with_principal<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Vec<Document>>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.query_documents_async_cancellable_with_principal(
            tenant_id,
            full_table_query(table),
            principal,
            cancel_wait,
            check_cancel,
        )
        .await
    }

    /// Fetches a single document in a logical table.
    pub fn get_document(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        document_id: DocumentId,
    ) -> Result<Document> {
        self.get_document_with_principal(
            tenant_id,
            table,
            document_id,
            &PrincipalContext::anonymous(),
        )
    }

    /// Fetches a single document in a logical table for the provided principal.
    pub fn get_document_with_principal(
        &self,
        tenant_id: &TenantId,
        table: &TableName,
        document_id: DocumentId,
        principal: &PrincipalContext,
    ) -> Result<Document> {
        let runtime = self.get_existing_tenant(tenant_id)?;
        let required_sequence = wait_for_latest_applied_visibility_blocking(&runtime);
        let _operation = runtime.enter_operation(tenant_id)?;
        let schema = runtime.schema();
        let authorization = ReadAuthorization::for_table(schema.get_table(table), principal)?;
        if authorization.impossible {
            return Err(Error::DocumentNotFound(document_id));
        }

        if let Some(document) = runtime.get_cached_document(table, document_id) {
            if !authorization.allows_document(principal, &document)? {
                return Err(Error::DocumentNotFound(document_id));
            }
            return Ok(document);
        }

        if let Some(snapshot) =
            runtime.materialized_serving_snapshot_for_table(table, required_sequence)
        {
            debug_assert!(snapshot.covered_sequence().0 >= required_sequence.0);
            if let Some(document) = snapshot.document(table, document_id) {
                if !authorization.allows_document(principal, &document)? {
                    return Err(Error::DocumentNotFound(document_id));
                }
                runtime.record_materialized_get_hit();
                runtime.cache_document(&document);
                return Ok(document);
            }
        }

        let document = runtime
            .store
            .get(table, &document_id)?
            .ok_or(Error::DocumentNotFound(document_id))?;
        if !authorization.allows_document(principal, &document)? {
            return Err(Error::DocumentNotFound(document_id));
        }
        runtime.cache_document(&document);
        Ok(document)
    }

    /// Fetches a single document in a logical table asynchronously.
    pub async fn get_document_async(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
    ) -> Result<Document> {
        self.get_document_async_cancellable_with_principal(
            tenant_id,
            table,
            document_id,
            PrincipalContext::anonymous(),
            pending(),
            || Ok(()),
        )
        .await
    }

    /// Fetches a single document asynchronously for the provided principal.
    pub async fn get_document_async_with_principal(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        principal: PrincipalContext,
    ) -> Result<Document> {
        self.get_document_async_cancellable_with_principal(
            tenant_id,
            table,
            document_id,
            principal,
            pending(),
            || Ok(()),
        )
        .await
    }

    /// Fetches a single document asynchronously with cooperative cancellation.
    pub async fn get_document_async_cancellable<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Document>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        self.get_document_async_cancellable_with_principal(
            tenant_id,
            table,
            document_id,
            PrincipalContext::anonymous(),
            cancel_wait,
            check_cancel,
        )
        .await
    }

    /// Fetches a single document asynchronously for the provided principal with cooperative cancellation.
    pub async fn get_document_async_cancellable_with_principal<Fut, Check>(
        self: &Arc<Self>,
        tenant_id: TenantId,
        table: TableName,
        document_id: DocumentId,
        principal: PrincipalContext,
        cancel_wait: Fut,
        check_cancel: Check,
    ) -> Result<Document>
    where
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let cancel_wait = cancel_wait.shared();
        let runtime = self.get_existing_tenant_async(&tenant_id).await?;
        let required_sequence = runtime.durable_head();
        runtime
            .wait_for_applied_sequence_cancellable(required_sequence, cancel_wait.clone())
            .await?;
        let _operation = runtime.enter_operation(&tenant_id)?;
        let schema = runtime.schema();
        let authorization = ReadAuthorization::for_table(schema.get_table(&table), &principal)?;
        if authorization.impossible {
            return Err(Error::DocumentNotFound(document_id));
        }

        if let Some(document) = runtime.get_cached_document(&table, document_id) {
            if cancel_wait.clone().now_or_never().is_some() {
                return Err(Error::Cancelled);
            }
            check_cancel()?;
            if !authorization.allows_document(&principal, &document)? {
                return Err(Error::DocumentNotFound(document_id));
            }
            return Ok(document);
        }

        if let Some(snapshot) =
            runtime.materialized_serving_snapshot_for_table(&table, required_sequence)
        {
            debug_assert!(snapshot.covered_sequence().0 >= required_sequence.0);
            if let Some(document) = snapshot.document(&table, document_id) {
                if cancel_wait.clone().now_or_never().is_some() {
                    return Err(Error::Cancelled);
                }
                check_cancel()?;
                if !authorization.allows_document(&principal, &document)? {
                    return Err(Error::DocumentNotFound(document_id));
                }
                runtime.record_materialized_get_hit();
                runtime.cache_document(&document);
                return Ok(document);
            }
        }

        let table_for_task = table.clone();
        let document = runtime
            .read_storage
            .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                check_cancel()?;
                store.get(&table_for_task, &document_id)
            })
            .await?
            .ok_or(Error::DocumentNotFound(document_id))?;
        if !authorization.allows_document(&principal, &document)? {
            return Err(Error::DocumentNotFound(document_id));
        }
        runtime.cache_document(&document);
        Ok(document)
    }
}
