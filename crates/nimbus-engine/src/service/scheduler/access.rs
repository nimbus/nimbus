use std::{future::Future, sync::Arc};

use nimbus_core::{Result, TenantId};
use nimbus_storage::TenantWriteOutcome;

use crate::persistence::{TenantPersistence, TenantPersistenceWriteOps};
use crate::service::tenants::with_tenant_runtime_operation;
use crate::{Service, tenant::TenantRuntime};

pub(super) fn with_scheduler_runtime<T, F>(
    service: &Service,
    tenant_id: &TenantId,
    task: F,
) -> Result<T>
where
    F: FnOnce(Arc<TenantRuntime>) -> Result<T>,
{
    let runtime = service.get_existing_tenant(tenant_id)?;
    with_tenant_runtime_operation(runtime, tenant_id, task)
}

pub(super) async fn read_scheduler_store<T, F>(
    service: &Arc<Service>,
    tenant_id: TenantId,
    task: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(TenantPersistence) -> Result<T> + Send + 'static,
{
    let runtime = service.get_existing_tenant_async(&tenant_id).await?;
    read_loaded_tenant_store(runtime, tenant_id, task).await
}

pub(super) async fn read_loaded_tenant_store<T, F>(
    runtime: Arc<TenantRuntime>,
    tenant_id: TenantId,
    task: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(TenantPersistence) -> Result<T> + Send + 'static,
{
    let tenant_id_for_task = tenant_id.clone();
    let runtime_for_task = runtime.clone();
    runtime
        .read_storage
        .execute(move |store| {
            with_tenant_runtime_operation(runtime_for_task, &tenant_id_for_task, |_runtime| {
                task(store)
            })
        })
        .await
}

pub(super) async fn write_scheduler_transaction<T, F>(
    service: &Arc<Service>,
    tenant_id: TenantId,
    task: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut dyn TenantPersistenceWriteOps) -> Result<T> + Send + 'static,
{
    let runtime = service.get_existing_tenant_async(&tenant_id).await?;
    write_loaded_tenant_transaction(runtime, tenant_id, task).await
}

pub(super) async fn write_loaded_tenant_transaction<T, F>(
    runtime: Arc<TenantRuntime>,
    tenant_id: TenantId,
    task: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut dyn TenantPersistenceWriteOps) -> Result<T> + Send + 'static,
{
    let tenant_id_for_task = tenant_id.clone();
    let runtime_for_task = runtime.clone();
    runtime
        .read_storage
        .execute_write(move |transaction| {
            with_tenant_runtime_operation(runtime_for_task, &tenant_id_for_task, |_runtime| {
                task(transaction)
            })
        })
        .await
        .map(|commit| commit.value)
}

pub(super) async fn write_scheduler_transaction_cancellable<T, Fut, Check, F>(
    service: &Arc<Service>,
    tenant_id: TenantId,
    cancel_wait: Fut,
    check_cancel: Check,
    task: F,
) -> Result<TenantWriteOutcome<T>>
where
    T: Send + 'static,
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + 'static,
    F: FnOnce(&mut dyn TenantPersistenceWriteOps) -> Result<T> + Send + 'static,
{
    let runtime = service.get_existing_tenant_async(&tenant_id).await?;
    write_loaded_tenant_transaction_cancellable(runtime, tenant_id, cancel_wait, check_cancel, task)
        .await
}

pub(super) async fn write_loaded_tenant_transaction_cancellable<T, Fut, Check, F>(
    runtime: Arc<TenantRuntime>,
    tenant_id: TenantId,
    cancel_wait: Fut,
    check_cancel: Check,
    task: F,
) -> Result<TenantWriteOutcome<T>>
where
    T: Send + 'static,
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + 'static,
    F: FnOnce(&mut dyn TenantPersistenceWriteOps) -> Result<T> + Send + 'static,
{
    let tenant_id_for_task = tenant_id.clone();
    let runtime_for_task = runtime.clone();
    runtime
        .read_storage
        .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
            with_tenant_runtime_operation(runtime_for_task, &tenant_id_for_task, |_runtime| {
                task(transaction)
            })
        })
        .await
}
