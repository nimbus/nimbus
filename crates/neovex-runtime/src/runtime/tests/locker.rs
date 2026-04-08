use super::*;
use crate::limits::{RuntimeExecutionModel, RuntimePoolKind};

fn locker_test_policy() -> Arc<RuntimePolicy> {
    Arc::new(RuntimePolicy::new(RuntimeLimits {
        execution_model: RuntimeExecutionModel::CooperativeLocker,
        runtime_pool_kind: RuntimePoolKind::StartupSnapshotCache,
        ..RuntimeLimits::default()
    }))
}

#[test]
fn runtime_builds_locker_jsruntime_from_snapshot() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let runtime_owner =
        NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), locker_test_policy());
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();
    let mut runtime = isolate_pool
        .take_runtime_with_options(&runtime_owner, &bundle, true)
        .expect("locker runtime should build from snapshot")
        .runtime;

    assert!(runtime.is_v8_lock_held());
    assert!(runtime.release_v8_lock());
    assert!(!runtime.is_v8_lock_held());

    let result = {
        let mut locked = runtime.acquire_v8_lock();
        let value = locked
            .execute_script(
                "locker_snapshot.js",
                "({ bootstrap: typeof globalThis.__neovexCreateContext, deno: typeof globalThis.Deno, sum: 1 + 1 })",
            )
            .expect("locker runtime should execute script");
        deserialize_json_value(&mut locked, value).expect("result should deserialize")
    };

    assert_eq!(
        result,
        serde_json::json!({
            "bootstrap": "function",
            "deno": "undefined",
            "sum": 2,
        })
    );
    assert!(!runtime.is_v8_lock_held());

    let metrics = runtime_owner.policy.metrics_snapshot();
    assert_eq!(metrics.isolate_pool_misses, 1);
    assert_eq!(metrics.isolate_pool_hits, 0);
}

#[test]
fn runtime_snapshot_backed_locker_runtimes_interleave_on_same_thread() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let runtime_owner =
        NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), locker_test_policy());
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();

    let mut rt1 = isolate_pool
        .take_runtime_with_options(&runtime_owner, &bundle, true)
        .expect("first locker runtime should build")
        .runtime;
    assert!(rt1.release_v8_lock());

    let mut rt2 = isolate_pool
        .take_runtime_with_options(&runtime_owner, &bundle, true)
        .expect("second locker runtime should build")
        .runtime;
    assert!(rt2.release_v8_lock());

    {
        let mut locked = rt1.acquire_v8_lock();
        locked
            .execute_script(
                "rt1_init.js",
                "if (typeof globalThis.__neovexCreateContext !== 'function') throw new Error('bootstrap missing'); globalThis.counter = 1;",
            )
            .expect("first runtime should initialize");
    }
    {
        let mut locked = rt2.acquire_v8_lock();
        locked
            .execute_script(
                "rt2_init.js",
                "if (typeof globalThis.__neovexCreateContext !== 'function') throw new Error('bootstrap missing'); globalThis.counter = 10;",
            )
            .expect("second runtime should initialize");
    }
    {
        let mut locked = rt1.acquire_v8_lock();
        locked
            .execute_script(
                "rt1_step.js",
                "if (globalThis.counter !== 1) throw new Error('rt1 lost state'); globalThis.counter += 1;",
            )
            .expect("first runtime should preserve state");
    }
    {
        let mut locked = rt2.acquire_v8_lock();
        locked
            .execute_script(
                "rt2_step.js",
                "if (globalThis.counter !== 10) throw new Error('rt2 lost state'); globalThis.counter += 5;",
            )
            .expect("second runtime should preserve state");
    }
    {
        let mut locked = rt1.acquire_v8_lock();
        locked
            .execute_script(
                "rt1_verify.js",
                "if (globalThis.counter !== 2) throw new Error('rt1 wrong final state');",
            )
            .expect("first runtime should keep final state");
    }
    {
        let mut locked = rt2.acquire_v8_lock();
        locked
            .execute_script(
                "rt2_verify.js",
                "if (globalThis.counter !== 15) throw new Error('rt2 wrong final state');",
            )
            .expect("second runtime should keep final state");
    }

    assert!(!rt1.is_v8_lock_held());
    assert!(!rt2.is_v8_lock_held());

    let metrics = runtime_owner.policy.metrics_snapshot();
    assert_eq!(metrics.isolate_pool_misses, 1);
    assert_eq!(metrics.isolate_pool_hits, 1);
}
