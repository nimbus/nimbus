use super::*;

#[tokio::test]
async fn runtime_cooperative_locker_slot_parks_and_resumes_after_async_host_completion() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const host = await ctx.db.get("messages", "doc-1");
  return {
    ok: true,
    host,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };
    let host = Arc::new(DeferredAsyncHost::default());
    let runtime_owner = NeovexRuntime::new(host.clone());
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();
    let watchdog = WatchdogTimer::new();
    let activity_signal = Arc::new(crate::executor::WorkerActivitySignal::new());
    let mut permit = SharedInvocationPermit::new(runtime_owner.policy(), None, None, false, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");

    let mut slot = runtime_owner
        .start_cooperative_locker_runtime_slot(
            &mut isolate_pool,
            CooperativeRuntimeSlotStart {
                invocation: RuntimeInvocationExecution {
                    watchdog: watchdog.clone(),
                    bundle: bundle.clone(),
                    request: request.clone(),
                    context: RuntimeInvocationContext::top_level(&request),
                    external_cancellation: None,
                    permit: permit.clone(),
                },
                activity_signal: activity_signal.clone(),
            },
        )
        .await
        .expect("cooperative locker slot should start");

    assert!(!slot.is_ready_to_resume());
    let mut parked = false;
    for poll_index in 0..8 {
        match slot.poll_once().await.expect("slot poll should succeed") {
            CooperativeRuntimeSlotPoll::Runnable => continue,
            CooperativeRuntimeSlotPoll::Parked => {
                parked = true;
                break;
            }
            CooperativeRuntimeSlotPoll::Completed => {
                panic!(
                    "deferred async host work should not complete before release (poll {poll_index})"
                );
            }
        }
    }
    assert!(parked, "deferred async host work should eventually park");
    assert_eq!(runtime_owner.policy.metrics_snapshot().active_isolates, 0);

    let initial_generation = activity_signal.current_generation();
    host.release();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if slot.is_ready_to_resume()
                || activity_signal.current_generation() > initial_generation
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("host completion should wake the cooperative slot");
    assert!(slot.is_ready_to_resume());
    let mut completed = false;
    for poll_index in 0..8 {
        match slot
            .poll_once()
            .await
            .expect("slot poll should succeed after wake")
        {
            CooperativeRuntimeSlotPoll::Runnable => continue,
            CooperativeRuntimeSlotPoll::Completed => {
                completed = true;
                break;
            }
            CooperativeRuntimeSlotPoll::Parked => {
                panic!(
                    "released async host work should not park again before completion (poll {poll_index})"
                );
            }
        }
    }
    assert!(
        completed,
        "released async host work should complete after wake"
    );

    let result = slot
        .take_result()
        .expect("slot should keep completed value");
    assert_eq!(
        result,
        serde_json::json!({
            "ok": true,
            "host": {
                "operation": "convex.ctx.db.get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "query:messages:list",
                }
            }
        })
    );

    let ready_jobs = permit.finish_invocation().await;
    assert!(ready_jobs.is_empty());
    assert_eq!(runtime_owner.policy.metrics_snapshot().active_isolates, 0);
    watchdog.shutdown();
}

#[tokio::test]
async fn runtime_cooperative_locker_slot_completes_immediate_async_host_work_without_parking() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };
    let runtime_owner = NeovexRuntime::new(Arc::new(AsyncEchoHost));
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();
    let watchdog = WatchdogTimer::new();
    let activity_signal = Arc::new(crate::executor::WorkerActivitySignal::new());
    let mut permit = SharedInvocationPermit::new(runtime_owner.policy(), None, None, false, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");

    let mut slot = runtime_owner
        .start_cooperative_locker_runtime_slot(
            &mut isolate_pool,
            CooperativeRuntimeSlotStart {
                invocation: RuntimeInvocationExecution {
                    watchdog: watchdog.clone(),
                    bundle: bundle.clone(),
                    request: request.clone(),
                    context: RuntimeInvocationContext::top_level(&request),
                    external_cancellation: None,
                    permit,
                },
                activity_signal,
            },
        )
        .await
        .expect("cooperative locker slot should start");

    let mut sequence = Vec::new();
    for _ in 0..20 {
        let poll = slot.poll_once().await.expect("slot poll should succeed");
        sequence.push(poll);
        if poll == CooperativeRuntimeSlotPoll::Completed {
            let result = slot
                .take_result()
                .expect("completed slot should retain its result");
            assert_eq!(
                result,
                serde_json::json!({
                    "operation": "convex.ctx.db.get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "query:messages:list",
                    },
                })
            );
            watchdog.shutdown();
            return;
        }
    }

    panic!(
        "cooperative locker slot should complete within a bounded number of polls; sequence={sequence:?}"
    );
}
