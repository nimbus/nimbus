use super::support::*;
use super::*;

const PIR3_WEB_STANDARD_SIDE_CHANNEL_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "pir3-web-standard-side-channel",
    "web_standard",
    "WebStandard user code sees coarsened timers, no SharedArrayBuffer, and disabled Atomics waits",
    "runtime::tests::basic_invocation::side_channel::pir3_web_standard_side_channel_surface_is_hardened_subprocess",
);

const PIR3_NODE_SIDE_CHANNEL_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "pir3-node-side-channel",
    "node20-node26",
    "Node user code sees coarsened timers, no SharedArrayBuffer, and disabled Atomics waits",
    "runtime::tests::basic_invocation::side_channel::pir3_node_targets_side_channel_surface_is_hardened_subprocess",
);

const PIR3_NODE_WORKER_SIDE_CHANNEL_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "pir3-node-worker-side-channel",
    "node22-worker_threads",
    "Node worker_threads user code sees coarsened timers, no SharedArrayBuffer, and disabled Atomics waits",
    "runtime::tests::basic_invocation::side_channel::pir3_node_worker_thread_side_channel_surface_is_hardened_subprocess",
);

const SIDE_CHANNEL_PROBE_BUNDLE: &str = r#"
function assertAtomicsWaitDisabled(name) {
  if (typeof Atomics?.[name] !== "function") {
    return { available: false, threw: null, name: null, message: null };
  }
  try {
    Atomics[name](new Int32Array(new ArrayBuffer(4)), 0, 0, 0);
    return { available: true, threw: false, name: null, message: null };
  } catch (error) {
    return {
      available: true,
      threw: true,
      name: error?.name ?? null,
      message: error?.message ?? String(error),
    };
  }
}

function timerSamples(fn) {
  return Array.from({ length: 8 }, () => fn());
}

function probeSideChannelSurface() {
  const wasmPlainMemory = (() => {
    try {
      const memory = new WebAssembly.Memory({ initial: 1 });
      return {
        created: true,
        bufferTag: Object.prototype.toString.call(memory.buffer),
        message: null,
      };
    } catch (error) {
      return {
        created: false,
        bufferTag: null,
        message: error?.message ?? String(error),
      };
    }
  })();
  const wasmSharedMemory = (() => {
    try {
      const memory = new WebAssembly.Memory({
        initial: 1,
        maximum: 1,
        shared: true,
      });
      return {
        created: true,
        bufferTag: Object.prototype.toString.call(memory.buffer),
        message: null,
      };
    } catch (error) {
      return {
        created: false,
        bufferTag: null,
        message: error?.message ?? String(error),
      };
    }
  })();
  return {
    sharedArrayBufferType: typeof globalThis.SharedArrayBuffer,
    wasmPlainMemory,
    wasmSharedMemory,
    atomicsWaitType: typeof Atomics?.wait,
    atomicsWait: assertAtomicsWaitDisabled("wait"),
    atomicsWaitAsyncType: typeof Atomics?.waitAsync,
    atomicsWaitAsync: assertAtomicsWaitDisabled("waitAsync"),
    dateNowType: typeof Date.now,
    dateNowSamples: timerSamples(() => Date.now()),
    performanceType: typeof globalThis.performance,
    performanceNowType: typeof globalThis.performance?.now,
    performanceNowSamples: timerSamples(() => globalThis.performance.now()),
  };
}

globalThis.__nimbusInvoke = function () {
  return probeSideChannelSurface();
};

export {};
"#;

const SIDE_CHANNEL_WORKER_PROBE_BUNDLE: &str = r#"
import { Worker } from "node:worker_threads";

globalThis.__nimbusInvoke = async function () {
  const workerSource = `
    const { parentPort } = require("node:worker_threads");

    function assertAtomicsWaitDisabled(name) {
      if (typeof Atomics?.[name] !== "function") {
        return { available: false, threw: null, name: null, message: null };
      }
      try {
        Atomics[name](new Int32Array(new ArrayBuffer(4)), 0, 0, 0);
        return { available: true, threw: false, name: null, message: null };
      } catch (error) {
        return {
          available: true,
          threw: true,
          name: error?.name ?? null,
          message: error?.message ?? String(error),
        };
      }
    }

    parentPort.postMessage({
      sharedArrayBufferType: typeof globalThis.SharedArrayBuffer,
      wasmPlainMemory: (() => {
        try {
          const memory = new WebAssembly.Memory({ initial: 1 });
          return {
            created: true,
            bufferTag: Object.prototype.toString.call(memory.buffer),
            message: null,
          };
        } catch (error) {
          return {
            created: false,
            bufferTag: null,
            message: error?.message ?? String(error),
          };
        }
      })(),
      wasmSharedMemory: (() => {
        try {
          const memory = new WebAssembly.Memory({
            initial: 1,
            maximum: 1,
            shared: true,
          });
          return {
            created: true,
            bufferTag: Object.prototype.toString.call(memory.buffer),
            message: null,
          };
        } catch (error) {
          return {
            created: false,
            bufferTag: null,
            message: error?.message ?? String(error),
          };
        }
      })(),
      atomicsWaitType: typeof Atomics?.wait,
      atomicsWait: assertAtomicsWaitDisabled("wait"),
      dateNowModulo: Date.now() % 10,
      performanceNowType: typeof globalThis.performance?.now,
      performanceNowModulo: globalThis.performance.now() % 10,
    });
  `;
  return await new Promise((resolve, reject) => {
    const worker = new Worker(workerSource, { eval: true });
    worker.once("message", resolve);
    worker.once("error", reject);
  });
};

export {};
"#;

fn assert_side_channel_surface(result: &Value) {
    assert_eq!(
        result["sharedArrayBufferType"],
        serde_json::json!("undefined")
    );
    assert_eq!(
        result["wasmPlainMemory"]["created"],
        serde_json::json!(true)
    );
    assert_eq!(
        result["wasmPlainMemory"]["bufferTag"],
        serde_json::json!("[object ArrayBuffer]")
    );
    assert_eq!(
        result["wasmSharedMemory"]["created"],
        serde_json::json!(false)
    );
    assert!(
        result["wasmSharedMemory"]["message"]
            .as_str()
            .expect("shared WebAssembly memory denial should serialize a message")
            .contains("Nimbus disables shared WebAssembly memory"),
        "unexpected shared WebAssembly memory result: {}",
        result["wasmSharedMemory"]
    );
    assert_eq!(result["atomicsWaitType"], serde_json::json!("function"));
    assert_eq!(result["atomicsWait"]["available"], serde_json::json!(true));
    assert_eq!(result["atomicsWait"]["threw"], serde_json::json!(true));
    assert_eq!(
        result["atomicsWait"]["name"],
        serde_json::json!("TypeError")
    );
    assert!(
        result["atomicsWait"]["message"]
            .as_str()
            .expect("Atomics.wait error should serialize a message")
            .contains("Nimbus disables Atomics.wait"),
        "unexpected Atomics.wait error: {}",
        result["atomicsWait"]
    );

    if result["atomicsWaitAsync"]["available"] == serde_json::json!(true) {
        assert_eq!(result["atomicsWaitAsync"]["threw"], serde_json::json!(true));
        assert!(
            result["atomicsWaitAsync"]["message"]
                .as_str()
                .expect("Atomics.waitAsync error should serialize a message")
                .contains("Nimbus disables Atomics.waitAsync"),
            "unexpected Atomics.waitAsync error: {}",
            result["atomicsWaitAsync"]
        );
    }

    assert_eq!(result["dateNowType"], serde_json::json!("function"));
    assert_eq!(result["performanceType"], serde_json::json!("object"));
    assert_eq!(result["performanceNowType"], serde_json::json!("function"));
    assert_timer_samples_are_coarsened(
        result["dateNowSamples"]
            .as_array()
            .expect("Date.now samples should serialize as an array"),
        "Date.now",
    );
    assert_timer_samples_are_coarsened(
        result["performanceNowSamples"]
            .as_array()
            .expect("performance.now samples should serialize as an array"),
        "performance.now",
    );
}

fn assert_timer_samples_are_coarsened(samples: &[Value], label: &str) {
    assert!(!samples.is_empty(), "{label} should produce samples");
    for sample in samples {
        let value = sample
            .as_f64()
            .unwrap_or_else(|| panic!("{label} sample should be numeric: {sample}"));
        let remainder = value.rem_euclid(10.0);
        assert!(
            remainder == 0.0,
            "{label} sample {value} should be coarsened to 10ms buckets"
        );
    }
}

async fn invoke_probe(limits: RuntimeLimits) -> Value {
    let (_tempdir, bundle_path) = write_app_style_bundle(SIDE_CHANNEL_PROBE_BUNDLE);
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(limits)),
    );
    runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "sideChannel:probe".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("side-channel probe bundle should execute")
}

#[test]
fn pir3_web_standard_side_channel_surface_is_hardened() {
    run_v8_sensitive_runtime_test_in_subprocess(PIR3_WEB_STANDARD_SIDE_CHANNEL_CASE);
}

#[test]
fn pir3_node_targets_side_channel_surface_is_hardened() {
    run_v8_sensitive_runtime_test_in_subprocess(PIR3_NODE_SIDE_CHANNEL_CASE);
}

#[test]
fn pir3_node_worker_thread_side_channel_surface_is_hardened() {
    run_v8_sensitive_runtime_test_in_subprocess(PIR3_NODE_WORKER_SIDE_CHANNEL_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate mixed-profile V8 snapshot external-reference state"]
fn pir3_web_standard_side_channel_surface_is_hardened_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(pir3_web_standard_side_channel_surface_is_hardened_inner());
}

#[test]
#[ignore = "runs in a subprocess to isolate mixed-profile V8 snapshot external-reference state"]
fn pir3_node_targets_side_channel_surface_is_hardened_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(pir3_node_targets_side_channel_surface_is_hardened_inner());
}

#[test]
#[ignore = "runs in a subprocess to isolate mixed-profile V8 snapshot external-reference state"]
fn pir3_node_worker_thread_side_channel_surface_is_hardened_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(pir3_node_worker_thread_side_channel_surface_is_hardened_inner());
}

async fn pir3_web_standard_side_channel_surface_is_hardened_inner() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let result = invoke_probe(RuntimeLimits::application_web_standard()).await;
    assert_side_channel_surface(&result);
}

async fn pir3_node_targets_side_channel_surface_is_hardened_inner() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    for limits in [
        RuntimeLimits::application_node20(),
        RuntimeLimits::application_node22(),
        RuntimeLimits::application_node24(),
        RuntimeLimits::application_node26(),
    ] {
        let result = invoke_probe(limits).await;
        assert_side_channel_surface(&result);
    }
}

async fn pir3_node_worker_thread_side_channel_surface_is_hardened_inner() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(SIDE_CHANNEL_WORKER_PROBE_BUNDLE);
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        Arc::new(RuntimePolicy::new(
            RuntimeLimits::application_node22_local_development(),
        )),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "sideChannel:workerProbe".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("side-channel worker probe bundle should execute");

    assert_eq!(
        result["sharedArrayBufferType"],
        serde_json::json!("undefined")
    );
    assert_eq!(
        result["wasmPlainMemory"]["created"],
        serde_json::json!(true)
    );
    assert_eq!(
        result["wasmPlainMemory"]["bufferTag"],
        serde_json::json!("[object ArrayBuffer]")
    );
    assert_eq!(
        result["wasmSharedMemory"]["created"],
        serde_json::json!(false)
    );
    assert!(
        result["wasmSharedMemory"]["message"]
            .as_str()
            .expect("worker shared WebAssembly memory denial should serialize a message")
            .contains("Nimbus disables shared WebAssembly memory"),
        "unexpected worker shared WebAssembly memory result: {}",
        result["wasmSharedMemory"]
    );
    assert_eq!(result["atomicsWaitType"], serde_json::json!("function"));
    assert_eq!(result["atomicsWait"]["available"], serde_json::json!(true));
    assert_eq!(result["atomicsWait"]["threw"], serde_json::json!(true));
    assert!(
        result["atomicsWait"]["message"]
            .as_str()
            .expect("worker Atomics.wait error should serialize a message")
            .contains("Nimbus disables Atomics.wait"),
        "unexpected worker Atomics.wait error: {}",
        result["atomicsWait"]
    );
    assert_eq!(result["dateNowModulo"], serde_json::json!(0));
    assert_eq!(result["performanceNowType"], serde_json::json!("function"));
    assert_eq!(result["performanceNowModulo"], serde_json::json!(0));
}
