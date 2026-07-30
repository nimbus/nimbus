use std::sync::Arc;

use futures::future::BoxFuture;
// `boxed()` is called only from the provider-gated hook impls below.
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
use futures::future::FutureExt;
#[cfg(feature = "postgres")]
use nimbus_storage::PostgresProvider;
use tokio_util::sync::CancellationToken;

use crate::engine::Engine;
#[cfg(any(feature = "libsql", feature = "mysql"))]
use crate::engine::ProviderPollWorker;

pub(crate) struct WorkerContext {
    pub(crate) engine: Arc<Engine>,
    pub(crate) shutdown: CancellationToken,
}

pub(crate) trait RuntimeHooks: Send + Sync {
    fn task_name(&self) -> &'static str;

    fn backend_info(&self) -> Option<String> {
        None
    }

    fn spawn_workers(self: Box<Self>, ctx: WorkerContext) -> BoxFuture<'static, ()>;
}

/// Gated with its provider: the hooks exist only to drive that provider's
/// background worker, and `PersistenceProvider::runtime_hooks` returns `None`
/// for every provider not compiled in.
#[cfg(feature = "postgres")]
pub(crate) struct PostgresRuntimeHooks {
    provider: Arc<PostgresProvider>,
}

#[cfg(feature = "postgres")]
impl PostgresRuntimeHooks {
    pub(crate) fn new(provider: Arc<PostgresProvider>) -> Self {
        Self { provider }
    }
}

// RuntimeHooks for postgres LISTEN/NOTIFY catch-up.
#[cfg(feature = "postgres")]
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
            ctx.engine
                .run_provider_notification_listener(provider, ctx.shutdown)
                .await;
        }
        .boxed()
    }
}

#[cfg(feature = "mysql")]
pub(crate) struct MySqlRuntimeHooks;

// RuntimeHooks for mysql provider polling.
#[cfg(feature = "mysql")]
impl RuntimeHooks for MySqlRuntimeHooks {
    fn task_name(&self) -> &'static str {
        ProviderPollWorker::MySql.task_name()
    }

    fn backend_info(&self) -> Option<String> {
        Some("mysql".to_owned())
    }

    fn spawn_workers(self: Box<Self>, ctx: WorkerContext) -> BoxFuture<'static, ()> {
        async move {
            ctx.engine
                .run_provider_poll_worker(ProviderPollWorker::MySql, ctx.shutdown)
                .await;
        }
        .boxed()
    }
}

#[cfg(feature = "libsql")]
pub(crate) struct LibsqlReplicaRuntimeHooks;

// RuntimeHooks for libsql provider polling.
#[cfg(feature = "libsql")]
impl RuntimeHooks for LibsqlReplicaRuntimeHooks {
    fn task_name(&self) -> &'static str {
        ProviderPollWorker::LibsqlReplica.task_name()
    }

    fn backend_info(&self) -> Option<String> {
        Some("libsql".to_owned())
    }

    fn spawn_workers(self: Box<Self>, ctx: WorkerContext) -> BoxFuture<'static, ()> {
        async move {
            ctx.engine
                .run_provider_poll_worker(ProviderPollWorker::LibsqlReplica, ctx.shutdown)
                .await;
        }
        .boxed()
    }
}
