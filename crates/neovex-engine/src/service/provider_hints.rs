use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use neovex_core::{Result, SequenceNumber, TenantId};
use neovex_storage::{
    LibsqlReplicaProvider, MySqlProvider, PostgresProvider, PostgresProviderNotification,
};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::persistence::TenantPersistence;
use crate::tenant::TenantRuntime;

use super::Service;

#[cfg(test)]
const POSTGRES_HINT_RECONNECT_DELAY: Duration = Duration::from_secs(1);
#[cfg(not(test))]
const POSTGRES_HINT_RECONNECT_DELAY: Duration = Duration::from_millis(250);
#[cfg(test)]
const POLLING_PROVIDER_INTERVAL: Duration = Duration::from_millis(100);
#[cfg(not(test))]
const POLLING_PROVIDER_INTERVAL: Duration = Duration::from_millis(500);

impl Service {
    pub(crate) fn ensure_provider_background_tasks_started(self: &Arc<Self>) {
        let Some(background) = self.provider_background_task() else {
            return;
        };
        if self
            .provider_hint_worker_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let service = self.clone();
        let shutdown = self.engine_executor.shutdown_token();
        self.spawn_background(background.task_name(), async move {
            background.run(service, shutdown).await;
        });
    }

    fn provider_background_task(&self) -> Option<ProviderBackgroundTask> {
        if let Some(provider) = self.postgres_provider() {
            return Some(ProviderBackgroundTask::Postgres(provider));
        }
        if let Some(provider) = self.libsql_replica_provider() {
            return Some(ProviderBackgroundTask::LibsqlReplica(provider));
        }
        self.mysql_provider().map(ProviderBackgroundTask::MySql)
    }

    async fn run_postgres_provider_hint_worker(
        self: Arc<Self>,
        provider: Arc<PostgresProvider>,
        shutdown: CancellationToken,
    ) {
        #[cfg(any(test, debug_assertions))]
        Service::assert_running_on_background_task("postgres_provider_hints");

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
        )
        .await
    }

    pub(super) async fn catch_up_loaded_provider_tenant_async(
        &self,
        runtime: Arc<TenantRuntime>,
        tenant_id: &TenantId,
        refresh_schema: bool,
        refresh_journal: bool,
    ) -> Result<()> {
        let _operation = runtime.enter_operation(tenant_id)?;

        if refresh_schema {
            runtime.store.invalidate_schema_cache();
            self.refresh_loaded_schema_from_store_async(&runtime)
                .await?;
        }

        if refresh_journal {
            let next_sequence = SequenceNumber(runtime.applied_head().0.saturating_add(1));
            let (progress, commits) = match &runtime.store {
                TenantPersistence::Postgres(store) => {
                    let progress = store.recover_durable_journal_async().await?;
                    let commits = if progress.applied_head.0 >= next_sequence.0 {
                        store.read_commit_log_from_async(next_sequence).await?
                    } else {
                        Vec::new()
                    };
                    (progress, commits)
                }
                TenantPersistence::Redb(_)
                | TenantPersistence::Sqlite(_)
                | TenantPersistence::LibsqlReplica(_)
                | TenantPersistence::MySql(_) => {
                    runtime
                        .read_storage
                        .execute(move |store| {
                            let progress = store.recover_durable_journal()?;
                            let commits = if progress.applied_head.0 >= next_sequence.0 {
                                store.read_commit_log_from(next_sequence)?
                            } else {
                                Vec::new()
                            };
                            Ok((progress, commits))
                        })
                        .await?
                }
            };
            if !commits.is_empty() {
                runtime.invalidate_document_cache_for_commits(commits.iter());
            }
            runtime.sync_mutation_journal_progress(progress);
            if !commits.is_empty() {
                self.process_applied_commit_batch(runtime, &commits);
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
            self.catch_up_loaded_provider_tenant_async(runtime, &tenant_id, true, true)
                .await?;
        }

        self.load_tenants_with_scheduled_work_async().await?;
        Ok(())
    }

    async fn run_mysql_provider_poll_worker(
        self: Arc<Self>,
        _provider: Arc<MySqlProvider>,
        shutdown: CancellationToken,
    ) {
        #[cfg(any(test, debug_assertions))]
        Service::assert_running_on_background_task("mysql_provider_poll");

        self.provider_hint_listener_ready
            .store(true, Ordering::Release);
        let mut last_next_due = None;
        loop {
            match self.poll_provider_once(last_next_due).await {
                Ok(next_due) => last_next_due = next_due,
                Err(error) => warn!(error = %error, "failed to poll MySQL provider state"),
            }
            if sleep_or_stop(POLLING_PROVIDER_INTERVAL, &shutdown).await {
                return;
            }
        }
    }

    async fn run_libsql_replica_provider_poll_worker(
        self: Arc<Self>,
        _provider: Arc<LibsqlReplicaProvider>,
        shutdown: CancellationToken,
    ) {
        #[cfg(any(test, debug_assertions))]
        Service::assert_running_on_background_task("libsql_replica_provider_poll");

        self.provider_hint_listener_ready
            .store(true, Ordering::Release);
        let mut last_next_due = None;
        loop {
            match self.poll_provider_once(last_next_due).await {
                Ok(next_due) => last_next_due = next_due,
                Err(error) => warn!(
                    error = %error,
                    "failed to poll replica-connected SQLite provider state"
                ),
            }
            if sleep_or_stop(POLLING_PROVIDER_INTERVAL, &shutdown).await {
                return;
            }
        }
    }

    async fn poll_provider_once(
        self: &Arc<Self>,
        last_next_due: Option<neovex_core::Timestamp>,
    ) -> Result<Option<neovex_core::Timestamp>> {
        let loaded = self
            .tenants
            .read()
            .expect("tenant registry lock should not be poisoned")
            .iter()
            .map(|(tenant_id, runtime)| (tenant_id.clone(), runtime.clone()))
            .collect::<Vec<_>>();

        for (tenant_id, runtime) in &loaded {
            if matches!(&runtime.store, TenantPersistence::MySql(_)) {
                runtime.store.invalidate_schema_cache();
            }
            let (store_schema, store_progress) = runtime
                .read_storage
                .execute(|store| Ok((store.load_schema()?, store.journal_progress()?)))
                .await?;
            let refresh_schema = store_schema != *runtime.schema();
            let refresh_journal = store_progress.durable_head.0 > runtime.durable_head().0
                || store_progress.applied_head.0 > runtime.applied_head().0;
            if refresh_schema || refresh_journal {
                self.catch_up_loaded_provider_tenant_async(
                    runtime.clone(),
                    tenant_id,
                    refresh_schema,
                    refresh_journal,
                )
                .await?;
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
            loaded_unloaded_tenant |= self
                .load_tenant_with_scheduled_work_if_present(tenant_id)
                .await?;
        }

        let next_due = self.next_loaded_scheduled_work_at_async().await?;
        if loaded_unloaded_tenant || next_due != last_next_due {
            self.wake_scheduler();
        }
        Ok(next_due)
    }
}

enum ProviderBackgroundTask {
    Postgres(Arc<PostgresProvider>),
    LibsqlReplica(Arc<LibsqlReplicaProvider>),
    MySql(Arc<MySqlProvider>),
}

impl ProviderBackgroundTask {
    fn task_name(&self) -> &'static str {
        match self {
            Self::Postgres(_) => "postgres_provider_hints",
            Self::LibsqlReplica(_) => "libsql_replica_provider_poll",
            Self::MySql(_) => "mysql_provider_poll",
        }
    }

    async fn run(self, service: Arc<Service>, shutdown: CancellationToken) {
        match self {
            Self::Postgres(provider) => {
                service
                    .run_postgres_provider_hint_worker(provider, shutdown)
                    .await;
            }
            Self::LibsqlReplica(provider) => {
                service
                    .run_libsql_replica_provider_poll_worker(provider, shutdown)
                    .await;
            }
            Self::MySql(provider) => {
                service
                    .run_mysql_provider_poll_worker(provider, shutdown)
                    .await;
            }
        }
    }
}

async fn sleep_or_stop(delay: Duration, shutdown: &CancellationToken) -> bool {
    tokio::select! {
        _ = shutdown.cancelled() => true,
        _ = tokio::time::sleep(delay) => false,
    }
}
