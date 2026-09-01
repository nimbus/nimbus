#![cfg(all(feature = "bun-jsc-linked-adapter", nimbus_bun_jsc_shared_adapter))]

use std::collections::BTreeMap;
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::time::{Duration, Instant};

use nimbus_runtime::{
    HostBridge, HostCallOperation, HostCallRequest, InvocationKind, InvocationRequest,
    NimbusRuntime, NimbusRuntimeError, RuntimeBundle, RuntimeEgressPosture, RuntimeLimits,
    RuntimePolicy,
};
use serde_json::{Value, json};

#[derive(Debug)]
struct RendezvousState {
    arrivals: usize,
    released: bool,
    failed: bool,
}

#[derive(Debug)]
struct RendezvousHost {
    expected: usize,
    state: Mutex<RendezvousState>,
    changed: Condvar,
}

impl RendezvousHost {
    fn new(expected: usize) -> Self {
        Self {
            expected,
            state: Mutex::new(RendezvousState {
                arrivals: 0,
                released: false,
                failed: false,
            }),
            changed: Condvar::new(),
        }
    }
}

impl HostBridge for RendezvousHost {
    fn call(&self, request: HostCallRequest) -> nimbus_runtime::Result<Value> {
        if request.operation != HostCallOperation::DocumentInsert {
            return Err(NimbusRuntimeError::Contract(format!(
                "Bun/JSC concurrency proof received unexpected host operation {}",
                request.operation
            )));
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut state = self
            .state
            .lock()
            .expect("Bun/JSC concurrency proof lock should not poison");
        if state.failed {
            return Err(NimbusRuntimeError::Contract(
                "Bun/JSC invocations did not reach the host bridge concurrently".to_string(),
            ));
        }
        state.arrivals += 1;
        if state.arrivals == self.expected {
            state.released = true;
            self.changed.notify_all();
        }

        while !state.released {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.failed = true;
                state.released = true;
                self.changed.notify_all();
                return Err(NimbusRuntimeError::Contract(format!(
                    "only {} of {} Bun/JSC invocations reached the host bridge concurrently",
                    state.arrivals, self.expected
                )));
            }
            let (next, timeout) = self
                .changed
                .wait_timeout(state, remaining)
                .expect("Bun/JSC concurrency proof lock should not poison");
            state = next;
            if timeout.timed_out() && !state.released {
                state.failed = true;
                state.released = true;
                self.changed.notify_all();
                return Err(NimbusRuntimeError::Contract(format!(
                    "only {} of {} Bun/JSC invocations reached the host bridge concurrently",
                    state.arrivals, self.expected
                )));
            }
        }

        if state.failed {
            return Err(NimbusRuntimeError::Contract(
                "Bun/JSC invocations did not reach the host bridge concurrently".to_string(),
            ));
        }
        Ok(json!("host-rendezvous-complete"))
    }
}

#[test]
fn bun_shared_adapter_initializes_once_across_concurrent_executor_workers() {
    const INVOCATIONS: usize = 4;

    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let bundle_path = temp_dir.path().join("bun-concurrent-init-wrapper.js");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function(request) {
  await globalThis.__nimbusAsyncHostValue("op_nimbus_document_insert", {
    table: "messages",
    fields: { worker: request.args.worker },
  });
  return { status: "ok", value: request.args.worker };
};
"#,
    )
    .expect("Bun/JSC bundle should be written");

    let host = Arc::new(RendezvousHost::new(INVOCATIONS));
    let mut limits = RuntimeLimits::application_bun_jsc();
    limits.max_concurrent_runtime_instances = INVOCATIONS;
    limits.worker_threads = INVOCATIONS;
    let runtime = NimbusRuntime::with_policy(
        host,
        Arc::new(RuntimePolicy::new(limits)),
        RuntimeEgressPosture::CoarsePermissions,
    );
    let barrier = Arc::new(Barrier::new(INVOCATIONS));
    let workers = (0..INVOCATIONS)
        .map(|worker| {
            let barrier = barrier.clone();
            let bundle_path = bundle_path.clone();
            let runtime = runtime.clone();
            std::thread::spawn(move || {
                let request = InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:concurrentInit".to_string(),
                    args: json!({ "worker": worker }),
                    page_size: None,
                    cursor: None,
                    auth: None,
                    services: BTreeMap::new(),
                };

                barrier.wait();
                runtime.invoke_bundle_blocking(&RuntimeBundle::new(bundle_path), &request)
            })
        })
        .collect::<Vec<_>>();

    let outcomes = workers
        .into_iter()
        .map(|handle| {
            handle
                .join()
                .expect("Bun/JSC concurrent-init worker should not panic")
        })
        .collect::<Vec<_>>();
    for (worker, outcome) in outcomes.into_iter().enumerate() {
        let value = outcome.expect("Bun/JSC concurrent-init invocation should execute");
        assert_eq!(value, json!({ "status": "ok", "value": worker }));
    }
}
