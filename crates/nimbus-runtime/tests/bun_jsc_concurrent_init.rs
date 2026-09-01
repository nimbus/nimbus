#![cfg(all(feature = "bun-jsc-linked-adapter", nimbus_bun_jsc_shared_adapter))]

use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};

use nimbus_runtime::{
    HostBridge, HostCallRequest, InvocationKind, InvocationRequest, NimbusRuntime,
    NimbusRuntimeError, RuntimeBundle, RuntimeEgressPosture, RuntimeLimits, RuntimePolicy,
};
use serde_json::{Value, json};

#[derive(Debug)]
struct NoopHost;

impl HostBridge for NoopHost {
    fn call(&self, _request: HostCallRequest) -> nimbus_runtime::Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "Bun/JSC concurrent-init proof must not reach the host bridge".to_string(),
        ))
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
  return { status: "ok", value: request.args.worker };
};
"#,
    )
    .expect("Bun/JSC bundle should be written");

    let barrier = Arc::new(Barrier::new(INVOCATIONS));
    let workers = (0..INVOCATIONS)
        .map(|worker| {
            let barrier = barrier.clone();
            let bundle_path = bundle_path.clone();
            std::thread::spawn(move || {
                let mut limits = RuntimeLimits::application_bun_jsc();
                limits.max_concurrent_runtime_instances = 1;
                limits.worker_threads = 1;
                let runtime = NimbusRuntime::with_policy(
                    Arc::new(NoopHost),
                    Arc::new(RuntimePolicy::new(limits)),
                    RuntimeEgressPosture::CoarsePermissions,
                );
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

    for (worker, handle) in workers.into_iter().enumerate() {
        let value = handle
            .join()
            .expect("Bun/JSC concurrent-init worker should not panic")
            .expect("Bun/JSC concurrent-init invocation should execute");
        assert_eq!(value, json!({ "status": "ok", "value": worker }));
    }
}
