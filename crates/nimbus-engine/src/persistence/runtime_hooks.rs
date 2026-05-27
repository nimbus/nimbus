use std::sync::Arc;

use futures::future::{BoxFuture, FutureExt};
use nimbus_storage::PostgresProvider;
use tokio_util::sync::CancellationToken;

use crate::service::{ProviderPollWorker, Service};

pub(crate) struct WorkerContext {
    pub(crate) service: Arc<Service>,
    pub(crate) shutdown: CancellationToken,
}

pub(crate) trait RuntimeHooks: Send + Sync {
    fn task_name(&self) -> &'static str;

    fn backend_info(&self) -> Option<String> {
        None
    }

    fn spawn_workers(self: Box<Self>, ctx: WorkerContext) -> BoxFuture<'static, ()>;
}

pub(crate) struct PostgresRuntimeHooks {
    provider: Arc<PostgresProvider>,
}

impl PostgresRuntimeHooks {
    pub(crate) fn new(provider: Arc<PostgresProvider>) -> Self {
        Self { provider }
    }
}

// RuntimeHooks for postgres LISTEN/NOTIFY catch-up.
impl RuntimeHooks for PostgresRuntimeHooks {
    fn task_name(&self) -> &'static str {
        "postgres_provider_hints"
    }

    fn backend_info(&self) -> Option<String> {
        Some("postgres".to_owned())
    }

    fn spawn_workers(self: Box<Self>, ctx: WorkerContext) -> BoxFuture<'static, ()> {
        let provider = self.provider;
        async move {
            ctx.service
                .run_provider_notification_listener(provider, ctx.shutdown)
                .await;
        }
        .boxed()
    }
}

pub(crate) struct MySqlRuntimeHooks;

// RuntimeHooks for mysql provider polling.
impl RuntimeHooks for MySqlRuntimeHooks {
    fn task_name(&self) -> &'static str {
        ProviderPollWorker::MySql.task_name()
    }

    fn backend_info(&self) -> Option<String> {
        Some("mysql".to_owned())
    }

    fn spawn_workers(self: Box<Self>, ctx: WorkerContext) -> BoxFuture<'static, ()> {
        async move {
            ctx.service
                .run_provider_poll_worker(ProviderPollWorker::MySql, ctx.shutdown)
                .await;
        }
        .boxed()
    }
}

pub(crate) struct LibsqlReplicaRuntimeHooks;

// RuntimeHooks for libsql provider polling.
impl RuntimeHooks for LibsqlReplicaRuntimeHooks {
    fn task_name(&self) -> &'static str {
        ProviderPollWorker::LibsqlReplica.task_name()
    }

    fn backend_info(&self) -> Option<String> {
        Some("libsql".to_owned())
    }

    fn spawn_workers(self: Box<Self>, ctx: WorkerContext) -> BoxFuture<'static, ()> {
        async move {
            ctx.service
                .run_provider_poll_worker(ProviderPollWorker::LibsqlReplica, ctx.shutdown)
                .await;
        }
        .boxed()
    }
}
