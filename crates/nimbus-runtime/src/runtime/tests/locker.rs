use super::*;
use crate::backends::v8::V8WorkerRuntimePool;

fn locker_test_policy() -> Arc<RuntimePolicy> {
    cooperative_startup_snapshot_runtime_test_policy()
}

pub(super) const LOCKER_SNAPSHOT_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "runtime-locker-builds-from-snapshot",
    "cooperative-startup-snapshot",
    "locker runtime builds from startup snapshot and exposes bootstrap globals only under lock",
    "runtime::tests::locker::runtime_builds_locker_jsruntime_from_snapshot_subprocess",
);

pub(super) const LOCKER_INTERLEAVE_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "runtime-locker-interleave-same-thread",
    "cooperative-startup-snapshot",
    "snapshot-backed locker runtimes preserve isolated state while interleaving on one thread",
    "runtime::tests::locker::runtime_snapshot_backed_locker_runtimes_interleave_on_same_thread_subprocess",
);

#[test]
fn runtime_builds_locker_jsruntime_from_snapshot() {
    run_v8_sensitive_runtime_test_in_subprocess(LOCKER_SNAPSHOT_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate locker V8 state"]
fn runtime_builds_locker_jsruntime_from_snapshot_subprocess() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let runtime_owner =
        NimbusRuntime::with_policy(Arc::new(RecordingHost::default()), locker_test_policy());
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let mut runtime = v8_runtime_pool
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
                "({ bootstrap: typeof globalThis.__nimbusCreateContext, deno: typeof globalThis.Deno, sum: 1 + 1 })",
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
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 0);
}

#[test]
fn runtime_snapshot_backed_locker_runtimes_interleave_on_same_thread() {
    run_v8_sensitive_runtime_test_in_subprocess(LOCKER_INTERLEAVE_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate locker V8 state"]
fn runtime_snapshot_backed_locker_runtimes_interleave_on_same_thread_subprocess() {
    let _guard = acquire_runtime_suite_lock();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let runtime_owner =
        NimbusRuntime::with_policy(Arc::new(RecordingHost::default()), locker_test_policy());
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();

    let mut rt1 = v8_runtime_pool
        .take_runtime_with_options(&runtime_owner, &bundle, true)
        .expect("first locker runtime should build")
        .runtime;
    assert!(rt1.release_v8_lock());

    let mut rt2 = v8_runtime_pool
        .take_runtime_with_options(&runtime_owner, &bundle, true)
        .expect("second locker runtime should build")
        .runtime;
    assert!(rt2.release_v8_lock());

    {
        let mut locked = rt1.acquire_v8_lock();
        locked
            .execute_script(
                "rt1_init.js",
                "if (typeof globalThis.__nimbusCreateContext !== 'function') throw new Error('bootstrap missing'); globalThis.counter = 1;",
            )
            .expect("first runtime should initialize");
    }
    {
        let mut locked = rt2.acquire_v8_lock();
        locked
            .execute_script(
                "rt2_init.js",
                "if (typeof globalThis.__nimbusCreateContext !== 'function') throw new Error('bootstrap missing'); globalThis.counter = 10;",
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
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
}
