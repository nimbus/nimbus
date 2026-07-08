use std::sync::Arc;

use nimbus_core::{
    CronJob, CronSchedule, DocumentId, Error, Mutation, Result, ScheduledJob, ScheduledJobOutcome,
    ScheduledJobResult, TenantId,
};
use nimbus_engine::Engine;
use serde_json::{Map, Value, json};

use crate::identity::is_reserved_tenant_id;
use crate::keys::{cron_job_document_id, scheduled_job_document_id};
use crate::schema::SystemTable;

use super::{
    delete_system_document_if_exists_async, ensure_system_tenant_async, object_fields,
    query_system_documents_by_eq_async, upsert_system_document_async,
};

pub(crate) async fn record_scheduled_job_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    job: &ScheduledJob,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    upsert_system_document_async(
        engine,
        SystemTable::ScheduledJobs,
        &scheduled_job_document_id(tenant_id, &job.id),
        scheduled_job_fields(tenant_id, &job.run_at, &job.mutation, "pending", None)?,
    )
    .await
}

pub async fn record_scheduled_job_result_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    result: &ScheduledJobResult,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let status = match result.outcome {
        ScheduledJobOutcome::Completed => "completed",
        ScheduledJobOutcome::Failed => "failed",
    };
    upsert_system_document_async(
        engine,
        SystemTable::ScheduledJobs,
        &scheduled_job_document_id(tenant_id, &result.id),
        scheduled_job_fields(
            tenant_id,
            &result.run_at,
            &result.mutation,
            status,
            Some(result),
        )?,
    )
    .await
}

pub async fn delete_scheduled_job_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    job_id: &DocumentId,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    delete_system_document_if_exists_async(
        engine,
        SystemTable::ScheduledJobs,
        &scheduled_job_document_id(tenant_id, job_id),
    )
    .await
}

pub(crate) async fn record_cron_job_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    cron: &CronJob,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    upsert_system_document_async(
        engine,
        SystemTable::CronJobs,
        &cron_job_document_id(tenant_id, &cron.name),
        cron_job_fields(tenant_id, cron)?,
    )
    .await
}

pub async fn delete_cron_job_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    name: &str,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    delete_system_document_if_exists_async(
        engine,
        SystemTable::CronJobs,
        &cron_job_document_id(tenant_id, name),
    )
    .await
}

pub async fn sync_scheduler_state_for_tenant_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let scheduled_jobs = engine.list_scheduled_jobs_async(tenant_id.clone()).await?;
    let active_scheduled_ids = scheduled_jobs
        .iter()
        .map(|job| scheduled_job_document_id(tenant_id, &job.id))
        .collect::<std::collections::BTreeSet<_>>();
    for job in &scheduled_jobs {
        record_scheduled_job_state_async(engine, tenant_id, job).await?;
    }
    delete_stale_scheduler_documents_async(
        engine,
        SystemTable::ScheduledJobs,
        tenant_id,
        "pending",
        &active_scheduled_ids,
    )
    .await?;

    let cron_jobs = engine.load_cron_jobs_async(tenant_id.clone()).await?;
    let active_cron_ids = cron_jobs
        .iter()
        .map(|cron| cron_job_document_id(tenant_id, &cron.name))
        .collect::<std::collections::BTreeSet<_>>();
    for cron in &cron_jobs {
        record_cron_job_state_async(engine, tenant_id, cron).await?;
    }
    delete_stale_scheduler_documents_async(
        engine,
        SystemTable::CronJobs,
        tenant_id,
        "active",
        &active_cron_ids,
    )
    .await
}

pub(crate) async fn sync_all_scheduler_state_async(engine: &Arc<Engine>) -> Result<()> {
    let tenants = engine.list_tenants_async().await?;
    for tenant_id in tenants {
        if is_reserved_tenant_id(&tenant_id) {
            continue;
        }
        sync_scheduler_state_for_tenant_async(engine, &tenant_id).await?;
    }
    Ok(())
}

async fn delete_stale_scheduler_documents_async(
    engine: &Arc<Engine>,
    table: SystemTable,
    tenant_id: &TenantId,
    stale_status: &str,
    active_document_ids: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let system_tenant = crate::identity::system_tenant_id()?;
    let table_name = table.table_name()?;
    let documents = query_system_documents_by_eq_async(
        engine,
        table,
        [
            ("tenantId", json!(tenant_id.as_str())),
            ("status", json!(stale_status)),
        ],
    )
    .await?;
    for document in documents {
        if active_document_ids.contains(&document.id.to_string()) {
            continue;
        }
        engine
            .delete_document_async(system_tenant.clone(), table_name.clone(), document.id)
            .await?;
    }
    Ok(())
}

fn scheduled_job_fields(
    tenant_id: &TenantId,
    run_at: &nimbus_core::Timestamp,
    mutation: &Mutation,
    status: &str,
    result: Option<&ScheduledJobResult>,
) -> Result<Map<String, Value>> {
    let mut fields = object_fields(json!({
        "tenantId": tenant_id.as_str(),
        "functionPath": mutation_function_path(mutation),
        "scheduledTime": run_at.0,
        "status": status,
        "args": mutation_payload(mutation)?,
    }));
    if let Some(result) = result {
        fields.insert(
            "result".to_owned(),
            json!({
                "finishedAt": result.finished_at.0,
                "outcome": match result.outcome {
                    ScheduledJobOutcome::Completed => "completed",
                    ScheduledJobOutcome::Failed => "failed",
                },
                "error": result.error.as_deref(),
            }),
        );
    }
    Ok(fields)
}

fn cron_job_fields(tenant_id: &TenantId, cron: &CronJob) -> Result<Map<String, Value>> {
    let mut fields = object_fields(json!({
        "tenantId": tenant_id.as_str(),
        "name": cron.name.as_str(),
        "schedule": cron_schedule_label(&cron.schedule),
        "functionPath": mutation_function_path(&cron.mutation),
        "nextRunAt": cron.next_run.0,
        "status": if cron.enabled { "active" } else { "paused" },
    }));
    if let Some(last_run) = cron.last_run {
        fields.insert("lastRunAt".to_owned(), json!(last_run.0));
    }
    Ok(fields)
}

fn mutation_function_path(mutation: &Mutation) -> String {
    match mutation {
        Mutation::Insert { table, .. } => format!("documents.{}.insert", table.as_str()),
        Mutation::Update { table, .. } => format!("documents.{}.update", table.as_str()),
        Mutation::Delete { table, .. } => format!("documents.{}.delete", table.as_str()),
    }
}

fn mutation_payload(mutation: &Mutation) -> Result<Value> {
    serde_json::to_value(mutation).map_err(|error| Error::Serialization(error.to_string()))
}

fn cron_schedule_label(schedule: &CronSchedule) -> String {
    match schedule {
        CronSchedule::Interval { seconds } => format!("interval:{seconds}s"),
    }
}
