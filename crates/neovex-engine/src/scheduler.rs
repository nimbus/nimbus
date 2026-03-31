use std::sync::Arc;
use std::time::Duration;

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
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let service = service.clone();
                match tokio::task::spawn_blocking(move || tick(service.as_ref())).await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => tracing::error!(error = %error, "scheduler tick failed"),
                    Err(error) => tracing::error!(error = %error, "scheduler task failed"),
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    tracing::info!("scheduler shutting down");
                    break;
                }
            }
        }
    }
}

pub(crate) fn tick(service: &Service) -> Result<()> {
    tick_at(service, Timestamp::now())
}

pub(crate) fn tick_at(service: &Service, now: Timestamp) -> Result<()> {
    for tenant_id in service.loaded_tenant_ids() {
        if let Err(error) = process_tenant(service, &tenant_id, now) {
            tracing::warn!(tenant = %tenant_id, error = %error, "scheduler failed for tenant");
        }
    }
    Ok(())
}

fn process_tenant(service: &Service, tenant_id: &TenantId, now: Timestamp) -> Result<()> {
    process_due_jobs(service, tenant_id, now)?;
    process_cron_jobs(service, tenant_id, now)?;
    Ok(())
}

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
            id: job.id,
            run_at: job.run_at,
            finished_at: Timestamp::now(),
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

fn dispatch_mutation(service: &Service, tenant_id: &TenantId, mutation: Mutation) -> Result<()> {
    match mutation {
        Mutation::Insert { table, fields } => service
            .insert_document(tenant_id, table, fields)
            .map(|_| ()),
        Mutation::Update { table, id, patch } => service
            .update_document(tenant_id, table, id, patch)
            .map(|_| ()),
        Mutation::Delete { table, id } => service.delete_document(tenant_id, table, id),
    }
}
