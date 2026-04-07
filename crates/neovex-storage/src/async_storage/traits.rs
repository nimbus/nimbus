use std::future::Future;
use std::sync::Arc;

use neovex_core::{Result, TenantId};

use crate::{TenantStore, TenantWriteCommit, TenantWriteTransaction, UsageStore};

pub trait StorageEngine {
    type TenantRead: TenantReadStorage;
    type Usage: UsageStorage;

    async fn list_tenants(&self) -> Result<Vec<TenantId>>;
}

pub trait TenantReadStorage: Send + Sync {
    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<TenantStore>) -> Result<T> + Send + 'static;

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
        F: FnOnce(Arc<TenantStore>, &mut dyn FnMut() -> Result<()>) -> Result<T> + Send + 'static;
}

pub enum TenantWriteOutcome<T> {
    CancelledBeforeCommit,
    Committed(TenantWriteCommit<T>),
}

pub trait TenantWriteStorage: Send + Sync {
    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T> + Send + 'static;

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
        F: FnOnce(&mut TenantWriteTransaction) -> Result<T> + Send + 'static;
}

pub trait UsageStorage: Send + Sync {
    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<UsageStore>) -> Result<T> + Send + 'static;
}
