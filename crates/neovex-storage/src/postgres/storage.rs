use super::*;

// Network-backed tenant stores keep provider-owned blocking write executors for
// now because transaction/session lifecycles are coupled to the Postgres async
// client. The generic `async_storage::write` executor is only shared across the
// embedded blocking-store seam in this wave.
impl PostgresTenantStorage {
    pub fn new(store: Arc<PostgresTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self::with_max_concurrent_reads(store, runtime_handle, default_postgres_read_parallelism())
    }

    pub fn with_max_concurrent_reads(
        store: Arc<PostgresTenantStore>,
        runtime_handle: TokioRuntimeHandle,
        max_concurrent_reads: usize,
    ) -> Self {
        Self {
            store: store.clone(),
            permits: Arc::new(Semaphore::new(max_concurrent_reads.max(1))),
            runtime_handle: runtime_handle.clone(),
            write_executor: PostgresBlockingWriteExecutor::new(store, runtime_handle),
        }
    }

    pub fn store(&self) -> Arc<PostgresTenantStore> {
        self.store.clone()
    }
}

impl TenantReadStorage for PostgresTenantStorage {
    type Store = PostgresTenantStore;

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<PostgresTenantStore>) -> Result<T> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(map_permit_error)?;
        let store = self.store.clone();
        self.runtime_handle
            .spawn_blocking(move || {
                let _permit = permit;
                task(store)
            })
            .await
            .map_err(map_join_error)?
    }

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
        F: FnOnce(Arc<PostgresTenantStore>, &mut dyn FnMut() -> Result<()>) -> Result<T>
            + Send
            + 'static,
    {
        tokio::pin!(cancel_wait);

        let permit = tokio::select! {
            _ = &mut cancel_wait => return Err(Error::Cancelled),
            permit = self.permits.clone().acquire_owned() => permit.map_err(map_permit_error)?,
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let store = self.store.clone();
        let cancelled_for_task = cancelled.clone();
        let mut handle = self.runtime_handle.spawn_blocking(move || {
            let _permit = permit;
            let mut combined_cancel = || {
                if cancelled_for_task.load(AtomicOrdering::SeqCst) {
                    return Err(Error::Cancelled);
                }
                check_cancel()
            };
            task(store, &mut combined_cancel)
        });

        tokio::select! {
            _ = &mut cancel_wait => {
                cancelled.store(true, AtomicOrdering::SeqCst);
                handle.abort();
                Err(Error::Cancelled)
            }
            result = &mut handle => result.map_err(map_join_error)?,
        }
    }
}

impl TenantWriteStorage for PostgresTenantStorage {
    type WriteTransaction = PostgresWriteTransaction;

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor.execute_write(task).await
    }

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
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor
            .execute_write_cancellable(cancel_wait, check_cancel, task)
            .await
    }
}

impl PostgresBlockingWriteExecutor {
    fn new(store: Arc<PostgresTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self {
            store,
            permits: Arc::new(Semaphore::new(POSTGRES_TENANT_WRITE_PARALLELISM)),
            runtime_handle,
        }
    }

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(map_permit_error)?;
        let store = self.store.clone();
        self.runtime_handle
            .spawn_blocking(move || {
                let _permit = permit;
                store.execute_write(task)
            })
            .await
            .map_err(map_join_error)?
    }

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
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        tokio::pin!(cancel_wait);

        let permit = tokio::select! {
            _ = &mut cancel_wait => return Ok(TenantWriteOutcome::CancelledBeforeCommit),
            permit = self.permits.clone().acquire_owned() => permit.map_err(map_permit_error)?,
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let store = self.store.clone();
        let cancelled_for_task = cancelled.clone();
        let mut handle = self.runtime_handle.spawn_blocking(move || {
            let _permit = permit;
            store.execute_write_cancellable(
                move || {
                    if cancelled_for_task.load(AtomicOrdering::SeqCst) {
                        return Err(Error::Cancelled);
                    }
                    check_cancel()
                },
                task,
            )
        });

        tokio::select! {
            result = &mut handle => map_write_result(result.map_err(map_join_error)?),
            _ = &mut cancel_wait => {
                cancelled.store(true, AtomicOrdering::SeqCst);
                map_write_result(handle.await.map_err(map_join_error)?)
            }
        }
    }
}

fn map_write_result<T>(result: Result<TenantWriteCommit<T>>) -> Result<TenantWriteOutcome<T>> {
    match result {
        Ok(committed) => Ok(TenantWriteOutcome::Committed(committed)),
        Err(Error::Cancelled) => Ok(TenantWriteOutcome::CancelledBeforeCommit),
        Err(error) => Err(error),
    }
}
