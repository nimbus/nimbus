use std::future::Future;
use std::sync::Arc;

use futures::FutureExt;
use neovex_core::{
    Document, PrincipalContext, Query, Result, SequenceNumber, TableSchema, TenantId,
    policy_revision_id,
};
use neovex_storage::TenantReadStorage;

use crate::tenant::{QueryPlanMetricKind, QueryPlanMetricOperation, TenantRuntime};

use super::super::queries::{
    query_documents_for_docs_with_principal,
    query_documents_for_snapshot_and_principal_cancellable,
    should_use_materialized_surface_for_query, snapshot_table_documents,
};

#[doc(hidden)]
pub struct SubscriptionBootstrapCancellation<Fut, Check> {
    cancel_wait: Fut,
    check_cancel: Check,
}

impl<Fut, Check> SubscriptionBootstrapCancellation<Fut, Check> {
    pub fn new(cancel_wait: Fut, check_cancel: Check) -> Self {
        Self {
            cancel_wait,
            check_cancel,
        }
    }

    pub(super) fn into_parts(self) -> (Fut, Check) {
        (self.cancel_wait, self.check_cancel)
    }
}

pub(crate) fn table_policy_revision(table_schema: Option<&TableSchema>) -> Result<String> {
    match table_schema {
        Some(table_schema) => table_schema.access_policy_revision(),
        None => policy_revision_id(None),
    }
}

#[derive(Debug)]
struct SubscriptionBootstrapResult {
    documents: Vec<Document>,
    covered_sequence: SequenceNumber,
}

pub(super) fn evaluate_subscription_bootstrap_cancellable_for_principal(
    runtime: &TenantRuntime,
    query: &Query,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<(Vec<Document>, SequenceNumber)> {
    let result = if let Some(result) =
        evaluate_subscription_bootstrap_with_materialized_surface_cancellable_for_principal(
            runtime,
            query,
            principal,
            check_cancel,
        )? {
        result
    } else {
        let schema = runtime.schema();
        let snapshot = runtime.store.read_snapshot()?;
        let covered_sequence = snapshot.applied_sequence()?;
        let (plan_kind, documents) = query_documents_for_snapshot_and_principal_cancellable(
            &snapshot,
            query,
            schema.get_table(&query.table),
            principal,
            check_cancel,
        )?;
        runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
        SubscriptionBootstrapResult {
            documents,
            covered_sequence,
        }
    };
    Ok((result.documents, result.covered_sequence))
}

fn evaluate_subscription_bootstrap_with_materialized_surface_cancellable_for_principal(
    runtime: &TenantRuntime,
    query: &Query,
    principal: &PrincipalContext,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Option<SubscriptionBootstrapResult>> {
    let schema = runtime.schema();
    if !should_use_materialized_surface_for_query(schema.get_table(&query.table), query, principal)?
    {
        return Ok(None);
    }

    let required_sequence = runtime.applied_head();
    let snapshot = runtime.load_materialized_serving_snapshot_cancellable(
        runtime.store.as_ref(),
        &query.table,
        required_sequence,
        check_cancel,
    )?;
    runtime.record_query_plan_metric(
        QueryPlanMetricOperation::Query,
        QueryPlanMetricKind::FullScan,
    );
    runtime.record_materialized_query_evaluation();
    Ok(Some(SubscriptionBootstrapResult {
        documents: query_documents_for_docs_with_principal(
            snapshot_table_documents(
                &snapshot,
                &query.table,
                "subscription bootstrap materialized evaluation",
            )?,
            &schema,
            query,
            principal,
        )?,
        covered_sequence: snapshot.covered_sequence(),
    }))
}

pub(super) async fn evaluate_subscription_bootstrap_async_for_principal<Fut, Check>(
    runtime: Arc<TenantRuntime>,
    tenant_id: TenantId,
    query: Query,
    principal: PrincipalContext,
    cancel_wait: Fut,
    check_cancel: Check,
) -> Result<(Vec<Document>, SequenceNumber)>
where
    Fut: Future<Output = ()> + Send,
    Check: Fn() -> Result<()> + Send + Sync + 'static,
{
    let cancel_wait = cancel_wait.shared();
    let check_cancel = Arc::new(check_cancel);
    let _operation = runtime.enter_operation(&tenant_id)?;
    let result = if let Some(result) =
        evaluate_subscription_bootstrap_with_materialized_surface_async_for_principal(
            runtime.clone(),
            query.clone(),
            principal.clone(),
            cancel_wait.clone(),
            check_cancel.clone(),
        )
        .await?
    {
        result
    } else {
        let schema = runtime.schema();
        let table_schema = schema.get_table(&query.table).cloned();
        let query_for_task = query.clone();
        let principal_for_task = principal.clone();
        let (plan_kind, result) = runtime
            .read_storage
            .execute_cancellable(
                cancel_wait,
                {
                    let check_cancel = check_cancel.clone();
                    move || check_cancel()
                },
                move |store, check_cancel| {
                    let snapshot = store.read_snapshot()?;
                    let covered_sequence = snapshot.applied_sequence()?;
                    let (plan_kind, documents) =
                        query_documents_for_snapshot_and_principal_cancellable(
                            &snapshot,
                            &query_for_task,
                            table_schema.as_ref(),
                            &principal_for_task,
                            check_cancel,
                        )?;
                    Ok((
                        plan_kind,
                        SubscriptionBootstrapResult {
                            documents,
                            covered_sequence,
                        },
                    ))
                },
            )
            .await?;
        runtime.record_query_plan_metric(QueryPlanMetricOperation::Query, plan_kind);
        result
    };
    Ok((result.documents, result.covered_sequence))
}

async fn evaluate_subscription_bootstrap_with_materialized_surface_async_for_principal<Fut, Check>(
    runtime: Arc<TenantRuntime>,
    query: Query,
    principal: PrincipalContext,
    cancel_wait: Fut,
    check_cancel: Arc<Check>,
) -> Result<Option<SubscriptionBootstrapResult>>
where
    Fut: Future<Output = ()> + Clone + Send,
    Check: Fn() -> Result<()> + Send + Sync + 'static,
{
    let required_sequence = runtime.applied_head();
    let schema = runtime.schema();
    if !should_use_materialized_surface_for_query(
        schema.get_table(&query.table),
        &query,
        &principal,
    )? {
        return Ok(None);
    }

    let snapshot = if let Some(snapshot) =
        runtime.materialized_serving_snapshot_for_table(&query.table, required_sequence)
    {
        snapshot
    } else {
        let runtime_for_task = runtime.clone();
        let table_for_task = query.table.clone();
        runtime
            .read_storage
            .execute_cancellable(
                cancel_wait,
                {
                    let check_cancel = check_cancel.clone();
                    move || check_cancel()
                },
                move |store, check_cancel| {
                    runtime_for_task.load_materialized_serving_snapshot_cancellable(
                        store.as_ref(),
                        &table_for_task,
                        required_sequence,
                        check_cancel,
                    )
                },
            )
            .await?
    };
    runtime.record_query_plan_metric(
        QueryPlanMetricOperation::Query,
        QueryPlanMetricKind::FullScan,
    );
    runtime.record_materialized_query_evaluation();
    Ok(Some(SubscriptionBootstrapResult {
        documents: query_documents_for_docs_with_principal(
            snapshot_table_documents(
                &snapshot,
                &query.table,
                "subscription bootstrap materialized evaluation",
            )?,
            &schema,
            &query,
            &principal,
        )?,
        covered_sequence: snapshot.covered_sequence(),
    }))
}
