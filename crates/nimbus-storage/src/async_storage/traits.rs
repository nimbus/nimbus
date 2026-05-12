use std::future::Future;
use std::sync::Arc;

use nimbus_core::{Result, TenantId};

use crate::TenantWriteCommit;

/// Minimal async composition root for embedded persistence providers.
///
/// This trait is intentionally small: it covers tenant discovery plus the
/// async read/write executors rooted in `TenantStore` and
/// `TenantWriteTransaction`. The broader migration contract still lives on the
/// concrete store and transaction types because the engine depends on
/// query-planner, journal, snapshot, and batch-apply surfaces that are more
/// specific than CRUD.
pub trait EmbeddedPersistenceProvider {
    type TenantRead: TenantReadStorage;

    async fn list_tenants(&self) -> Result<Vec<TenantId>>;
}

/// Async read executor over the live tenant store surface.
///
/// Call sites rely on read tasks receiving `Arc<TenantStore>` so they can use
/// the existing snapshot, query-planner loading, journal, and materialized-read
/// helpers without translating them into a separate CRUD vocabulary.
pub trait TenantReadStorage: Send + Sync {
    type Store: Send + Sync + 'static;

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Self::Store>) -> Result<T> + Send + 'static;

    /// Executes a cancellable read task.
    ///
    /// Cancellation may short-circuit before the blocking work starts or while
    /// long scans periodically poll `check_cancel`. The engine depends on this
    /// to abort query, materialized-read, and subscription-bootstrap work
    /// without changing the underlying read semantics.
    async fn execute_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(Arc<Self::Store>, &mut dyn FnMut() -> Result<()>) -> Result<T> + Send + 'static;
}

/// Result of a cancellable write request at the current engine/storage seam.
///
/// The distinction is semantically important: once the durable commit point is
/// crossed the caller must observe a committed result even if transport-level
/// cancellation races with the response path.
pub enum TenantWriteOutcome<T> {
    CancelledBeforeCommit,
    Committed(TenantWriteCommit<T>),
}

/// Async write executor over `TenantWriteTransaction`.
///
/// Live call sites use this for schema updates, scheduler state transitions,
/// and other write helpers that must preserve the current transaction lifecycle
/// rather than flattening everything into document-only helpers.
pub trait TenantWriteStorage: Send + Sync {
    type WriteTransaction;

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut Self::WriteTransaction) -> Result<T> + Send + 'static;

    /// Executes a cancellable write task while preserving the current
    /// pre-commit versus committed-write split.
    async fn execute_write_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteOutcome<T>>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut Self::WriteTransaction) -> Result<T> + Send + 'static;
}

/// Async read executor for the global usage store.
pub trait UsageStorage: Send + Sync {
    type Store: Send + Sync + 'static;

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<Self::Store>) -> Result<T> + Send + 'static;
}
