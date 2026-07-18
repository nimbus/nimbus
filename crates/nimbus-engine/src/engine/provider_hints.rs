use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use nimbus_core::{Result, SequenceNumber, TenantEventRecord, TenantId};
use nimbus_storage::{PostgresProvider, PostgresProviderNotification};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::persistence::WorkerContext;
use crate::tenant::TenantRuntime;

use super::Engine;
use super::mutations::document_bearing_commit_identity;

#[cfg(test)]
const POSTGRES_HINT_RECONNECT_DELAY: Duration = Duration::from_secs(1);
#[cfg(not(test))]
const POSTGRES_HINT_RECONNECT_DELAY: Duration = Duration::from_millis(250);
#[cfg(test)]
const POLLING_PROVIDER_INTERVAL: Duration = Duration::from_millis(100);
#[cfg(not(test))]
const POLLING_PROVIDER_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy)]
pub(crate) enum ProviderPollWorker {
    MySql,
    LibsqlReplica,
}

impl ProviderPollWorker {
    pub(crate) fn task_name(self) -> &'static str {
        match self {
            Self::MySql => "mysql_provider_poll",
            Self::LibsqlReplica => "libsql_replica_provider_poll",
        }
    }

    fn failure_message(self) -> &'static str {
        match self {
            Self::MySql => "failed to poll MySQL provider state",
            Self::LibsqlReplica => "failed to poll replica-connected SQLite provider state",
        }
    }
}

impl Engine {
    pub(crate) fn ensure_provider_background_tasks_started(self: &Arc<Self>) {
        let Some(runtime_hooks) = self.persistence_provider.runtime_hooks() else {
            return;
        };
        if self
            .provider_hint_worker_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let task_name = runtime_hooks.task_name();
        if let Some(backend_info) = runtime_hooks.backend_info() {
            debug!(backend = %backend_info, task = task_name, "starting provider runtime hooks");
        }
        let ctx = WorkerContext {
            engine: self.clone(),
            shutdown: self.engine_executor.shutdown_token(),
        };
        self.spawn_background(task_name, async move {
            runtime_hooks.spawn_workers(ctx).await;
        });
    }

    pub(crate) async fn run_provider_notification_listener(
        self: Arc<Self>,
        provider: Arc<PostgresProvider>,
        shutdown: CancellationToken,
    ) {
        #[cfg(any(test, debug_assertions))]
        Engine::assert_running_on_background_task("postgres_provider_hints");

        let mut first_attach = true;
        loop {
            let mut listener = match provider.connect_notification_listener().await {
                Ok(listener) => listener,
                Err(error) => {
                    warn!(error = %error, "failed to connect Postgres hint listener");
                    if sleep_or_stop(POSTGRES_HINT_RECONNECT_DELAY, &shutdown).await {
                        return;
                    }
                    continue;
                }
            };
            if let Err(error) = self
                .catch_up_postgres_provider_after_listener_attach()
                .await
            {
                warn!(
                    error = %error,
                    "failed to catch up Postgres state after listener attach"
                );
            }
            if first_attach {
                self.provider_hint_listener_ready
                    .store(true, Ordering::Release);
            }
            first_attach = false;

            loop {
                tokio::select! {
                    _ = shutdown.cancelled() => {
                        return;
                    }
                    next = listener.recv() => {
                        match next {
                            Some(Ok(notification)) => {
                                if let Err(error) =
                                    self.handle_postgres_provider_notification(notification).await
                                {
                                    warn!(error = %error, "failed to apply Postgres hint");
                                }
                            }
                            Some(Err(error)) => {
                                warn!(error = %error, "Postgres hint listener lost its connection");
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }

            if sleep_or_stop(POSTGRES_HINT_RECONNECT_DELAY, &shutdown).await {
                return;
            }
        }
    }

    async fn handle_postgres_provider_notification(
        self: &Arc<Self>,
        notification: PostgresProviderNotification,
    ) -> Result<()> {
        let tenant_id = notification.tenant_id.clone();
        if let Some(runtime) = self.loaded_runtime(&tenant_id) {
            self.refresh_loaded_postgres_tenant_async(runtime, &tenant_id, &notification)
                .await?;
        } else if notification.scheduler_changed {
            self.load_tenant_with_scheduled_work_if_present(tenant_id.clone())
                .await?;
        }

        if notification.scheduler_changed {
            self.wake_scheduler();
        }
        Ok(())
    }

    fn loaded_runtime(&self, tenant_id: &TenantId) -> Option<Arc<TenantRuntime>> {
        self.tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .get(tenant_id)
            .cloned()
    }

    async fn refresh_loaded_postgres_tenant_async(
        &self,
        runtime: Arc<TenantRuntime>,
        tenant_id: &TenantId,
        notification: &PostgresProviderNotification,
    ) -> Result<()> {
        self.catch_up_loaded_provider_tenant_async(
            runtime,
            tenant_id,
            notification.schema_changed,
            notification.journal_changed,
            true,
        )
        .await
    }

    pub(super) async fn catch_up_loaded_provider_tenant_async(
        &self,
        runtime: Arc<TenantRuntime>,
        tenant_id: &TenantId,
        refresh_schema: bool,
        refresh_journal: bool,
        emit_trigger_candidates: bool,
    ) -> Result<()> {
        let _operation = runtime.enter_operation(tenant_id)?;

        if refresh_schema {
            runtime.store.invalidate_schema_cache();
            self.refresh_loaded_schema_from_store_async(&runtime)
                .await?;
        }

        if refresh_journal {
            let next_sequence = SequenceNumber(runtime.applied_head().0.saturating_add(1));
            let (progress, records) = runtime
                .store
                .recover_journal_tail_async(&runtime.read_storage, next_sequence)
                .await?;
            let commits = records
                .iter()
                .map(TenantEventRecord::as_commit_entry)
                .collect::<Vec<_>>();
            if !commits.is_empty() {
                runtime.invalidate_document_cache_for_commits(commits.iter());
            }
            runtime
                .sync_mutation_journal_progress_async(progress)
                .await?;
            if !commits.is_empty() {
                // Unlike the live mutation-queue apply path, a raw journal
                // tail re-read here can span more than one originating
                // operation, so identity must be proven kind-aware over the
                // still-unflattened records, not assumed from `len() == 1`.
                let commit_identity = document_bearing_commit_identity(&records);
                self.process_applied_commit_batch_fanout(
                    runtime.clone(),
                    &commits,
                    commit_identity,
                    emit_trigger_candidates,
                );
                let _ = self.enqueue_provider_catch_up_commit_observers(runtime, &commits);
            }
        }

        Ok(())
    }
    async fn catch_up_postgres_provider_after_listener_attach(self: &Arc<Self>) -> Result<()> {
        let loaded = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .iter()
            .map(|(tenant_id, runtime)| (tenant_id.clone(), runtime.clone()))
            .collect::<Vec<_>>();

        for (tenant_id, runtime) in loaded {
            // PostgreSQL's LISTEN contract requires an authoritative state
            // inspection after the listener commit before the process can rely
            // on subsequent notifications. That authoritative catch-up must
            // cover both schema and journal-backed state on every attach:
            // startup can race the first listener becoming live, and later
            // reconnects can miss schema notifications just as easily as
            // journal notifications while the LISTEN connection is down.
            if let Err(error) = self
                .catch_up_loaded_provider_tenant_async(
                    runtime.clone(),
                    &tenant_id,
                    true,
                    true,
                    true,
                )
                .await
            {
                if self.background_shutdown_started() {
                    return Err(error);
                }
                runtime.record_provider_catch_up_failure();
                warn!(
                    tenant = %tenant_id,
                    error = %error,
                    "failed to catch up loaded provider tenant after listener attach; continuing with other tenants"
                );
            }
        }

        self.load_tenants_with_scheduled_work_async().await?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn catch_up_provider_after_listener_attach_for_testing(
        self: &Arc<Self>,
    ) -> Result<()> {
        self.catch_up_postgres_provider_after_listener_attach()
            .await
    }

    pub(crate) async fn run_provider_poll_worker(
        self: Arc<Self>,
        worker: ProviderPollWorker,
        shutdown: CancellationToken,
    ) {
        #[cfg(any(test, debug_assertions))]
        Engine::assert_running_on_background_task(worker.task_name());

        self.provider_hint_listener_ready
            .store(true, Ordering::Release);
        let mut last_next_due = None;
        loop {
            match self.poll_provider_once(last_next_due).await {
                Ok(next_due) => last_next_due = next_due,
                Err(error) => warn!(error = %error, "{}", worker.failure_message()),
            }
            if sleep_or_stop(POLLING_PROVIDER_INTERVAL, &shutdown).await {
                return;
            }
        }
    }

    async fn poll_provider_once(
        self: &Arc<Self>,
        last_next_due: Option<nimbus_core::Timestamp>,
    ) -> Result<Option<nimbus_core::Timestamp>> {
        let loaded = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .iter()
            .map(|(tenant_id, runtime)| (tenant_id.clone(), runtime.clone()))
            .collect::<Vec<_>>();

        for (tenant_id, runtime) in &loaded {
            let refresh = async {
                let refresh_plan = runtime
                    .store
                    .plan_loaded_runtime_refresh_async(
                        &runtime.read_storage,
                        runtime.schema().as_ref(),
                        runtime.durable_head(),
                        runtime.applied_head(),
                    )
                    .await?;
                let refresh_schema = refresh_plan.refresh_schema;
                let refresh_journal = refresh_plan.refresh_journal;
                if refresh_schema || refresh_journal {
                    self.catch_up_loaded_provider_tenant_async(
                        runtime.clone(),
                        tenant_id,
                        refresh_schema,
                        refresh_journal,
                        true,
                    )
                    .await?;
                }
                Ok(())
            }
            .await;
            if let Err(error) = refresh {
                if self.background_shutdown_started() {
                    return Err(error);
                }
                runtime.record_provider_catch_up_failure();
                warn!(
                    tenant = %tenant_id,
                    error = %error,
                    "failed to refresh loaded provider tenant; continuing provider poll"
                );
            }
        }

        let loaded_tenant_ids = loaded
            .iter()
            .map(|(tenant_id, _)| tenant_id.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut loaded_unloaded_tenant = false;
        for tenant_id in self.persistence_provider.list_tenants().await? {
            if loaded_tenant_ids.contains(&tenant_id) {
                continue;
            }
            match self
                .load_tenant_with_scheduled_work_if_present(tenant_id.clone())
                .await
            {
                Ok(loaded) => loaded_unloaded_tenant |= loaded,
                Err(error) => {
                    if self.background_shutdown_started() {
                        return Err(error);
                    }
                    if let Some(runtime) = self.loaded_runtime(&tenant_id) {
                        runtime.record_provider_catch_up_failure();
                    }
                    warn!(
                        tenant = %tenant_id,
                        error = %error,
                        "failed to load provider tenant with scheduled work; continuing provider poll"
                    );
                }
            }
        }

        let next_due = self.next_loaded_scheduled_work_at_async().await?;
        if loaded_unloaded_tenant || next_due != last_next_due {
            self.wake_scheduler();
        }
        Ok(next_due)
    }
}

async fn sleep_or_stop(delay: Duration, shutdown: &CancellationToken) -> bool {
    tokio::select! {
        _ = shutdown.cancelled() => true,
        _ = tokio::time::sleep(delay) => false,
    }
}
