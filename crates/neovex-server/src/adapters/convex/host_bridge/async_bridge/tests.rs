use std::sync::Arc;

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
async fn async_host_call_records_precancel_metric() {
    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits::default()));
    let cancellation = HostCallCancellation::default();
    cancellation.cancel();

    let result = execute_async_host_call(
        RuntimeAsyncHostCallTrace::new(tracing::Span::none(), "convex runtime async host call"),
        policy.metrics(),
        convex_host_operation_name(HostCallOperation::DocumentGet),
        cancellation,
        async { Ok(json!("unexpected")) },
    )
    .await;

    assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
    let snapshot = policy.metrics_snapshot();
    assert_eq!(snapshot.canceled_host_ops, 1);
    assert_eq!(snapshot.precanceled_host_ops, 1);
    assert_eq!(snapshot.in_flight_canceled_host_ops, 0);
    assert_eq!(
        host_operation_metrics(
            &policy,
            convex_host_operation_name(HostCallOperation::DocumentGet)
        ),
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
async fn async_host_call_records_cooperative_read_cancellation() {
    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits::default()));
    let cancellation = HostCallCancellation::default();
    let started = Arc::new(Notify::new());

    let task = tokio::spawn({
        let started = started.clone();
        let metrics = policy.metrics();
        let cancellation = cancellation.clone();
        async move {
            execute_async_host_call(
                RuntimeAsyncHostCallTrace::new(
                    tracing::Span::none(),
                    "convex runtime async host call",
                ),
                metrics,
                convex_host_operation_name(HostCallOperation::DocumentGet),
                cancellation.clone(),
                async move {
                    started.notify_one();
                    cancellation.cancelled().await;
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
        host_operation_metrics(
            &policy,
            convex_host_operation_name(HostCallOperation::DocumentGet)
        ),
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
async fn async_host_call_waits_for_write_completion_after_cancellation() {
    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits::default()));
    let cancellation = HostCallCancellation::default();
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());

    let task = tokio::spawn({
        let started = started.clone();
        let release = release.clone();
        let metrics = policy.metrics();
        let cancellation = cancellation.clone();
        async move {
            execute_async_host_call(
                RuntimeAsyncHostCallTrace::new(
                    tracing::Span::none(),
                    "convex runtime async host call",
                ),
                metrics,
                convex_host_operation_name(HostCallOperation::DocumentInsert),
                cancellation,
                async move {
                    started.notify_one();
                    release.notified().await;
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
    assert!(
        cancellation.is_cancelled(),
        "cancellation signal should flip synchronously before the write is released"
    );
    release.notify_waiters();

    let result = timeout(Duration::from_secs(1), task)
        .await
        .expect("write host call should finish after release")
        .expect("write host call task should join")
        .expect("write host call should complete successfully");
    assert_eq!(result, json!("committed"));
    let snapshot = policy.metrics_snapshot();
    assert_eq!(snapshot.canceled_host_ops, 0);
    assert_eq!(
        host_operation_metrics(
            &policy,
            convex_host_operation_name(HostCallOperation::DocumentInsert)
        ),
        RuntimeHostOperationMetricsSnapshot {
            started: 1,
            succeeded: 1,
            failed: 0,
            canceled_before_start: 0,
            canceled_in_flight: 0,
        }
    );
}
