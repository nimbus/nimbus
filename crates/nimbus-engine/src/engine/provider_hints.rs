use std::sync::Arc;
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use std::sync::atomic::Ordering;
// The reconnect/poll delays and the shared `sleep_or_stop` helper they drive
// exist only for the remote-provider workers below.
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use std::time::Duration;

use nimbus_core::{Result, SequenceNumber, TenantEventRecord, TenantId};
#[cfg(feature = "postgres")]
use nimbus_storage::{PostgresProvider, PostgresProviderNotification};
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use tokio_util::sync::CancellationToken;
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use tracing::debug;
#[cfg(any(test, feature = "libsql", feature = "mysql", feature = "postgres"))]
use tracing::warn;

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use crate::persistence::WorkerContext;
use crate::tenant::TenantRuntime;

use super::Engine;
use super::mutations::document_bearing_commit_identity;

#[cfg(all(test, feature = "postgres"))]
const POSTGRES_HINT_RECONNECT_DELAY: Duration = Duration::from_secs(1);
#[cfg(all(not(test), feature = "postgres"))]
const POSTGRES_HINT_RECONNECT_DELAY: Duration = Duration::from_millis(250);
#[cfg(any(feature = "libsql", feature = "mysql"))]
const POLLING_PROVIDER_INTERVAL: Duration = Duration::from_millis(500);
#[cfg(any(feature = "libsql", feature = "mysql"))]
const PROVIDER_TENANT_SWEEP_PAGE_SIZE: usize = 8;

/// Which polling provider a hint worker serves. PostgreSQL is absent by
/// design: it is driven by `LISTEN`/`NOTIFY` rather than polling, so it has its
/// own listener path below.
#[cfg(any(feature = "libsql", feature = "mysql"))]
#[derive(Clone, Copy)]
pub(crate) enum ProviderPollWorker {
    #[cfg(feature = "mysql")]
    MySql,
    #[cfg(feature = "libsql")]
    LibsqlReplica,
}

#[cfg(any(feature = "libsql", feature = "mysql"))]
impl ProviderPollWorker {
    pub(crate) fn task_name(self) -> &'static str {
        match self {
            #[cfg(feature = "mysql")]
            Self::MySql => "mysql_provider_poll",
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica => "libsql_replica_provider_poll",
        }
    }

    fn failure_message(self) -> &'static str {
        match self {
            #[cfg(feature = "mysql")]
            Self::MySql => "failed to poll MySQL provider state",
            #[cfg(feature = "libsql")]
            Self::LibsqlReplica => "failed to poll replica-connected SQLite provider state",
        }
    }
}

impl Engine {
    /// Starts the one background worker the configured remote provider needs.
    /// Paired with the no-provider definition below: an embedded-only build has
    /// no hint worker to start, so the call sites stay unconditional.
    #[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
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
        if let Err((error, _future)) = self.try_spawn_background(task_name, async move {
            runtime_hooks.spawn_workers(ctx).await;
        }) {
            self.provider_hint_worker_started
                .store(false, Ordering::Release);
            if !self.background_shutdown_started() {
                warn!(error = %error, task = task_name, "failed to start provider runtime hooks");
            }
        }
    }

    #[cfg(not(any(feature = "libsql", feature = "mysql", feature = "postgres")))]
    pub(crate) fn ensure_provider_background_tasks_started(self: &Arc<Self>) {}

    #[cfg(feature = "postgres")]
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

    #[cfg(feature = "postgres")]
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

    #[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
    fn loaded_runtime(&self, tenant_id: &TenantId) -> Option<Arc<TenantRuntime>> {
        self.tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .get(tenant_id)
            .cloned()
    }

    #[cfg(feature = "postgres")]
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
        let mut observer_records = Vec::new();

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
            }
            observer_records = records;
        }

        // Schema refresh deliberately follows journal reconciliation. A
        // provider notification may coalesce both hints, and publishing a new
        // schema against stale serving state would create a projection that no
        // durable source frontier actually represents.
        let changed_schema_tables = if refresh_schema {
            runtime.store.invalidate_schema_cache();
            self.refresh_loaded_schema_from_store_async(&runtime)
                .await?
        } else {
            Vec::new()
        };

        if !observer_records.is_empty() || !changed_schema_tables.is_empty() {
            let projection_token = self.provider_projection_token(&runtime).await?;
            if !observer_records.is_empty() {
                let _ = self.enqueue_provider_catch_up_commit_observers(
                    runtime.clone(),
                    &observer_records,
                    projection_token,
                );
            }
            for table in changed_schema_tables {
                self.notify_table_schema_change_observers(
                    runtime.tenant_id(),
                    &table,
                    projection_token,
                );
            }
        }

        Ok(())
    }
    // PostgreSQL's listener is the production caller, but the engine tests also
    // drive this catch-up directly against embedded fixtures, so it is built for
    // every test configuration.
    #[cfg(any(test, feature = "postgres"))]
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

    #[cfg(any(feature = "libsql", feature = "mysql"))]
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
        let mut tenant_sweep_after = None;
        loop {
            match self
                .poll_provider_once(last_next_due, tenant_sweep_after.as_ref())
                .await
            {
                Ok((next_due, next_sweep_after)) => {
                    last_next_due = next_due;
                    tenant_sweep_after = next_sweep_after;
                }
                Err(error) => warn!(error = %error, "{}", worker.failure_message()),
            }
            if sleep_or_stop(POLLING_PROVIDER_INTERVAL, &shutdown).await {
                return;
            }
        }
    }

    #[cfg(any(feature = "libsql", feature = "mysql"))]
    async fn poll_provider_once(
        self: &Arc<Self>,
        last_next_due: Option<nimbus_core::Timestamp>,
        tenant_sweep_after: Option<&TenantId>,
    ) -> Result<(Option<nimbus_core::Timestamp>, Option<TenantId>)> {
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
        let tenant_page = self
            .persistence_provider
            .list_tenants_page(tenant_sweep_after, PROVIDER_TENANT_SWEEP_PAGE_SIZE)
            .await?;
        let next_sweep_after = tenant_page.next_after;
        for tenant_id in tenant_page.tenant_ids {
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
        Ok((next_due, next_sweep_after))
    }
}

#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
async fn sleep_or_stop(delay: Duration, shutdown: &CancellationToken) -> bool {
    tokio::select! {
        _ = shutdown.cancelled() => true,
        _ = tokio::time::sleep(delay) => false,
    }
}
