use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use nimbus_core::{Mutation, Result, ScheduledJobOutcome, ScheduledJobResult, TenantId, Timestamp};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::Engine;

mod durable_deadline;

use durable_deadline::DurableDeadlineWake;

/// Runs the global scheduler loop until shutdown is requested.
pub async fn run_scheduler(engine: Arc<Engine>, shutdown: watch::Receiver<bool>) {
    run_scheduler_with_interval(engine, shutdown, Duration::from_secs(1)).await;
}

pub(crate) async fn run_scheduler_with_interval(
    engine: Arc<Engine>,
    mut shutdown: watch::Receiver<bool>,
    resample_interval: Duration,
) {
    let retry_delay = resample_interval.max(Duration::from_millis(1));
    let mut retries = TenantRetryBackoff::default();
    loop {
        let now = Instant::now();
        let tenant_ids = engine.loaded_tenant_ids();
        let ready_tenants = retries.ready_tenants(&tenant_ids, now);
        for (tenant_id, result) in process_tenants_async(&engine, ready_tenants, engine.now()).await
        {
            match result {
                Ok(()) => retries.record_success(&tenant_id),
                Err(error) => {
                    retries.record_failure(tenant_id.clone(), Instant::now(), retry_delay);
                    tracing::warn!(
                        tenant = %tenant_id,
                        retry_after_millis = retry_delay.as_millis(),
                        error = %error,
                        "scheduler failed for tenant; applying tenant-local bounded retry"
                    );
                }
            }
        }

        let blocked_tenants = retries.blocked_tenants();
        let mut next_retry_at = retries.next_retry_at();
        let next_due = match engine
            .next_loaded_scheduled_work_at_excluding_async(&blocked_tenants)
            .await
        {
            Ok(next_due) => next_due,
            Err(error) => {
                tracing::error!(error = %error, "scheduler failed to compute next due work");
                let coordination_retry = Instant::now() + retry_delay;
                next_retry_at = Some(match next_retry_at {
                    Some(current) => current.min(coordination_retry),
                    None => coordination_retry,
                });
                None
            }
        };

        if durable_deadline::wait(
            &engine,
            next_due,
            next_retry_at,
            resample_interval,
            &mut shutdown,
        )
        .await
            == DurableDeadlineWake::Shutdown
        {
            tracing::info!("scheduler shutting down");
            break;
        }
    }
}

#[cfg(test)]
pub(crate) async fn tick_async(engine: &Arc<Engine>) -> Result<()> {
    tick_at_async(engine, engine.now()).await
}

#[cfg(test)]
pub(crate) async fn tick_at_async(engine: &Arc<Engine>, now: Timestamp) -> Result<()> {
    for (tenant_id, result) in process_tenants_async(engine, engine.loaded_tenant_ids(), now).await
    {
        if let Err(error) = result {
            tracing::warn!(tenant = %tenant_id, error = %error, "scheduler failed for tenant");
        }
    }
    Ok(())
}

async fn process_tenants_async(
    engine: &Arc<Engine>,
    tenant_ids: Vec<TenantId>,
    now: Timestamp,
) -> Vec<(TenantId, Result<()>)> {
    let max_concurrent_tenant_ticks = scheduler_tenant_tick_parallelism(tenant_ids.len());
    stream::iter(tenant_ids)
        .map(|tenant_id| {
            let engine = engine.clone();
            async move {
                let result = process_tenant_async(&engine, &tenant_id, now).await;
                (tenant_id, result)
            }
        })
        .buffer_unordered(max_concurrent_tenant_ticks)
        .collect()
        .await
}

fn scheduler_tenant_tick_parallelism(tenant_count: usize) -> usize {
    let available_parallelism = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(4)
        .max(1);
    tenant_count.max(1).min(available_parallelism)
}

#[derive(Default)]
struct TenantRetryBackoff {
    retry_at: BTreeMap<TenantId, Instant>,
}

impl TenantRetryBackoff {
    fn ready_tenants(&mut self, tenant_ids: &[TenantId], now: Instant) -> Vec<TenantId> {
        let loaded = tenant_ids.iter().cloned().collect::<BTreeSet<_>>();
        self.retry_at
            .retain(|tenant_id, _| loaded.contains(tenant_id));

        let mut ready = Vec::with_capacity(tenant_ids.len());
        for tenant_id in tenant_ids {
            match self.retry_at.get(tenant_id).copied() {
                Some(retry_at) if retry_at > now => {}
                Some(_) => {
                    self.retry_at.remove(tenant_id);
                    ready.push(tenant_id.clone());
                }
                None => ready.push(tenant_id.clone()),
            }
        }
        ready
    }

    fn record_success(&mut self, tenant_id: &TenantId) {
        self.retry_at.remove(tenant_id);
    }

    fn record_failure(&mut self, tenant_id: TenantId, now: Instant, retry_delay: Duration) {
        self.retry_at.insert(tenant_id, now + retry_delay);
    }

    fn blocked_tenants(&self) -> BTreeSet<TenantId> {
        self.retry_at.keys().cloned().collect()
    }

    fn next_retry_at(&self) -> Option<Instant> {
        self.retry_at.values().copied().min()
    }
}

async fn process_tenant_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    now: Timestamp,
) -> Result<()> {
    process_due_jobs_async(engine, tenant_id, now).await?;
    process_cron_jobs_async(engine, tenant_id, now).await?;
    Ok(())
}

async fn process_due_jobs_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    now: Timestamp,
) -> Result<()> {
    let due_jobs = engine.claim_due_jobs_async(tenant_id.clone(), now).await?;
    for job in due_jobs {
        let job_id = job.id.clone();
        let execution_id = format!("scheduled:{}", job.id);
        let result = engine
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
            finished_at: engine.now(),
            mutation: job.mutation,
            outcome: if result.is_ok() {
                ScheduledJobOutcome::Completed
            } else {
                ScheduledJobOutcome::Failed
            },
            error: result.as_ref().err().map(ToString::to_string),
        };
        if let Err(error) = engine
            .record_scheduled_job_result_async(tenant_id.clone(), execution_result)
            .await
        {
            tracing::warn!(
                tenant = %tenant_id,
                job_id = %job_id,
                error = %error,
                "scheduled job result bookkeeping failed"
            );
            continue;
        }
        if let Err(error) = engine
            .complete_scheduled_job_async(tenant_id.clone(), job_id.clone())
            .await
        {
            tracing::warn!(
                tenant = %tenant_id,
                job_id = %job_id,
                error = %error,
                "scheduled job completion bookkeeping failed"
            );
            continue;
        }
    }
    Ok(())
}

async fn process_cron_jobs_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    now: Timestamp,
) -> Result<()> {
    let cron_jobs = engine.load_cron_jobs_async(tenant_id.clone()).await?;
    for mut cron in cron_jobs {
        if !cron.enabled || cron.next_run.0 > now.0 {
            continue;
        }

        if let Err(error) =
            dispatch_mutation_async(engine, tenant_id.clone(), cron.mutation.clone()).await
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
        engine
            .update_cron_job_async(tenant_id.clone(), cron)
            .await?;
    }
    Ok(())
}

async fn dispatch_mutation_async(
    engine: &Arc<Engine>,
    tenant_id: TenantId,
    mutation: Mutation,
) -> Result<()> {
    match mutation {
        Mutation::Insert { table, id, fields } => engine
            .insert_document_async_with(
                tenant_id,
                table,
                id,
                fields,
                crate::AsyncMutationContext::anonymous(std::future::pending(), || Ok(())),
            )
            .await
            .map(|_| ()),
        Mutation::Update { table, id, patch } => engine
            .update_document_async(tenant_id, table, id, patch)
            .await
            .map(|_| ()),
        Mutation::Delete { table, id } => engine.delete_document_async(tenant_id, table, id).await,
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;

    #[test]
    fn tenant_scheduler_retry_is_bounded_and_tenant_local() {
        let tenant_a = TenantId::new("retry-a").expect("tenant id");
        let tenant_b = TenantId::new("retry-b").expect("tenant id");
        let started = Instant::now();
        let retry_delay = Duration::from_secs(1);
        let mut retries = TenantRetryBackoff::default();

        retries.record_failure(tenant_a.clone(), started, retry_delay);
        assert_eq!(
            retries.ready_tenants(&[tenant_a.clone(), tenant_b.clone()], started),
            vec![tenant_b.clone()],
            "one contended tenant must not suppress an unrelated tenant"
        );
        assert_eq!(
            retries.next_retry_at(),
            Some(started + retry_delay),
            "retry must be scheduled at the bounded deadline"
        );
        assert_eq!(
            retries.ready_tenants(&[tenant_a.clone(), tenant_b.clone()], started + retry_delay,),
            vec![tenant_a.clone(), tenant_b.clone()],
            "the contended tenant must become eligible at its deadline"
        );

        retries.record_failure(tenant_a.clone(), started, retry_delay);
        retries.record_success(&tenant_a);
        assert!(retries.blocked_tenants().is_empty());

        retries.record_failure(tenant_a, started, retry_delay);
        assert_eq!(
            retries.ready_tenants(std::slice::from_ref(&tenant_b), started),
            vec![tenant_b],
            "unloaded tenants must be removed from retry state"
        );
        assert!(retries.blocked_tenants().is_empty());
    }
}
