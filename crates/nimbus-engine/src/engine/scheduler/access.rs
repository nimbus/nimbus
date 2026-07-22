use std::sync::atomic::{AtomicBool, Ordering};
use std::{future::Future, sync::Arc};

use nimbus_core::{Error, Result, ScheduledJob, TenantId};
use nimbus_storage::{SchedulerWrite, SchedulerWriteResult};

use crate::engine::execution_units::labels;
use crate::engine::tenants::with_tenant_runtime_operation;
use crate::persistence::TenantPersistence;
use crate::{Engine, tenant::TenantRuntime};

pub(super) fn with_scheduler_runtime<T, F>(
    engine: &Engine,
    tenant_id: &TenantId,
    task: F,
) -> Result<T>
where
    F: FnOnce(Arc<TenantRuntime>) -> Result<T>,
{
    let runtime = engine.get_existing_tenant(tenant_id)?;
    with_tenant_runtime_operation(runtime, tenant_id, task)
}

pub(super) async fn read_scheduler_store<T, F>(
    engine: &Arc<Engine>,
    tenant_id: TenantId,
    task: F,
) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(TenantPersistence) -> Result<T> + Send + 'static,
{
    let runtime = engine.get_existing_tenant_async(&tenant_id).await?;
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

pub(super) fn write_scheduler_state_blocking(
    engine: &Engine,
    tenant_id: &TenantId,
    operation: SchedulerWrite,
) -> Result<SchedulerWriteResult> {
    let runtime = engine.get_existing_tenant(tenant_id)?;
    let operation_guard = runtime.enter_operation(tenant_id)?;
    let recovery_now = engine.now();
    let initiated_eviction = Arc::new(AtomicBool::new(false));
    let initiated_eviction_for_commit = initiated_eviction.clone();
    let runtime_for_commit = runtime.clone();
    let commit_faults = engine.commit_faults.clone();
    let result = runtime.submit_internal_committer(move || {
        runtime_for_commit.persist_scheduler_write(
            operation,
            recovery_now,
            || Ok(()),
            move || {
                commit_faults
                    .wait(labels::SCHEDULER_DURABLE_BEFORE_ACK)
                    .into_result()
            },
            initiated_eviction_for_commit,
        )
    });
    drop(operation_guard);
    let eviction_completion = initiated_eviction
        .load(Ordering::Acquire)
        .then(|| runtime.eviction_completion());
    drop(runtime);
    if let Some(completion) = eviction_completion {
        completion.wait_blocking();
    }
    result
}

pub(super) async fn write_scheduler_state(
    engine: &Arc<Engine>,
    tenant_id: TenantId,
    operation: SchedulerWrite,
) -> Result<SchedulerWriteResult> {
    let runtime = engine.get_existing_tenant_async(&tenant_id).await?;
    let operation_guard = runtime.enter_operation(&tenant_id)?;
    let recovery_now = engine.now();
    let initiated_eviction = Arc::new(AtomicBool::new(false));
    let initiated_eviction_for_commit = initiated_eviction.clone();
    let runtime_for_commit = runtime.clone();
    let commit_faults = engine.commit_faults.clone();
    let result = runtime
        .submit_internal_committer_async(move || {
            runtime_for_commit.persist_scheduler_write(
                operation,
                recovery_now,
                || Ok(()),
                move || {
                    commit_faults
                        .wait(labels::SCHEDULER_DURABLE_BEFORE_ACK)
                        .into_result()
                },
                initiated_eviction_for_commit,
            )
        })
        .await;
    drop(operation_guard);
    await_scheduler_eviction_if_started(runtime, &initiated_eviction).await;
    result
}

pub(super) async fn write_scheduler_state_cancellable<Fut, Check>(
    engine: &Arc<Engine>,
    tenant_id: TenantId,
    operation: SchedulerWrite,
    cancel_wait: Fut,
    check_cancel: Check,
) -> Result<SchedulerWriteResult>
where
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + 'static,
{
    let runtime = engine.get_existing_tenant_async(&tenant_id).await?;
    write_loaded_scheduler_state_cancellable(
        engine,
        runtime,
        tenant_id,
        operation,
        engine.now(),
        cancel_wait,
        check_cancel,
    )
    .await
}

pub(super) async fn write_loaded_scheduler_state_cancellable<Fut, Check>(
    engine: &Arc<Engine>,
    runtime: Arc<TenantRuntime>,
    tenant_id: TenantId,
    operation: SchedulerWrite,
    recovery_now: nimbus_core::Timestamp,
    cancel_wait: Fut,
    check_cancel: Check,
) -> Result<SchedulerWriteResult>
where
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + 'static,
{
    let operation_guard = runtime.enter_operation(&tenant_id)?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancelled_for_commit = cancelled.clone();
    let initiated_eviction = Arc::new(AtomicBool::new(false));
    let initiated_eviction_for_commit = initiated_eviction.clone();
    let runtime_for_commit = runtime.clone();
    let commit_faults = engine.commit_faults.clone();
    let result = {
        let submit = runtime.submit_internal_committer_async(move || {
            runtime_for_commit.persist_scheduler_write(
                operation,
                recovery_now,
                move || {
                    check_cancel()?;
                    if cancelled_for_commit.load(Ordering::Acquire) {
                        Err(Error::Cancelled)
                    } else {
                        Ok(())
                    }
                },
                move || {
                    commit_faults
                        .wait(labels::SCHEDULER_DURABLE_BEFORE_ACK)
                        .into_result()
                },
                initiated_eviction_for_commit,
            )
        });
        tokio::pin!(submit);
        tokio::pin!(cancel_wait);
        tokio::select! {
            biased;
            result = &mut submit => result,
            () = &mut cancel_wait => {
                cancelled.store(true, Ordering::Release);
                submit.as_mut().await
            }
        }
    };
    drop(operation_guard);
    await_scheduler_eviction_if_started(runtime, &initiated_eviction).await;
    result
}

async fn await_scheduler_eviction_if_started(
    runtime: Arc<TenantRuntime>,
    initiated_eviction: &AtomicBool,
) {
    let completion = initiated_eviction
        .load(Ordering::Acquire)
        .then(|| runtime.eviction_completion());
    drop(runtime);
    if let Some(completion) = completion {
        completion.wait().await;
    }
}

pub(super) fn expect_scheduler_unit(result: SchedulerWriteResult) -> Result<()> {
    match result {
        SchedulerWriteResult::Unit => Ok(()),
        other => Err(Error::Internal(format!(
            "scheduler write returned unexpected result: {other:?}"
        ))),
    }
}

pub(super) fn expect_claimed(result: SchedulerWriteResult) -> Result<Vec<ScheduledJob>> {
    match result {
        SchedulerWriteResult::Claimed(jobs) => Ok(jobs),
        other => Err(Error::Internal(format!(
            "scheduler claim returned unexpected result: {other:?}"
        ))),
    }
}

pub(super) fn expect_removed(result: SchedulerWriteResult) -> Result<bool> {
    match result {
        SchedulerWriteResult::Removed(removed) => Ok(removed),
        other => Err(Error::Internal(format!(
            "scheduler cancellation returned unexpected result: {other:?}"
        ))),
    }
}
