use std::sync::Arc;

use nimbus_core::{
    AtomicWriteBatch, AtomicWriteBatchOutcome, Document, DocumentLocator, Error, PrincipalContext,
    Result, TableName, TenantId,
};
use nimbus_engine::{MutationExecutionUnit, Service};
use nimbus_runtime::{HostCallCancellation, NimbusRuntimeError};
use serde_json::{Map, Value};

use crate::execution::errors::{check_host_cancellation, ensure_runtime_host_not_cancelled};

pub(crate) trait RuntimeCapabilityHost {
    fn validate_session(
        &self,
        session_id: Option<&str>,
    ) -> std::result::Result<(), NimbusRuntimeError>;

    fn mutation_execution_unit(&self) -> Option<&Arc<MutationExecutionUnit>>;

    fn service(&self) -> &Arc<Service>;

    fn tenant_id(&self) -> &TenantId;

    fn principal(&self) -> &PrincipalContext;

    fn record_document_read(&self, locator: &DocumentLocator);
}

pub(crate) fn validate_runtime_capability_access<H>(
    host: &H,
    session_id: Option<&str>,
    cancellation: &HostCallCancellation,
) -> std::result::Result<(), NimbusRuntimeError>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    host.validate_session(session_id)?;
    ensure_runtime_host_not_cancelled(cancellation)
}

pub(crate) fn get_document<H>(host: &H, locator: &DocumentLocator) -> Result<Option<Document>>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    host.mutation_execution_unit()
        .map_or_else(
            || {
                host.service()
                    .get_document_with_principal(
                        host.tenant_id(),
                        &locator.table,
                        locator.id.clone(),
                        host.principal(),
                    )
                    .map(Some)
                    .or_else(|error| match error {
                        Error::DocumentNotFound(_) => Ok(None),
                        other => Err(other),
                    })
            },
            |execution_unit| execution_unit.get_document(&locator.table, locator.id.clone()),
        )
        .inspect(|document| {
            if document.is_some() {
                host.record_document_read(locator);
            }
        })
}

pub(crate) async fn get_document_async<H>(
    host: &H,
    locator: &DocumentLocator,
    cancellation: &HostCallCancellation,
) -> Result<Option<Document>>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        return execution_unit
            .get_document(&locator.table, locator.id.clone())
            .inspect(|document| {
                if document.is_some() {
                    host.record_document_read(locator);
                }
            });
    }

    let check_cancellation = cancellation.clone();
    host.service()
        .get_document_async_cancellable_with_principal(
            host.tenant_id().clone(),
            locator.table.clone(),
            locator.id.clone(),
            host.principal().clone(),
            cancellation.cancelled(),
            move || check_host_cancellation(&check_cancellation),
        )
        .await
        .map(Some)
        .or_else(|error| match error {
            Error::DocumentNotFound(_) => Ok(None),
            other => Err(other),
        })
        .inspect(|document| {
            if document.is_some() {
                host.record_document_read(locator);
            }
        })
}

pub(crate) fn execute_atomic_write_batch<H>(
    host: &H,
    batch: AtomicWriteBatch,
) -> Result<AtomicWriteBatchOutcome>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        execution_unit.stage_atomic_write_batch(batch)
    } else {
        host.service()
            .begin_mutation_execution_unit(host.tenant_id().clone(), host.principal().clone())?
            .execute_atomic_write_batch(batch)
    }
}

pub(crate) async fn execute_atomic_write_batch_async<H>(
    host: &H,
    batch: AtomicWriteBatch,
    cancellation: &HostCallCancellation,
) -> Result<AtomicWriteBatchOutcome>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        return execution_unit.stage_atomic_write_batch(batch);
    }
    check_host_cancellation(cancellation)?;
    Err(Error::InvalidInput(
        "async atomic write batch execution requires an active mutation execution unit".to_string(),
    ))
}

pub(crate) fn insert_document<H>(
    host: &H,
    table: TableName,
    fields: Map<String, Value>,
) -> Result<nimbus_core::DocumentId>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        execution_unit.insert_document(table, fields)
    } else {
        host.service().insert_document_with(
            host.tenant_id(),
            table,
            None,
            fields,
            nimbus_engine::MutationActor::with_principal(host.principal()),
        )
    }
}

pub(crate) async fn insert_document_async<H>(
    host: &H,
    table: TableName,
    fields: Map<String, Value>,
    cancellation: &HostCallCancellation,
) -> Result<nimbus_core::DocumentId>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        return execution_unit.insert_document(table, fields);
    }

    let check_cancellation = cancellation.clone();
    let cancel_wait = {
        let cancellation = cancellation.clone();
        async move {
            cancellation.cancelled().await;
        }
    };
    host.service()
        .insert_document_async_with(
            host.tenant_id().clone(),
            table,
            None,
            fields,
            nimbus_engine::AsyncMutationContext::with_principal(
                host.principal().clone(),
                cancel_wait,
                move || check_host_cancellation(&check_cancellation),
            ),
        )
        .await
}

pub(crate) fn update_document<H>(
    host: &H,
    table: TableName,
    document_id: nimbus_core::DocumentId,
    patch: Map<String, Value>,
) -> Result<nimbus_core::DocumentId>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        execution_unit.update_document(table, document_id, patch)
    } else {
        host.service().update_document_with(
            host.tenant_id(),
            table,
            document_id,
            patch,
            nimbus_engine::MutationActor::with_principal(host.principal()),
        )
    }
}

pub(crate) async fn update_document_async<H>(
    host: &H,
    table: TableName,
    document_id: nimbus_core::DocumentId,
    patch: Map<String, Value>,
    cancellation: &HostCallCancellation,
) -> Result<nimbus_core::DocumentId>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        return execution_unit.update_document(table, document_id, patch);
    }

    let check_cancellation = cancellation.clone();
    let cancel_wait = {
        let cancellation = cancellation.clone();
        async move {
            cancellation.cancelled().await;
        }
    };
    host.service()
        .update_document_async_with(
            host.tenant_id().clone(),
            table,
            document_id,
            patch,
            nimbus_engine::AsyncMutationContext::with_principal(
                host.principal().clone(),
                cancel_wait,
                move || check_host_cancellation(&check_cancellation),
            ),
        )
        .await
}

pub(crate) fn delete_document<H>(
    host: &H,
    table: TableName,
    document_id: nimbus_core::DocumentId,
) -> Result<()>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        execution_unit.delete_document(table, document_id)
    } else {
        host.service().delete_document_with(
            host.tenant_id(),
            table,
            document_id,
            nimbus_engine::MutationActor::with_principal(host.principal()),
        )
    }
}

pub(crate) async fn delete_document_async<H>(
    host: &H,
    table: TableName,
    document_id: nimbus_core::DocumentId,
    cancellation: &HostCallCancellation,
) -> Result<()>
where
    H: RuntimeCapabilityHost + ?Sized,
{
    if let Some(execution_unit) = host.mutation_execution_unit() {
        return execution_unit.delete_document(table, document_id);
    }

    let check_cancellation = cancellation.clone();
    let cancel_wait = {
        let cancellation = cancellation.clone();
        async move {
            cancellation.cancelled().await;
        }
    };
    host.service()
        .delete_document_async_with(
            host.tenant_id().clone(),
            table,
            document_id,
            nimbus_engine::AsyncMutationContext::with_principal(
                host.principal().clone(),
                cancel_wait,
                move || check_host_cancellation(&check_cancellation),
            ),
        )
        .await
}
