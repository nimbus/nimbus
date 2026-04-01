use super::*;

fn record_host_operation_result(
    metrics: &neovex_runtime::RuntimeMetrics,
    operation: &str,
    result: &std::result::Result<Value, NeovexRuntimeError>,
) {
    match result {
        Ok(_) => metrics.record_host_operation_succeeded(operation),
        Err(NeovexRuntimeError::Cancelled) => {
            metrics.record_host_operation_canceled_in_flight(operation);
        }
        Err(_) => metrics.record_host_operation_failed(operation),
    }
}

#[derive(Clone)]
struct AsyncHostCallTrace {
    span: tracing::Span,
    enqueued_at: std::time::Instant,
}

impl AsyncHostCallTrace {
    fn new(bridge: &ConvexRuntimeBridge, operation: &str) -> Self {
        static NEXT_ASYNC_HOST_CALL_ID: AtomicU64 = AtomicU64::new(1);

        let span = tracing::debug_span!(
            "convex_runtime_async_host_call",
            tenant = %bridge.tenant_id,
            server_request_id = ?bridge.server_request_id,
            session_id = %bridge.session_id,
            operation,
            host_call_id = NEXT_ASYNC_HOST_CALL_ID.fetch_add(1, Ordering::Relaxed),
        );
        let trace = Self {
            span,
            enqueued_at: std::time::Instant::now(),
        };
        tracing::debug!(
            parent: &trace.span,
            "convex runtime async host call enqueued"
        );
        trace
    }

    fn record_canceled_before_start(
        &self,
        cause: Option<neovex_runtime::HostCallCancellationCause>,
    ) {
        match cause {
            Some(cause) => tracing::debug!(
                parent: &self.span,
                queue_wait_ms = self.enqueued_at.elapsed().as_secs_f64() * 1000.0,
                cancellation_cause = cause.as_str(),
                "convex runtime async host call canceled before start"
            ),
            None => tracing::debug!(
                parent: &self.span,
                queue_wait_ms = self.enqueued_at.elapsed().as_secs_f64() * 1000.0,
                "convex runtime async host call canceled before start"
            ),
        }
    }

    fn record_started(&self) -> std::time::Instant {
        let started_at = std::time::Instant::now();
        tracing::debug!(
            parent: &self.span,
            queue_wait_ms = started_at.duration_since(self.enqueued_at).as_secs_f64() * 1000.0,
            "convex runtime async host call started"
        );
        started_at
    }

    fn record_finished(
        &self,
        started_at: std::time::Instant,
        result: &std::result::Result<Value, NeovexRuntimeError>,
        cancellation_cause: Option<neovex_runtime::HostCallCancellationCause>,
    ) {
        let execution_ms = started_at.elapsed().as_secs_f64() * 1000.0;
        match result {
            Ok(_) => tracing::debug!(
                parent: &self.span,
                execution_ms,
                "convex runtime async host call finished"
            ),
            Err(NeovexRuntimeError::Cancelled) => match cancellation_cause {
                Some(cause) => tracing::debug!(
                    parent: &self.span,
                    execution_ms,
                    cancellation_cause = cause.as_str(),
                    "convex runtime async host call canceled in flight"
                ),
                None => tracing::debug!(
                    parent: &self.span,
                    execution_ms,
                    "convex runtime async host call canceled in flight"
                ),
            },
            Err(error) => tracing::debug!(
                parent: &self.span,
                execution_ms,
                error = %error,
                "convex runtime async host call failed"
            ),
        }
    }

    fn record_join_failure(&self, error: &tokio::task::JoinError) {
        tracing::debug!(
            parent: &self.span,
            error = %error,
            "convex runtime async host call failed before completion"
        );
    }
}

async fn execute_async_blocking_host_call<F>(
    trace: AsyncHostCallTrace,
    metrics: Arc<neovex_runtime::RuntimeMetrics>,
    operation: String,
    cancellation: HostCallCancellation,
    task: F,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    F: FnOnce(HostCallCancellation) -> std::result::Result<Value, NeovexRuntimeError>
        + Send
        + 'static,
{
    let cancellation_cause = cancellation.cause();
    if cancellation.is_cancelled() {
        metrics.record_host_operation_canceled_before_start(&operation);
        trace.record_canceled_before_start(cancellation_cause);
        return Err(NeovexRuntimeError::Cancelled);
    }

    let metrics_for_task = metrics.clone();
    let operation_for_task = operation.clone();
    let trace_for_task = trace.clone();
    let handle = tokio::task::spawn_blocking(move || {
        let started_at = trace_for_task.record_started();
        metrics_for_task.record_host_operation_started(&operation_for_task);
        let result = task(cancellation);
        (started_at, result)
    });
    let (started_at, result) = match handle.await {
        Ok(output) => output,
        Err(error) => {
            trace.record_join_failure(&error);
            metrics.record_host_operation_failed(&operation);
            return Err(NeovexRuntimeError::Contract(format!(
                "runtime host bridge task failed: {error}"
            )));
        }
    };
    trace.record_finished(started_at, &result, cancellation_cause);
    record_host_operation_result(&metrics, &operation, &result);
    result
}

impl HostBridge for ConvexRuntimeBridge {
    fn call(&self, request: HostCallRequest) -> std::result::Result<Value, NeovexRuntimeError> {
        let metrics = self.registry.runtime_policy().metrics();
        let operation = request.operation.clone();
        metrics.record_host_operation_started(&operation);
        let result = self.dispatch_host_call(request);
        record_host_operation_result(&metrics, &operation, &result);
        result
    }

    fn call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let metrics = self.registry.runtime_policy().metrics();
        let operation = request.operation.clone();
        if cancellation.is_cancelled() {
            metrics.record_host_operation_canceled_before_start(&operation);
            return Err(NeovexRuntimeError::Cancelled);
        }
        metrics.record_host_operation_started(&operation);
        let result = self.dispatch_host_call_cancellable(request, cancellation);
        record_host_operation_result(&metrics, &operation, &result);
        result
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let bridge = self.clone();
        let trace = AsyncHostCallTrace::new(&bridge, &request.operation);
        let metrics = bridge.registry.runtime_policy().metrics();
        let operation = request.operation.clone();
        Box::pin(execute_async_blocking_host_call(
            trace,
            metrics,
            operation,
            cancellation,
            move |cancellation| bridge.dispatch_host_call_cancellable(request, &cancellation),
        ))
    }
}

impl ConvexRuntimeBridge {
    pub(in crate::adapters::convex) fn dispatch_host_call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match request.operation.as_str() {
            "convex.ctx.db.query.start" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_start(request.payload)
            }
            "convex.ctx.db.query.with_index" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_with_index(request.payload)
            }
            "convex.ctx.db.query.filter" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_filter(request.payload)
            }
            "convex.ctx.db.query.order" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_query_order(request.payload)
            }
            "convex.http_route" => {
                self.invoke_http_route_cancellable(request.payload, cancellation)
            }
            "convex.ctx.query" => self.invoke_ctx_query_cancellable(request.payload, cancellation),
            "convex.ctx.paginated_query" => {
                self.invoke_ctx_paginated_query_cancellable(request.payload, cancellation)
            }
            "convex.ctx.mutation" => {
                self.invoke_ctx_mutation_cancellable(request.payload, cancellation)
            }
            "convex.ctx.action" => {
                self.invoke_ctx_action_cancellable(request.payload, cancellation)
            }
            "convex.ctx.run_query" => {
                self.invoke_ctx_run_query_cancellable(request.payload, cancellation)
            }
            "convex.ctx.run_mutation" => {
                self.invoke_ctx_run_mutation_cancellable(request.payload, cancellation)
            }
            "convex.ctx.run_action" => {
                self.invoke_ctx_run_action_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.get" => {
                self.invoke_ctx_db_get_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.insert" => {
                self.invoke_ctx_db_insert_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.patch" => {
                self.invoke_ctx_db_patch_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.delete" => {
                self.invoke_ctx_db_delete_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.collect" => {
                self.invoke_ctx_query_collect_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.take" => {
                self.invoke_ctx_query_take_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.paginate" => {
                self.invoke_ctx_query_paginate_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.first" => {
                self.invoke_ctx_query_first_cancellable(request.payload, cancellation)
            }
            "convex.ctx.db.query.unique" => {
                self.invoke_ctx_query_unique_cancellable(request.payload, cancellation)
            }
            "convex.ctx.scheduler.run_after" => {
                self.invoke_ctx_scheduler_run_after_cancellable(request.payload, cancellation)
            }
            "convex.ctx.scheduler.run_at" => {
                self.invoke_ctx_scheduler_run_at_cancellable(request.payload, cancellation)
            }
            "convex.ctx.scheduler.cancel" => {
                self.invoke_ctx_scheduler_cancel_cancellable(request.payload, cancellation)
            }
            "convex.ctx.runtime.enter_nested_call" => {
                ensure_runtime_host_not_cancelled(cancellation)?;
                self.invoke_ctx_runtime_enter_nested_call(request.payload)
            }
            other => Err(NeovexRuntimeError::Contract(format!(
                "unsupported convex runtime operation: {other}"
            ))),
        }
    }

    pub(in crate::adapters::convex) fn dispatch_host_call(
        &self,
        request: HostCallRequest,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        match request.operation.as_str() {
            "convex.http_route" => self.invoke_http_route(request.payload),
            "convex.ctx.query" => self.invoke_ctx_query(request.payload),
            "convex.ctx.paginated_query" => self.invoke_ctx_paginated_query(request.payload),
            "convex.ctx.mutation" => self.invoke_ctx_mutation(request.payload),
            "convex.ctx.action" => self.invoke_ctx_action(request.payload),
            "convex.ctx.runtime.enter_nested_call" => {
                self.invoke_ctx_runtime_enter_nested_call(request.payload)
            }
            "convex.ctx.run_query" => self.invoke_ctx_run_query(request.payload),
            "convex.ctx.run_mutation" => self.invoke_ctx_run_mutation(request.payload),
            "convex.ctx.run_action" => self.invoke_ctx_run_action(request.payload),
            "convex.ctx.db.get" => self.invoke_ctx_db_get(request.payload),
            "convex.ctx.db.query.start" => self.invoke_ctx_query_start(request.payload),
            "convex.ctx.db.query.with_index" => self.invoke_ctx_query_with_index(request.payload),
            "convex.ctx.db.query.filter" => self.invoke_ctx_query_filter(request.payload),
            "convex.ctx.db.query.order" => self.invoke_ctx_query_order(request.payload),
            "convex.ctx.db.query.collect" => self.invoke_ctx_query_collect(request.payload),
            "convex.ctx.db.query.take" => self.invoke_ctx_query_take(request.payload),
            "convex.ctx.db.query.paginate" => self.invoke_ctx_query_paginate(request.payload),
            "convex.ctx.db.query.first" => self.invoke_ctx_query_first(request.payload),
            "convex.ctx.db.query.unique" => self.invoke_ctx_query_unique(request.payload),
            "convex.ctx.db.insert" => self.invoke_ctx_db_insert(request.payload),
            "convex.ctx.db.patch" => self.invoke_ctx_db_patch(request.payload),
            "convex.ctx.db.delete" => self.invoke_ctx_db_delete(request.payload),
            "convex.ctx.scheduler.run_after" => {
                self.invoke_ctx_scheduler_run_after(request.payload)
            }
            "convex.ctx.scheduler.run_at" => self.invoke_ctx_scheduler_run_at(request.payload),
            "convex.ctx.scheduler.cancel" => self.invoke_ctx_scheduler_cancel(request.payload),
            other => Err(NeovexRuntimeError::Contract(format!(
                "unsupported convex runtime operation: {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};

    use neovex_runtime::{RuntimeHostOperationMetricsSnapshot, RuntimeLimits, RuntimePolicy};
    use serde_json::json;
    use tokio::sync::Notify;
    use tokio::time::{Duration, timeout};

    use super::*;

    fn host_operation_metrics(
        policy: &RuntimePolicy,
        operation: &str,
    ) -> RuntimeHostOperationMetricsSnapshot {
        policy
            .metrics_snapshot()
            .host_operations
            .get(operation)
            .copied()
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn async_blocking_host_call_records_precancel_metric() {
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits::default()));
        let cancellation = HostCallCancellation::default();
        cancellation.cancel();

        let result = execute_async_blocking_host_call(
            AsyncHostCallTrace {
                span: tracing::Span::none(),
                enqueued_at: std::time::Instant::now(),
            },
            policy.metrics(),
            "convex.ctx.db.get".to_string(),
            cancellation,
            |_cancellation| Ok(json!("unexpected")),
        )
        .await;

        assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
        let snapshot = policy.metrics_snapshot();
        assert_eq!(snapshot.canceled_host_ops, 1);
        assert_eq!(snapshot.precanceled_host_ops, 1);
        assert_eq!(snapshot.in_flight_canceled_host_ops, 0);
        assert_eq!(
            host_operation_metrics(&policy, "convex.ctx.db.get"),
            RuntimeHostOperationMetricsSnapshot {
                started: 0,
                succeeded: 0,
                failed: 0,
                canceled_before_start: 1,
                canceled_in_flight: 0,
            }
        );
    }

    #[tokio::test]
    async fn async_blocking_host_call_records_cooperative_read_cancellation() {
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits::default()));
        let cancellation = HostCallCancellation::default();
        let started = Arc::new(Notify::new());

        let task = tokio::spawn({
            let started = started.clone();
            let metrics = policy.metrics();
            let cancellation = cancellation.clone();
            async move {
                execute_async_blocking_host_call(
                    AsyncHostCallTrace {
                        span: tracing::Span::none(),
                        enqueued_at: std::time::Instant::now(),
                    },
                    metrics,
                    "convex.ctx.db.get".to_string(),
                    cancellation,
                    move |cancellation| {
                        started.notify_one();
                        while !cancellation.is_cancelled() {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(NeovexRuntimeError::Cancelled)
                    },
                )
                .await
            }
        });

        timeout(Duration::from_secs(1), started.notified())
            .await
            .expect("blocking host call should start");
        cancellation.cancel();

        let result = timeout(Duration::from_secs(1), task)
            .await
            .expect("canceled host call should resolve promptly")
            .expect("blocking host call task should join");
        assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
        let snapshot = policy.metrics_snapshot();
        assert_eq!(snapshot.canceled_host_ops, 1);
        assert_eq!(snapshot.precanceled_host_ops, 0);
        assert_eq!(snapshot.in_flight_canceled_host_ops, 1);
        assert_eq!(
            host_operation_metrics(&policy, "convex.ctx.db.get"),
            RuntimeHostOperationMetricsSnapshot {
                started: 1,
                succeeded: 0,
                failed: 0,
                canceled_before_start: 0,
                canceled_in_flight: 1,
            }
        );
    }

    #[tokio::test]
    async fn async_blocking_host_call_waits_for_write_completion_after_cancellation() {
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits::default()));
        let cancellation = HostCallCancellation::default();
        let started = Arc::new(Notify::new());
        let release = Arc::new((Mutex::new(false), Condvar::new()));

        let task = tokio::spawn({
            let started = started.clone();
            let release = release.clone();
            let metrics = policy.metrics();
            let cancellation = cancellation.clone();
            async move {
                execute_async_blocking_host_call(
                    AsyncHostCallTrace {
                        span: tracing::Span::none(),
                        enqueued_at: std::time::Instant::now(),
                    },
                    metrics,
                    "convex.ctx.db.insert".to_string(),
                    cancellation,
                    move |_cancellation| {
                        started.notify_one();
                        let (lock, cvar) = &*release;
                        let mut released = lock
                            .lock()
                            .expect("write completion lock should not be poisoned");
                        while !*released {
                            released = cvar
                                .wait(released)
                                .expect("write completion wait should not be poisoned");
                        }
                        Ok(json!("committed"))
                    },
                )
                .await
            }
        });

        timeout(Duration::from_secs(1), started.notified())
            .await
            .expect("blocking write should start");
        cancellation.cancel();
        tokio::time::sleep(Duration::from_millis(25)).await;

        {
            let (lock, cvar) = &*release;
            let mut released = lock
                .lock()
                .expect("write completion lock should not be poisoned");
            *released = true;
            cvar.notify_all();
        }

        let result = timeout(Duration::from_secs(1), task)
            .await
            .expect("write host call should finish after release")
            .expect("write host call task should join")
            .expect("write host call should complete successfully");
        assert_eq!(result, json!("committed"));
        let snapshot = policy.metrics_snapshot();
        assert_eq!(snapshot.canceled_host_ops, 0);
        assert_eq!(
            host_operation_metrics(&policy, "convex.ctx.db.insert"),
            RuntimeHostOperationMetricsSnapshot {
                started: 1,
                succeeded: 1,
                failed: 0,
                canceled_before_start: 0,
                canceled_in_flight: 0,
            }
        );
    }
}
