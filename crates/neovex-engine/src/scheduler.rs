use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use neovex_core::{Mutation, Result, ScheduledJobOutcome, ScheduledJobResult, TenantId, Timestamp};
use tokio::sync::watch;

use crate::Service;

/// Runs the global scheduler loop until shutdown is requested.
pub async fn run_scheduler(service: Arc<Service>, shutdown: watch::Receiver<bool>) {
    run_scheduler_with_interval(service, shutdown, Duration::from_secs(1)).await;
}

pub(crate) async fn run_scheduler_with_interval(
    service: Arc<Service>,
    mut shutdown: watch::Receiver<bool>,
    _interval: Duration,
) {
    loop {
        if let Err(error) = tick_async(&service).await {
            tracing::error!(error = %error, "scheduler tick failed");
        }

        let next_due = match service.next_loaded_scheduled_work_at_async().await {
            Ok(next_due) => next_due,
            Err(error) => {
                tracing::error!(error = %error, "scheduler failed to compute next due work");
                None
            }
        };

        let wake = service.scheduler_notifier().notified();
        tokio::pin!(wake);

        match next_due {
            Some(next_due) if next_due <= service.now() => continue,
            Some(next_due) => {
                let delay_ms = next_due.0.saturating_sub(service.now().0);
                let sleep = tokio::time::sleep(Duration::from_millis(delay_ms));
                tokio::pin!(sleep);
                tokio::select! {
                    _ = &mut sleep => {}
                    _ = &mut wake => {}
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            tracing::info!("scheduler shutting down");
                            break;
                        }
                    }
                }
            }
            None => {
                tokio::select! {
                    _ = &mut wake => {}
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            tracing::info!("scheduler shutting down");
                            break;
                        }
                    }
                }
            }
        }
    }
}

pub(crate) async fn tick_async(service: &Arc<Service>) -> Result<()> {
    tick_at_async(service, service.now()).await
}

#[cfg(test)]
pub(crate) fn tick_at(service: &Service, now: Timestamp) -> Result<()> {
    for tenant_id in service.loaded_tenant_ids() {
        if let Err(error) = process_tenant(service, &tenant_id, now) {
            tracing::warn!(tenant = %tenant_id, error = %error, "scheduler failed for tenant");
        }
    }
    Ok(())
}

pub(crate) async fn tick_at_async(service: &Arc<Service>, now: Timestamp) -> Result<()> {
    let tenant_ids = service.loaded_tenant_ids();
    let max_concurrent_tenant_ticks = scheduler_tenant_tick_parallelism(tenant_ids.len());
    stream::iter(tenant_ids)
        .for_each_concurrent(max_concurrent_tenant_ticks, |tenant_id| {
            let service = service.clone();
            async move {
                if let Err(error) = process_tenant_async(&service, &tenant_id, now).await {
                    tracing::warn!(tenant = %tenant_id, error = %error, "scheduler failed for tenant");
                }
            }
        })
        .await;
    Ok(())
}

fn scheduler_tenant_tick_parallelism(tenant_count: usize) -> usize {
    let available_parallelism = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(4)
        .max(1);
    tenant_count.max(1).min(available_parallelism)
}

#[cfg(test)]
fn process_tenant(service: &Service, tenant_id: &TenantId, now: Timestamp) -> Result<()> {
    process_due_jobs(service, tenant_id, now)?;
    process_cron_jobs(service, tenant_id, now)?;
    Ok(())
}

async fn process_tenant_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    now: Timestamp,
) -> Result<()> {
    process_due_jobs_async(service, tenant_id, now).await?;
    process_cron_jobs_async(service, tenant_id, now).await?;
    Ok(())
}

#[cfg(test)]
fn process_due_jobs(service: &Service, tenant_id: &TenantId, now: Timestamp) -> Result<()> {
    let due_jobs = service.claim_due_jobs(tenant_id, now)?;
    for job in due_jobs {
        let execution_id = format!("scheduled:{}", job.id);
        let result =
            service.execute_scheduled_mutation(tenant_id, &execution_id, job.mutation.clone());
        match &result {
            Ok(true) => {
                tracing::debug!(tenant = %tenant_id, job_id = %job.id, "scheduled job completed");
            }
            Ok(false) => {
                tracing::debug!(
                    tenant = %tenant_id,
                    job_id = %job.id,
                    "scheduled job replay deduplicated"
                );
            }
            Err(error) => {
                tracing::warn!(
                    tenant = %tenant_id,
                    job_id = %job.id,
                    error = %error,
                    "scheduled job failed"
                );
            }
        }

        let execution_result = ScheduledJobResult {
            id: job.id.clone(),
            run_at: job.run_at,
            finished_at: service.now(),
            mutation: job.mutation,
            outcome: if result.is_ok() {
                ScheduledJobOutcome::Completed
            } else {
                ScheduledJobOutcome::Failed
            },
            error: result.as_ref().err().map(ToString::to_string),
        };
        service.record_scheduled_job_result(tenant_id, &execution_result)?;
        service.complete_scheduled_job(tenant_id, &job.id)?;
    }
    Ok(())
}

async fn process_due_jobs_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    now: Timestamp,
) -> Result<()> {
    let due_jobs = service.claim_due_jobs_async(tenant_id.clone(), now).await?;
    for job in due_jobs {
        let job_id = job.id.clone();
        let execution_id = format!("scheduled:{}", job.id);
        let result = service
            .execute_scheduled_mutation_async(tenant_id.clone(), execution_id, job.mutation.clone())
            .await;
        match &result {
            Ok(true) => {
                tracing::debug!(tenant = %tenant_id, job_id = %job.id, "scheduled job completed");
            }
            Ok(false) => {
                tracing::debug!(
                    tenant = %tenant_id,
                    job_id = %job.id,
                    "scheduled job replay deduplicated"
                );
            }
            Err(error) => {
                tracing::warn!(
                    tenant = %tenant_id,
                    job_id = %job.id,
                    error = %error,
                    "scheduled job failed"
                );
            }
        }

        let execution_result = ScheduledJobResult {
            id: job_id.clone(),
            run_at: job.run_at,
            finished_at: service.now(),
            mutation: job.mutation,
            outcome: if result.is_ok() {
                ScheduledJobOutcome::Completed
            } else {
                ScheduledJobOutcome::Failed
            },
            error: result.as_ref().err().map(ToString::to_string),
        };
        service
            .record_scheduled_job_result_async(tenant_id.clone(), execution_result)
            .await?;
        service
            .complete_scheduled_job_async(tenant_id.clone(), job_id)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
fn process_cron_jobs(service: &Service, tenant_id: &TenantId, now: Timestamp) -> Result<()> {
    let cron_jobs = service.load_cron_jobs(tenant_id)?;
    for mut cron in cron_jobs {
        if !cron.enabled || cron.next_run.0 > now.0 {
            continue;
        }

        if let Err(error) = dispatch_mutation(service, tenant_id, cron.mutation.clone()) {
            tracing::warn!(
                tenant = %tenant_id,
                cron = %cron.name,
                error = %error,
                "cron job failed"
            );
        } else {
            tracing::debug!(tenant = %tenant_id, cron = %cron.name, "cron job completed");
        }

        cron.last_run = Some(now);
        cron.next_run = cron.schedule.next_after(now);
        service.update_cron_job(tenant_id, &cron)?;
    }
    Ok(())
}

async fn process_cron_jobs_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    now: Timestamp,
) -> Result<()> {
    let cron_jobs = service.load_cron_jobs_async(tenant_id.clone()).await?;
    for mut cron in cron_jobs {
        if !cron.enabled || cron.next_run.0 > now.0 {
            continue;
        }

        if let Err(error) =
            dispatch_mutation_async(service, tenant_id.clone(), cron.mutation.clone()).await
        {
            tracing::warn!(
                tenant = %tenant_id,
                cron = %cron.name,
                error = %error,
                "cron job failed"
            );
        } else {
            tracing::debug!(tenant = %tenant_id, cron = %cron.name, "cron job completed");
        }

        cron.last_run = Some(now);
        cron.next_run = cron.schedule.next_after(now);
        service
            .update_cron_job_async(tenant_id.clone(), cron)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
fn dispatch_mutation(service: &Service, tenant_id: &TenantId, mutation: Mutation) -> Result<()> {
    match mutation {
        Mutation::Insert { table, id, fields } => service
            .insert_document_with_id_with_principal(
                tenant_id,
                table,
                id,
                fields,
                &neovex_core::PrincipalContext::anonymous(),
            )
            .map(|_| ()),
        Mutation::Update { table, id, patch } => service
            .update_document(tenant_id, table, id, patch)
            .map(|_| ()),
        Mutation::Delete { table, id } => service.delete_document(tenant_id, table, id),
    }
}

async fn dispatch_mutation_async(
    service: &Arc<Service>,
    tenant_id: TenantId,
    mutation: Mutation,
) -> Result<()> {
    match mutation {
        Mutation::Insert { table, id, fields } => service
            .insert_document_async_with_id_with_principal(
                tenant_id,
                table,
                id,
                fields,
                neovex_core::PrincipalContext::anonymous(),
            )
            .await
            .map(|_| ()),
        Mutation::Update { table, id, patch } => service
            .update_document_async(tenant_id, table, id, patch)
            .await
            .map(|_| ()),
        Mutation::Delete { table, id } => service.delete_document_async(tenant_id, table, id).await,
    }
}
