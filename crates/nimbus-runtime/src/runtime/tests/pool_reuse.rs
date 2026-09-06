//! Pool-reuse + cross-profile shared-RO-heap cage oracle.
//!
//! STRUCTURE (the forensic narrative lives in the memory file, not here):
//! - Every cage-building test is `#[ignore]`'d and runs ONLY via its `isol_*` parent in a
//!   fresh-cage subprocess, wired in the `isolated_pool_reuse_tests!` block. The cage ships in
//!   the prebuilt rusty_v8 regardless of the cargo feature, so an in-process run would crash
//!   the shared binary.
//! - CONTROLS (`crash(N): ...`) abort BY DESIGN; the harness asserts they die with a cage
//!   signature (vector.h:415 / index<size / Unknown external reference / SIGBUS). They do NOT run JS — they
//!   abort during construction, before JS would run.
//! - FIXES (`fix: ...`) must SUCCEED; build-only fixes also execute `BUILTIN_SMOKE_JS` to
//!   assert the isolate runs, not merely constructs.
//! - Run the cage lane with `make test-rust-runtime-cage` (filter `isol_`, feature-on).

use std::rc::Rc;

use deno_core::{JsRuntime, PollEventLoopOptions};

use super::*;
use crate::backends::v8::{ReusableV8Runtime, V8RuntimeConstructionMode, V8WorkerRuntimePool};
use crate::host::HostBridgeFuture;
const BLOCKING_TEST_RECEIVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

fn recv_within<T>(receiver: &std::sync::mpsc::Receiver<T>, context: &str) -> T {
    receiver
        .recv_timeout(BLOCKING_TEST_RECEIVE_TIMEOUT)
        .unwrap_or_else(|error| {
            panic!(
                "{context} within {BLOCKING_TEST_RECEIVE_TIMEOUT:?}; blocking test channel \
                 failed: {error}"
            )
        })
}

fn recv_until_disconnected<T>(receiver: &std::sync::mpsc::Receiver<T>, context: &str) -> Option<T> {
    match receiver.recv_timeout(BLOCKING_TEST_RECEIVE_TIMEOUT) {
        Ok(value) => Some(value),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => None,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("{context} within {BLOCKING_TEST_RECEIVE_TIMEOUT:?}")
        }
    }
}

/// Generates parent dispatchers that run each `#[ignore]`'d cage-sensitive pool_reuse test in
/// a FRESH subprocess (its own V8 cage). This is what makes a `--features
/// v8-pointer-compression` run BOTH safe and meaningful: no cross-test cage-install-order
/// interference, no process-global anchor-state poisoning, and the crash-by-design controls
/// cannot abort the shared test binary.
///
/// `fix: parent => child` asserts the child SUCCEEDS; `crash(N): parent => child` asserts the
/// child ABORTS by a cage signal within N attempts (the non-vacuousness controls — N>1 only
/// for the racy concurrent races). Crash parents are feature-gated because feature-off there is
/// no cage and the child cannot crash. Parents are named `isol_*` and are the runnable entries;
/// the feature-on CI lane filters on `isol_`.
macro_rules! isolated_pool_reuse_tests {
    ( $( $kind:ident $(($attempts:literal))? : $parent:ident => $child:ident ),+ $(,)? ) => {
        $( isolated_pool_reuse_tests!(@gen $kind $(($attempts))? : $parent => $child); )+
    };
    (@gen fix : $parent:ident => $child:ident) => {
        #[test]
        fn $parent() {
            run_v8_sensitive_runtime_test_in_subprocess(IsolatedRuntimeTestCase::new(
                stringify!($child),
                "pool-reuse",
                stringify!($child),
                concat!("runtime::tests::pool_reuse::", stringify!($child)),
            ));
        }
    };
    (@gen crash ($attempts:literal) : $parent:ident => $child:ident) => {
        #[cfg(feature = "v8-pointer-compression")]
        #[test]
        fn $parent() {
            run_v8_crash_control_in_subprocess(
                IsolatedRuntimeTestCase::new(
                    stringify!($child),
                    "pool-reuse",
                    stringify!($child),
                    concat!("runtime::tests::pool_reuse::", stringify!($child)),
                ),
                $attempts,
            );
        }
    };
}

// THE ORACLE WIRING. Each child below is `#[ignore]`'d (defined further down) and runs only
// via its `isol_*` parent here, in its own process. Crash controls assert the bug still
// aborts by signal (non-vacuous); fix/safety tests assert success. Feature-on CI runs these
// `isol_*` parents (filter `isol_`); feature-off they pass trivially through a cage-less child.
isolated_pool_reuse_tests! {
    // crash-by-design controls (must ABORT by cage signal; feature-on only):
    crash(6): isol_concurrent_cross_profile_crashes
        => concurrent_cross_profile_creation_without_drops_does_not_abort,
    crash(6): isol_concurrent_both_profile_crashes
        => concurrent_both_profile_snapshot_creation_does_not_abort,
    crash(2): isol_weblean_first_crashes
        => weblean_installed_first_then_nodefull_does_not_abort,
    crash(2): isol_gate_snapshotted_weblean_crashes
        => gate_snapshotted_weblean_against_nodefull_anchor_ro_intrinsics_correct,
    crash(2): isol_disposed_anchor_thread_exit_crashes
        => disposed_anchor_thread_exit_makes_crash_return,
    // fix / safety / correctness (must SUCCEED):
    fix: isol_anchor_regression_i => anchor_regression_i_weblean_first_forced_nodefull_first,
    fix: isol_anchor_regression_ii => anchor_regression_ii_nodefull_scale_to_zero_anchor_pinned,
    fix: isol_anchor_regression_iii => anchor_regression_iii_cross_profile_refill_green,
    fix: isol_anchor_armed_and_gated => anchor_armed_and_gated_at_v8_backend_creation,
    fix: isol_anchor_floor_fires => anchor_floor_fires_when_armed_but_not_installed,
    fix: isol_anchor_host_call_count => anchor_nodefull_build_host_call_count,
    fix: isol_baseline_weblean => baseline_snapshotted_weblean_ro_intrinsics_correct,
    fix: isol_reachable_fix
        => reachable_fix_unsnapshotted_weblean_against_nodefull_anchor_ro_intrinsics_correct,
    fix: isol_option_c => option_c_both_unsnapshotted_concurrent_does_not_abort,
    fix: isol_audit1_web_api => audit1_unsnapshotted_weblean_web_api_correct,
    fix: isol_audit1b_fetch_deny => audit1b_weblean_fetch_present_and_deny_by_default,
    fix: isol_audit3_negcap => audit3_unsnapshotted_weblean_negative_capability_isolated,
    fix: isol_serial_cross_profile => serial_cross_profile_creation_does_not_abort,
    fix: isol_same_thread_persists => anchor_ro_heap_persists_past_isolate_disposal_same_thread,
    // safe-pattern builders (reliable in isolation; share the cage in-process, so isolate):
    fix: isol_concurrent_snapshot_nodefull => concurrent_snapshot_isolate_creation_does_not_abort,
    fix: isol_reuse_main_context
        => reuse_main_context_execution_under_concurrent_creation_does_not_abort,
    fix: isol_coliveness => cross_thread_coliveness_without_concurrent_creation_does_not_abort,
    fix: isol_coliveness_at_scale
        => coliveness_at_scale_without_concurrent_cross_profile_creation_does_not_abort,
    fix: isol_grouped_fill => grouped_concurrent_fill_does_not_abort,
    fix: isol_arm_blocks_until_installed
        => anchor_arm_blocks_until_installed_window_unreachable_via_create,
    fix: isol_floor_pre_arm
        => anchor_floor_pre_arm_build_records_whether_floor_fires,
    crash(2): isol_direct_path_ws_snapshotted_crashes
        => direct_path_webstandard_snapshotted_crashes_against_production_anchor,
    fix: isol_direct_path_ws_unsnapshotted
        => direct_path_webstandard_unsnapshotted_no_crash_after_fix,
    // --- pre-existing pool_reuse cage tests, isolated (Item 1) ---
    fix: isol_pooled_runtime_invocations_keep_module_state_fresh => pooled_runtime_invocations_keep_module_state_fresh,
    fix: isol_pooled_runtime_invocations_reset_auth_and_host_call_session_state => pooled_runtime_invocations_reset_auth_and_host_call_session_state,
    fix: isol_warm_pooled_runtime_rebinds_host_bridge_per_invocation => warm_pooled_runtime_rebinds_host_bridge_per_invocation,
    fix: isol_reused_runtime_refreshes_invocation_cancellation_state_before_next_invoke => reused_runtime_refreshes_invocation_cancellation_state_before_next_invoke,
    fix: isol_reused_runtime_uses_bound_host_call_session_before_next_invoke => reused_runtime_uses_bound_host_call_session_before_next_invoke,
}

/// PROVE-DON'T-ASSUME (the floor-panic question): is the in-process floor-panic a
/// test-parallelism artifact, or a REAL arm-before-install gap in the production startup? An
/// injected install delay makes the ANCHOR_ENABLED..ANCHOR_INSTALLED window deterministic.
/// (1) A DIRECT isolate build during the window (the only way to hit it, BYPASSING create) is
/// caught by the floor — confirming the floor is the working alarm. (2) `enable_and_arm` (what
/// `V8RuntimeBackendFactory::create` calls) BLOCKS for the full install and returns only with
/// ANCHOR_INSTALLED=true — so the window is UNREACHABLE via the production path. Together: the
/// floor-panic is a parallel test building through a non-create path during another test's
/// install, NOT a production startup gap. (If arm did NOT block, this asserts and the fix has a
/// real ordering bug.)
#[test]
#[ignore = "mutates anchor globals + install delay; run via isol_arm_blocks_until_installed"]
fn anchor_arm_blocks_until_installed_window_unreachable_via_create() {
    use crate::runtime::driver::anchor;
    use std::time::Instant;

    anchor::set_anchor_install_delay_ms_for_test(700);

    // Arm via the PRODUCTION path on a background thread; it must block ~700ms on install.
    let armer = std::thread::spawn(|| {
        let t0 = Instant::now();
        anchor::enable_and_arm_nodefull_anchor();
        t0.elapsed()
    });

    // Wait until we are inside the window: ANCHOR_ENABLED set, ANCHOR_INSTALLED not yet.
    while !anchor::anchor_enabled_for_test() {
        std::thread::yield_now();
    }
    assert!(
        !anchor::anchor_installed_for_test(),
        "with a 700ms install delay we must observe the open window (enabled, not installed)"
    );

    // (1) The floor MUST catch a DIRECT isolate build during the window (bypassing create()).
    let floor_fired = std::panic::catch_unwind(|| {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let _ = owner.bootstrap_snapshot();
    })
    .is_err();
    assert!(
        floor_fired,
        "the floor must catch an isolate built during the install window (the alarm works)"
    );

    // (2) The production arming path BLOCKED for the full install and returned installed.
    let arm_elapsed = armer.join().expect("armer thread should not panic");
    assert!(
        anchor::anchor_installed_for_test(),
        "enable_and_arm must return only after ANCHOR_INSTALLED is true"
    );
    assert!(
        arm_elapsed.as_millis() >= 700,
        "enable_and_arm/create must BLOCK for the full install (took {arm_elapsed:?}); a shorter \
         time would mean an arm-before-install startup gap, NOT a test-only race"
    );
}

/// PROVE-DON'T-ASSUME (what does the floor actually guard?). The floor's DOC claims it catches
/// "an isolate built before the anchor installs — a regression catch for a future init reorder."
/// But `assert_anchor_floor` is gated on `ANCHOR_ENABLED`, which is only set INSIDE
/// `install_nodefull_anchor`. So a build that happens BEFORE the anchor is ever armed (the real
/// "someone reordered init / added a build path before create() arms" case) has
/// ANCHOR_ENABLED=false and the floor is DORMANT. This test builds an isolate in a fresh process
/// with the anchor NEVER armed and records whether the floor fires. Result decides the fix:
/// dormant => the floor only guards the install window (proven test-only) and is WEAKER than its
/// doc claims (strengthen it to catch pre-arm builds, or document it window-only); fires => it is
/// the real pre-arm ordering guard.
#[test]
#[ignore = "own process: asserts floor behavior on a pre-arm build; run via isol_floor_pre_arm"]
fn anchor_floor_pre_arm_build_records_whether_floor_fires() {
    use crate::runtime::driver::anchor;

    assert!(
        !anchor::anchor_enabled_for_test(),
        "precondition: fresh process, anchor never armed (ANCHOR_ENABLED must be false)"
    );

    // Build an isolate directly with the anchor NEVER armed. bootstrap_snapshot ->
    // create_v8_startup_snapshot -> assert_anchor_floor is the floor check on this path.
    let built_ok = std::panic::catch_unwind(|| {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(
                crate::RuntimeLimits::application_node22(),
            )),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        owner
            .bootstrap_snapshot()
            .expect("snapshot builds when the anchor is not armed");
    })
    .is_ok();

    // PROVEN BEHAVIOR: the floor is DORMANT before arming. A pre-arm build does NOT trip it
    // (ANCHOR_ENABLED is false), so the floor as built guards ONLY the install window, not a
    // real pre-arm init reorder. (If this ever fails, the floor started firing pre-arm —
    // re-evaluate the strengthen-vs-document decision.)
    assert!(
        built_ok,
        "floor FIRED on a pre-arm build — it catches pre-arm reorders after all"
    );
}

/// DEMONSTRATION (the construction-mode hazard B surfaced): the direct invocation path
/// `invoke_bundle_unmanaged(None)` (invocation.rs:233, reached by `RuntimeExecutor::invoke`)
/// hardcodes `V8RuntimeConstructionMode::StartupSnapshot` and builds `self`'s profile
/// SNAPSHOTTED — BYPASSING `for_compatibility_target` (the ab6b1c477 pool fix). So a WebStandard
/// runtime invoked through the direct path builds WebStandard SNAPSHOTTED, which deserializes
/// against the NodeFull anchor's superset RO heap and aborts (SIGBUS) — exactly what
/// `gate_snapshotted_weblean_*` proves, but now via the PRODUCTION anchor (`enable_and_arm`,
/// what V8RuntimeBackendFactory::create calls). This reproduces the None-branch's build calls
/// with the production anchor armed: it MUST crash, proving ab6b1c477 is incomplete (the direct
/// path is a second WebStandard-snapshotted crash path). Wired as a CRASH CONTROL.
#[test]
#[ignore = "CRASH CONTROL: direct-path WebStandard-snapshotted build aborts vs the production anchor"]
fn direct_path_webstandard_snapshotted_crashes_against_production_anchor() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    // PRODUCTION anchor: NodeFull installed first, exactly as V8RuntimeBackendFactory::create.
    crate::runtime::driver::anchor::enable_and_arm_nodefull_anchor();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");
    rt.block_on(async {
        // Exactly what invoke_bundle_unmanaged(None) does for a WebStandard `self`: SNAPSHOTTED.
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let bundle = RuntimeBundle::new(&bundle_path);
        let snap = owner
            .bootstrap_snapshot()
            .expect("webstandard snapshot builds");
        let _rt = owner
            .create_runtime_from_snapshot(&bundle, snap)
            .expect("unreachable: WebStandard snapshot SIGBUSes against the NodeFull anchor");
    });
}

/// FIX VERIFICATION: after invocation.rs's None branch honors for_compatibility_target, a
/// WebStandard runtime invoked through the ACTUAL direct path (`invoke_bundle_unmanaged(None)`,
/// what `RuntimeExecutor::invoke` / the server blocking path calls) builds UNSNAPSHOTTED and
/// runs cleanly against the production anchor — no crash. Counterpart to the crash-control above.
#[tokio::test]
#[ignore = "own process (arms anchor): verifies the fixed direct-None path is unsnapshotted"]
async fn direct_path_webstandard_unsnapshotted_no_crash_after_fix() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    // Production anchor (NodeFull installed first), then invoke WebStandard via the direct path.
    crate::runtime::driver::anchor::enable_and_arm_nodefull_anchor();

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: serde_json::Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let runtime_instance = NimbusRuntime::with_policy(
        std::sync::Arc::new(RecordingHost::default()),
        std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let watchdog = WatchdogTimer::new();
    let mut permit =
        SharedInvocationPermit::new(runtime_instance.policy(), None, None, false, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");
    let context = RuntimeInvocationContext::top_level_for_tenant_for_test(&request, "tenant-a");

    let result = runtime_instance
        .invoke_bundle_unmanaged(
            None,
            RuntimeInvocationExecution {
                watchdog: watchdog.clone(),
                bundle: bundle.clone(),
                request: request.clone(),
                context: context.clone(),
                execution_plan: crate::execution_plan::RuntimeExecutionPlan::for_invocation(
                    runtime_instance.policy().as_ref(),
                    &request,
                    &context,
                ),
                external_cancellation: None,
                response_ready_tx: None,
                permit: permit.clone(),
            },
        )
        .await;
    assert!(
        result.is_ok(),
        "direct-path WebStandard must build UNSNAPSHOTTED (no crash) after the fix: {:?}",
        result.err()
    );
}

/// EXPERIMENT: building snapshot-backed V8 isolates concurrently on multiple OS
/// threads must not abort. Without a fix this aborts in
/// SharedHeapDeserializer::DeserializeStringTable (single-cage shared string
/// table). NOT #[ignore]'d here on purpose: this run must show it PASSING with
/// the serialize-creation construction lock active.
#[test]
#[ignore = "cage-isolated: run via isol_concurrent_snapshot_nodefull"]
fn concurrent_snapshot_isolate_creation_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            let warmup = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            warmup
                .bootstrap_snapshot()
                .expect("startup snapshot should build");
        });
    }

    let thread_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(4);
    let barrier = std::sync::Arc::new(crate::test_support::BoundedTestBarrier::new(thread_count));
    let bundle_path = std::sync::Arc::new(bundle_path);

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let barrier = std::sync::Arc::clone(&barrier);
            let bundle_path = std::sync::Arc::clone(&bundle_path);
            std::thread::spawn(move || {
                let worker_rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("worker runtime should build");
                worker_rt.block_on(async move {
                    let runtime_instance = NimbusRuntime::with_policy(
                        std::sync::Arc::new(RecordingHost::default()),
                        std::sync::Arc::new(RuntimePolicy::new(
                            crate::RuntimeLimits::application_node22(),
                        )),
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    );
                    let bundle = RuntimeBundle::new(&*bundle_path);
                    barrier.wait();
                    for _ in 0..3 {
                        let snapshot = runtime_instance
                            .bootstrap_snapshot()
                            .expect("cached startup snapshot");
                        let mut runtime = runtime_instance
                            .create_runtime_from_snapshot(&bundle, snapshot)
                            .expect("isolate should build from snapshot under concurrency");
                        runtime
                            .execute_script("ck", BUILTIN_SMOKE_JS)
                            .expect("built isolate must EXECUTE JS, not merely construct");
                    }
                });
            })
        })
        .collect();

    for handle in handles {
        handle
            .join()
            .expect("concurrent isolate-creation thread should not panic");
    }
}

/// Exercise heavy string interning in retained main contexts concurrently with
/// snapshot-backed isolate creation. The selected architecture must not race a
/// sibling isolate's shared-cage snapshot restoration.
#[test]
#[ignore = "cage-isolated: run via isol_reuse_main_context"]
fn reuse_main_context_execution_under_concurrent_creation_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            let warmup = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            warmup
                .bootstrap_snapshot()
                .expect("startup snapshot should build");
        });
    }

    let thread_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(4);
    let barrier = std::sync::Arc::new(crate::test_support::BoundedTestBarrier::new(thread_count));
    let bundle_path = std::sync::Arc::new(bundle_path);

    let handles: Vec<_> = (0..thread_count)
        .map(|_| {
            let barrier = std::sync::Arc::clone(&barrier);
            let bundle_path = std::sync::Arc::clone(&bundle_path);
            std::thread::spawn(move || {
                let worker_rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("worker runtime should build");
                worker_rt.block_on(async move {
                    let runtime_instance = NimbusRuntime::with_policy(
                        std::sync::Arc::new(RecordingHost::default()),
                        std::sync::Arc::new(RuntimePolicy::new(
                            crate::RuntimeLimits::application_node22(),
                        )),
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    );
                    let bundle = RuntimeBundle::new(&*bundle_path);
                    barrier.wait();
                    for _ in 0..3 {
                        let snapshot = runtime_instance
                            .bootstrap_snapshot()
                            .expect("cached startup snapshot");
                        let mut runtime = runtime_instance
                            .create_runtime_from_snapshot(&bundle, snapshot)
                            .expect("isolate should build from snapshot under concurrency");
                        // Exercise heavy interning in the retained main context.
                        for k in 0..50u32 {
                            let code = format!(
                                "globalThis.__rk_{k} = {{ a{k}: 1, b{k}: 2, c{k}: 3, d{k}: 4 }};"
                            );
                            runtime
                                .execute_script("reuse_ctx_probe", code)
                                .expect("main-context script should run");
                        }
                    }
                });
            })
        })
        .collect();

    for handle in handles {
        handle
            .join()
            .expect("reuse-main-context thread should not panic");
    }
}

/// EXPERIMENT A coverage (the plan's load-bearing check): concurrently create BOTH
/// profile snapshots -- WebLean (RuntimeLimits::default() = WebStandard) and NodeFull
/// (application_node22) -- interleaved across barrier threads. This is the openworkers
/// "different snapshots loaded concurrently into the shared cage" hazard, which the
/// existing barrier tests (NodeFull-only) never exercise. snapshot-everywhere must
/// survive this.
#[test]
#[ignore = "cage-isolated CRASH CONTROL: run via isol_concurrent_both_profile_crashes"]
fn concurrent_both_profile_snapshot_creation_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            for limits in [
                crate::RuntimeLimits::application_node22(),
                crate::RuntimeLimits::default(),
            ] {
                let warmup = NimbusRuntime::with_policy(
                    std::sync::Arc::new(RecordingHost::default()),
                    std::sync::Arc::new(RuntimePolicy::new(limits)),
                    crate::RuntimeEgressPosture::CoarsePermissions,
                );
                warmup
                    .bootstrap_snapshot()
                    .expect("startup snapshot should build");
            }
        });
    }

    let thread_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(4);
    let barrier = std::sync::Arc::new(crate::test_support::BoundedTestBarrier::new(thread_count));
    let bundle_path = std::sync::Arc::new(bundle_path);

    let handles: Vec<_> = (0..thread_count)
        .map(|i| {
            let barrier = std::sync::Arc::clone(&barrier);
            let bundle_path = std::sync::Arc::clone(&bundle_path);
            let is_node = i % 2 == 0;
            std::thread::spawn(move || {
                let worker_rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("worker runtime should build");
                worker_rt.block_on(async move {
                    let limits = if is_node {
                        crate::RuntimeLimits::application_node22()
                    } else {
                        crate::RuntimeLimits::default()
                    };
                    let runtime_instance = NimbusRuntime::with_policy(
                        std::sync::Arc::new(RecordingHost::default()),
                        std::sync::Arc::new(RuntimePolicy::new(limits)),
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    );
                    let bundle = RuntimeBundle::new(&*bundle_path);
                    barrier.wait();
                    for _ in 0..3 {
                        let snapshot = runtime_instance
                            .bootstrap_snapshot()
                            .expect("cached startup snapshot");
                        // CRASH CONTROL: this concurrent cross-profile build aborts before JS
                        // would run — deliberately NO smoke JS (per the controls-vs-fix rule).
                        let _runtime = runtime_instance
                            .create_runtime_from_snapshot(&bundle, snapshot)
                            .expect("isolate should build from snapshot under concurrency");
                    }
                });
            })
        })
        .collect();

    for handle in handles {
        handle
            .join()
            .expect("both-profile concurrent-creation thread should not panic");
    }
}

/// EXPERIMENT (asymmetric endstate): NodeFull keeps its heap snapshot while
/// WebLean is built UNSNAPSHOTTED (`create_runtime(.., None, ..)`). Only ONE
/// snapshot type ever enters the shared cage, so the cross-snapshot
/// DeserializeStringTable conflict that aborts
/// `concurrent_both_profile_snapshot_creation_does_not_abort` cannot occur. This
/// is the proof that "snapshot the expensive profile, unsnapshot the cheap one"
/// removes the race while keeping NodeFull's cold-start advantage. Runs the same
/// interleaved barrier as the both-profile test; must PASS.
#[test]
#[ignore = "cage-isolated FLAKY race demo (~5/12 crash); manual diagnostic, not a CI gate"]
fn concurrent_asymmetric_nodefull_snapshot_weblean_unsnapshotted_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            let warmup = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            warmup
                .bootstrap_snapshot()
                .expect("nodefull startup snapshot should build");
        });
    }

    let thread_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(4);
    let barrier = std::sync::Arc::new(crate::test_support::BoundedTestBarrier::new(thread_count));
    let bundle_path = std::sync::Arc::new(bundle_path);

    let handles: Vec<_> = (0..thread_count)
        .map(|i| {
            let barrier = std::sync::Arc::clone(&barrier);
            let bundle_path = std::sync::Arc::clone(&bundle_path);
            let is_node = i % 2 == 0;
            std::thread::spawn(move || {
                let worker_rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("worker runtime should build");
                worker_rt.block_on(async move {
                    let limits = if is_node {
                        crate::RuntimeLimits::application_node22()
                    } else {
                        crate::RuntimeLimits::default()
                    };
                    let runtime_instance = NimbusRuntime::with_policy(
                        std::sync::Arc::new(RecordingHost::default()),
                        std::sync::Arc::new(RuntimePolicy::new(limits)),
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    );
                    let bundle = RuntimeBundle::new(&*bundle_path);
                    barrier.wait();
                    for _ in 0..3 {
                        if is_node {
                            let snapshot = runtime_instance
                                .bootstrap_snapshot()
                                .expect("cached nodefull startup snapshot");
                            let _runtime = runtime_instance
                                .create_runtime_from_snapshot(&bundle, snapshot)
                                .expect("nodefull isolate should build from snapshot");
                        } else {
                            let _runtime = runtime_instance
                                .create_runtime(&bundle, None, false)
                                .expect("weblean isolate should build unsnapshotted");
                        }
                    }
                });
            })
        })
        .collect();

    for handle in handles {
        handle
            .join()
            .expect("asymmetric concurrent-creation thread should not panic");
    }
}

/// EXPERIMENT (serial cross-profile): create NodeFull and WebLean isolates
/// ALTERNATELY on ONE thread — zero concurrency. If this still aborts, the
/// conflict is COEXISTENCE (two different profile read-only heaps cannot share
/// one cage), not a concurrent-writer race the create-lock could close. If it
/// passes, the crash needs concurrency and the lock has a gap.
#[test]
#[ignore = "cage-isolated: run via isol_serial_cross_profile"]
fn serial_cross_profile_creation_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");
    rt.block_on(async {
        let bundle = RuntimeBundle::new(&bundle_path);
        for round in 0..6 {
            let is_node = round % 2 == 0;
            let limits = if is_node {
                crate::RuntimeLimits::application_node22()
            } else {
                crate::RuntimeLimits::default()
            };
            let runtime_instance = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(limits)),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            if is_node {
                let snapshot = runtime_instance
                    .bootstrap_snapshot()
                    .expect("cached nodefull startup snapshot");
                let mut runtime = runtime_instance
                    .create_runtime_from_snapshot(&bundle, snapshot)
                    .expect("nodefull isolate should build from snapshot");
                runtime
                    .execute_script("ck", BUILTIN_SMOKE_JS)
                    .expect("built nodefull isolate must EXECUTE JS, not merely construct");
            } else {
                let mut runtime = runtime_instance
                    .create_runtime(&bundle, None, false)
                    .expect("weblean isolate should build unsnapshotted");
                runtime
                    .execute_script("ck", BUILTIN_SMOKE_JS)
                    .expect("built weblean isolate must EXECUTE JS, not merely construct");
            }
        }
    });
}

/// EXPERIMENT (cross-thread co-liveness, NO concurrent creation): thread A
/// creates ONE NodeFull isolate and parks it ALIVE+idle on a channel. Only then
/// does the MAIN thread serially create+drop WebLean isolates 10x. Creations
/// never overlap (A is idle while main builds; main is single-threaded), but a
/// different-profile (NodeFull) isolate is LIVE in the cage throughout. If a
/// WebLean build aborts here, the killer is co-LIVENESS of different profiles —
/// and ordering pool-fill would NOT save us. If it passes, the killer is
/// specifically CONCURRENT creation and serialized/ordered fill is safe.
#[test]
#[ignore = "cage-isolated: run via isol_coliveness"]
fn cross_thread_coliveness_without_concurrent_creation_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    let (a_ready_tx, a_ready_rx) = std::sync::mpsc::channel::<()>();
    let (a_release_tx, a_release_rx) = std::sync::mpsc::channel::<()>();
    let bp_a = std::sync::Arc::clone(&bundle_path);
    let handle_a = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("thread-a runtime should build");
        rt.block_on(async move {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let bundle = RuntimeBundle::new(&*bp_a);
            let snapshot = owner
                .bootstrap_snapshot()
                .expect("cached nodefull startup snapshot");
            let mut nodefull = owner
                .create_runtime_from_snapshot(&bundle, snapshot)
                .expect("nodefull isolate should build from snapshot");
            nodefull
                .execute_script("ck", BUILTIN_SMOKE_JS)
                .expect("parked nodefull isolate must EXECUTE JS, not merely construct");
            a_ready_tx.send(()).expect("signal nodefull alive");
            // Park: keep nodefull alive+idle (NOT creating) until released.
            recv_within(&a_release_rx, "test should release parked nodefull isolate");
        });
    });
    recv_within(&a_ready_rx, "nodefull isolate should report ready");

    let main_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("main runtime should build");
    main_rt.block_on(async {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let bundle = RuntimeBundle::new(&*bundle_path);
        for _ in 0..10 {
            let mut weblean = owner
                .create_runtime(&bundle, None, false)
                .expect("weblean isolate should build unsnapshotted while nodefull is alive");
            weblean
                .execute_script("ck", BUILTIN_SMOKE_JS)
                .expect("weblean isolate must EXECUTE JS, not merely construct");
        }
    });

    a_release_tx.send(()).expect("release nodefull");
    handle_a.join().expect("thread-a should not panic");
}

/// EXPERIMENT (co-liveness AT SCALE, no concurrent cross-profile creation): park
/// EIGHT NodeFull isolates alive across eight threads, THEN serially build WebLean
/// isolates on the main thread while all eight are resident. The NodeFull builds
/// are same-profile (safe even concurrently); the only cross-profile event is each
/// WebLean build while eight NodeFull isolates are LIVE — never a cross-profile
/// build overlapping another build. If WebLean aborts here, the killer is
/// co-LIVENESS AT SCALE (both profiles warm together is unsafe). If it passes, the
/// killer is specifically CONCURRENT cross-profile CREATION — i.e. a create-lock
/// gap, and both profiles can stay warm with both snapshots.
#[test]
#[ignore = "cage-isolated: run via isol_coliveness_at_scale"]
fn coliveness_at_scale_without_concurrent_cross_profile_creation_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    const PARKED: usize = 8;
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let release_rx = std::sync::Arc::new(std::sync::Mutex::new(release_rx));

    let handles: Vec<_> = (0..PARKED)
        .map(|_| {
            let bp = std::sync::Arc::clone(&bundle_path);
            let ready_tx = ready_tx.clone();
            let release_rx = std::sync::Arc::clone(&release_rx);
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("parked-thread runtime should build");
                rt.block_on(async move {
                    let owner = NimbusRuntime::with_policy(
                        std::sync::Arc::new(RecordingHost::default()),
                        std::sync::Arc::new(RuntimePolicy::new(
                            crate::RuntimeLimits::application_node22(),
                        )),
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    );
                    let bundle = RuntimeBundle::new(&*bp);
                    let snapshot = owner
                        .bootstrap_snapshot()
                        .expect("cached nodefull startup snapshot");
                    let mut nodefull = owner
                        .create_runtime_from_snapshot(&bundle, snapshot)
                        .expect("nodefull isolate should build from snapshot");
                    nodefull
                        .execute_script("ck", BUILTIN_SMOKE_JS)
                        .expect("parked nodefull isolate must EXECUTE JS, not merely construct");
                    ready_tx.send(()).expect("signal nodefull alive");
                    // Park alive+idle until released.
                    let _guard = release_rx.lock().expect("release lock");
                    recv_within(&_guard, "test should release parked nodefull isolate");
                });
            })
        })
        .collect();

    // Wait until all eight NodeFull isolates are alive (done creating).
    for _ in 0..PARKED {
        recv_within(&ready_rx, "nodefull isolate should report ready");
    }

    let main_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("main runtime should build");
    main_rt.block_on(async {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let bundle = RuntimeBundle::new(&*bundle_path);
        for _ in 0..10 {
            let mut weblean = owner
                .create_runtime(&bundle, None, false)
                .expect("weblean should build while eight nodefull isolates are alive");
            weblean
                .execute_script("ck", BUILTIN_SMOKE_JS)
                .expect("weblean isolate must EXECUTE JS, not merely construct");
        }
    });

    for _ in 0..PARKED {
        release_tx.send(()).expect("release a parked nodefull");
    }
    for handle in handles {
        handle.join().expect("parked thread should not panic");
    }
}

/// EXPERIMENT (concurrent cross-profile CREATION, NO drops): eight threads cross a
/// barrier and each builds exactly ONE isolate (alternating NodeFull/WebLean),
/// then parks it ALIVE — no isolate is ever dropped while others build. This keeps
/// the concurrent cross-profile CREATION of the crashing tests but removes the
/// create-vs-dispose overlap. If this aborts, the killer is concurrent
/// cross-profile CREATION itself (a create-lock gap on the build path). If it
/// passes, the killer is the create-vs-DISPOSE race (the lock fails to serialize
/// isolate teardown against concurrent builds).
#[test]
#[ignore = "cage-isolated CRASH CONTROL: run via isol_concurrent_cross_profile_crashes"]
fn concurrent_cross_profile_creation_without_drops_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    // WARM both profile snapshots first, so no snapshot BUILD happens during the
    // barrier'd concurrent creation. This isolates "two create_runtime calls race
    // under the lock" from "snapshot-build races creation".
    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            for limits in [
                crate::RuntimeLimits::application_node22(),
                crate::RuntimeLimits::default(),
            ] {
                let warmup = NimbusRuntime::with_policy(
                    std::sync::Arc::new(RecordingHost::default()),
                    std::sync::Arc::new(RuntimePolicy::new(limits)),
                    crate::RuntimeEgressPosture::CoarsePermissions,
                );
                warmup
                    .bootstrap_snapshot()
                    .expect("startup snapshot should build");
            }
        });
    }

    let thread_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(8);
    let barrier = std::sync::Arc::new(crate::test_support::BoundedTestBarrier::new(thread_count));
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let release_rx = std::sync::Arc::new(std::sync::Mutex::new(release_rx));

    let handles: Vec<_> = (0..thread_count)
        .map(|i| {
            let bp = std::sync::Arc::clone(&bundle_path);
            let barrier = std::sync::Arc::clone(&barrier);
            let release_rx = std::sync::Arc::clone(&release_rx);
            let is_node = i % 2 == 0;
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("worker runtime should build");
                rt.block_on(async move {
                    let limits = if is_node {
                        crate::RuntimeLimits::application_node22()
                    } else {
                        crate::RuntimeLimits::default()
                    };
                    let owner = NimbusRuntime::with_policy(
                        std::sync::Arc::new(RecordingHost::default()),
                        std::sync::Arc::new(RuntimePolicy::new(limits)),
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    );
                    let bundle = RuntimeBundle::new(&*bp);
                    // All threads build at once; no isolate is dropped meanwhile.
                    barrier.wait();
                    let _runtime = if is_node {
                        let snapshot = owner
                            .bootstrap_snapshot()
                            .expect("cached nodefull startup snapshot");
                        owner
                            .create_runtime_from_snapshot(&bundle, snapshot)
                            .expect("nodefull isolate should build from snapshot")
                    } else {
                        owner
                            .create_runtime(&bundle, None, false)
                            .expect("weblean isolate should build unsnapshotted")
                    };
                    // Park alive: keep _runtime resident, never drop while others build.
                    let _guard = release_rx.lock().expect("release lock");
                    recv_within(&_guard, "test should release concurrently parked isolate");
                });
            })
        })
        .collect();

    // Let the builds settle, then release all parked isolates together.
    let main_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("main runtime should build");
    main_rt.block_on(async {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let bundle = RuntimeBundle::new(&*bundle_path);
        // One extra build on the main thread to ensure all workers are past the barrier.
        let _warm = owner
            .create_runtime(&bundle, None, false)
            .expect("main weblean build should succeed");
    });
    for _ in 0..thread_count {
        release_tx.send(()).expect("release a parked isolate");
    }
    for handle in handles {
        handle.join().expect("worker thread should not panic");
    }
}

/// DIAGNOSTIC (grouped concurrent fill — provisional): build ALL NodeFull
/// isolates concurrently (parked), barrier, THEN build ALL WebLean isolates
/// concurrently (parked). Cross-profile builds NEVER interleave; within each group
/// builds are concurrent. A pass shows ONLY that initial-fill ordering avoids the
/// hazard — provisional, NOT a fix (and may pass for the incidental reason that the
/// second profile only ever deserializes against a fully-settled single-profile
/// cage). The refill test (#1b) is the real decider.
#[test]
#[ignore = "cage-isolated: run via isol_grouped_fill"]
fn grouped_concurrent_fill_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            for limits in [
                crate::RuntimeLimits::application_node22(),
                crate::RuntimeLimits::default(),
            ] {
                let w = NimbusRuntime::with_policy(
                    std::sync::Arc::new(RecordingHost::default()),
                    std::sync::Arc::new(RuntimePolicy::new(limits)),
                    crate::RuntimeEgressPosture::CoarsePermissions,
                );
                w.bootstrap_snapshot()
                    .expect("startup snapshot should build");
            }
        });
    }

    const PER_GROUP: usize = 6;
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let release_rx = std::sync::Arc::new(std::sync::Mutex::new(release_rx));
    let mut handles = Vec::new();

    for &is_node in &[true, false] {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        for _ in 0..PER_GROUP {
            let bp = std::sync::Arc::clone(&bundle_path);
            let ready_tx = ready_tx.clone();
            let release_rx = std::sync::Arc::clone(&release_rx);
            handles.push(std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("worker runtime should build");
                rt.block_on(async move {
                    let limits = if is_node {
                        crate::RuntimeLimits::application_node22()
                    } else {
                        crate::RuntimeLimits::default()
                    };
                    let owner = NimbusRuntime::with_policy(
                        std::sync::Arc::new(RecordingHost::default()),
                        std::sync::Arc::new(RuntimePolicy::new(limits)),
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    );
                    let bundle = RuntimeBundle::new(&*bp);
                    let mut iso = if is_node {
                        let snap = owner
                            .bootstrap_snapshot()
                            .expect("cached nodefull snapshot");
                        owner
                            .create_runtime_from_snapshot(&bundle, snap)
                            .expect("nodefull isolate should build")
                    } else {
                        owner
                            .create_runtime(&bundle, None, false)
                            .expect("weblean isolate should build")
                    };
                    iso.execute_script("ck", BUILTIN_SMOKE_JS)
                        .expect("grouped isolate must EXECUTE JS, not merely construct");
                    ready_tx.send(()).expect("ready signal");
                    let _g = release_rx.lock().expect("release lock");
                    recv_within(&_g, "test should release grouped parked isolate");
                });
            }));
        }
        // Barrier: this whole group must be built+parked before the next starts.
        for _ in 0..PER_GROUP {
            recv_within(&ready_rx, "grouped isolate build should complete");
        }
    }

    for _ in 0..(2 * PER_GROUP) {
        release_tx.send(()).expect("release a parked isolate");
    }
    for h in handles {
        h.join().expect("worker thread should not panic");
    }
}

/// DIAGNOSTIC (cross-profile REFILL): build a settled mixed
/// pool (both profiles resident on parked slot-threads), then repeatedly flip ONE
/// slot to the OPPOSITE profile (drop its isolate, build the other profile) while
/// the other SLOTS-1 isolates stay resident. This is warm-pool steady state:
/// continuous refill RE-INTERLEAVES a cross-profile build into an ACCUMULATED mixed
/// pool — i.e. the #6 crash regime, not the #4 initial-grouped-fill regime. If this
/// aborts, "group fill by profile" is DEAD as a fix (it would pass CI on initial
/// fill and reintroduce the crash on the first cross-profile eviction-refill in
/// production). Build IS serialized by the create-lock; the hazard, if any, is
/// state residue, not live overlap.
#[test]
#[ignore = "cage-isolated FLAKY race demo (~5/12 crash); manual diagnostic, not a CI gate"]
fn cross_profile_refill_into_resident_mixed_pool_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            for limits in [
                crate::RuntimeLimits::application_node22(),
                crate::RuntimeLimits::default(),
            ] {
                let w = NimbusRuntime::with_policy(
                    std::sync::Arc::new(RecordingHost::default()),
                    std::sync::Arc::new(RuntimePolicy::new(limits)),
                    crate::RuntimeEgressPosture::CoarsePermissions,
                );
                w.bootstrap_snapshot()
                    .expect("startup snapshot should build");
            }
        });
    }

    const SLOTS: usize = 8;
    const REFILL_ROUNDS: usize = 24;

    let mut cmd_txs = Vec::new();
    let mut done_rxs = Vec::new();
    let mut handles = Vec::new();
    for _ in 0..SLOTS {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Option<bool>>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let bp = std::sync::Arc::clone(&bundle_path);
        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("slot runtime should build");
            rt.block_on(async move {
                let bundle = RuntimeBundle::new(&*bp);
                // Hold ONE isolate (+ its owner) at a time; drop before rebuilding.
                let mut current: Option<(NimbusRuntime, JsRuntime)> = None;
                while let Some(cmd) = recv_until_disconnected(
                    &cmd_rx,
                    "cross-profile refill slot should receive a command",
                ) {
                    match cmd {
                        Some(is_node) => {
                            // DROP the resident isolate BEFORE building the new profile.
                            drop(current.take());
                            let limits = if is_node {
                                crate::RuntimeLimits::application_node22()
                            } else {
                                crate::RuntimeLimits::default()
                            };
                            let owner = NimbusRuntime::with_policy(
                                std::sync::Arc::new(RecordingHost::default()),
                                std::sync::Arc::new(RuntimePolicy::new(limits)),
                                crate::RuntimeEgressPosture::CoarsePermissions,
                            );
                            let iso = if is_node {
                                let snap = owner
                                    .bootstrap_snapshot()
                                    .expect("cached nodefull snapshot");
                                owner
                                    .create_runtime_from_snapshot(&bundle, snap)
                                    .expect("nodefull isolate should build")
                            } else {
                                owner
                                    .create_runtime(&bundle, None, false)
                                    .expect("weblean isolate should build")
                            };
                            current = Some((owner, iso));
                            done_tx.send(()).expect("slot done signal");
                        }
                        None => break,
                    }
                }
                drop(current.take());
            });
        });
        cmd_txs.push(cmd_tx);
        done_rxs.push(done_rx);
        handles.push(handle);
    }

    // Phase 1: build a settled mixed pool (alternating profiles), all resident.
    let mut slot_is_node = [false; SLOTS];
    for i in 0..SLOTS {
        slot_is_node[i] = i % 2 == 0;
        cmd_txs[i]
            .send(Some(slot_is_node[i]))
            .expect("send initial build");
    }
    for rx in &done_rxs {
        recv_within(rx, "initial cross-profile slot build should complete");
    }

    // Phase 2: continuous cross-profile REFILL into the resident mixed pool.
    for round in 0..REFILL_ROUNDS {
        let j = round % SLOTS;
        slot_is_node[j] = !slot_is_node[j];
        cmd_txs[j]
            .send(Some(slot_is_node[j]))
            .expect("send refill build");
        recv_within(&done_rxs[j], "cross-profile refill build should complete");
    }

    for tx in &cmd_txs {
        let _ = tx.send(None);
    }
    for h in handles {
        h.join().expect("slot thread should not panic");
    }
}

/// DIRECTIONALITY A (predict ABORT): WebLean (smaller RO heap) installs the shared
/// read-only heap FIRST and stays resident; then NodeFull (larger RO heap)
/// deserializes. Per the core-dump mechanism (`ReadReadOnlyHeapRef`,
/// deserializer.cc:1165, OOB during `DeserializeStringTable`), NodeFull's snapshot
/// references an RO-heap slot beyond WebLean's smaller installed RO heap → vector
/// OOB. This is the REVERSE of `cross_thread_coliveness_*` (which parked NodeFull
/// first and was SAFE). A crash here proves the killer is RO-heap-size ORDER.
#[test]
#[ignore = "cage-isolated CRASH CONTROL: run via isol_weblean_first_crashes"]
fn weblean_installed_first_then_nodefull_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            for limits in [
                crate::RuntimeLimits::application_node22(),
                crate::RuntimeLimits::default(),
            ] {
                let w = NimbusRuntime::with_policy(
                    std::sync::Arc::new(RecordingHost::default()),
                    std::sync::Arc::new(RuntimePolicy::new(limits)),
                    crate::RuntimeEgressPosture::CoarsePermissions,
                );
                w.bootstrap_snapshot()
                    .expect("startup snapshot should build");
            }
        });
    }

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let bp = std::sync::Arc::clone(&bundle_path);
    let handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("weblean runtime should build");
        rt.block_on(async move {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let bundle = RuntimeBundle::new(&*bp);
            // WebLean installs the shared RO heap first, then stays resident.
            let _web = owner
                .create_runtime(&bundle, None, false)
                .expect("weblean should build and install the shared RO heap");
            ready_tx.send(()).expect("signal weblean installed");
            recv_within(&release_rx, "test should release parked weblean isolate");
        });
    });
    recv_within(&ready_rx, "weblean isolate should report installed");

    let main_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("main runtime should build");
    main_rt.block_on(async {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(
                crate::RuntimeLimits::application_node22(),
            )),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let bundle = RuntimeBundle::new(&*bundle_path);
        for _ in 0..3 {
            let snap = owner
                .bootstrap_snapshot()
                .expect("cached nodefull snapshot");
            let _node = owner
                .create_runtime_from_snapshot(&bundle, snap)
                .expect("nodefull builds against weblean-installed RO heap");
        }
    });

    release_tx.send(()).expect("release weblean");
    handle.join().expect("weblean thread should not panic");
}

/// FIX (anchor installed first): install the shared read-only heap ONCE from the
/// LARGEST-RO-footprint profile (NodeFull) via a resident "anchor" isolate BEFORE
/// any other profile builds, then run the EXACT cross-profile refill regime that
/// crashed 5/12 (`cross_profile_refill_*`). If the anchor makes it SAFE, the fix is
/// "install the shared RO heap from the superset profile first" — keeps BOTH
/// snapshots, no multi-cage, ordering only at process startup. (Caveat NOT covered
/// here: silent RO-object aliasing — a follow-up must RUN representative JS in
/// WebLean isolates built against NodeFull's RO heap to rule out wrong-object reads.)
#[test]
#[ignore = "MANUAL-ANCHOR diagnostic, demoted from the cage lane. Measured 400/400 clean in \
            isolation (NOT a steady rate), but it crashed ONCE under cage-lane LOAD with vector.h:415 \
            — a load-triggered window race: the hand-rolled anchor here neither blocks-until-installed \
            NOR arms the floor, so under scheduling jitter a refill build can install a smaller heap \
            before NodeFull. The PRODUCTION-anchor twin over the same refill (anchor_regression_iii, \
            install_nodefull_anchor: blocks on install + arms the floor) measured 400/400 clean and is \
            the wired gate. So this is a test-only artifact of the hand-rolled anchor, not a \
            production race. Manual repro only."]
fn nodefull_anchor_first_then_cross_profile_refill_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            for limits in [
                crate::RuntimeLimits::application_node22(),
                crate::RuntimeLimits::default(),
            ] {
                let w = NimbusRuntime::with_policy(
                    std::sync::Arc::new(RecordingHost::default()),
                    std::sync::Arc::new(RuntimePolicy::new(limits)),
                    crate::RuntimeEgressPosture::CoarsePermissions,
                );
                w.bootstrap_snapshot()
                    .expect("startup snapshot should build");
            }
        });
    }

    // ANCHOR: install the shared RO heap from NodeFull (superset) and keep resident.
    let (aready_tx, aready_rx) = std::sync::mpsc::channel::<()>();
    let (arelease_tx, arelease_rx) = std::sync::mpsc::channel::<()>();
    let abp = std::sync::Arc::clone(&bundle_path);
    let anchor = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("anchor runtime should build");
        rt.block_on(async move {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let bundle = RuntimeBundle::new(&*abp);
            let snap = owner
                .bootstrap_snapshot()
                .expect("cached nodefull snapshot");
            let _anchor_node = owner
                .create_runtime_from_snapshot(&bundle, snap)
                .expect("anchor nodefull should install the superset RO heap");
            aready_tx.send(()).expect("signal anchor installed");
            recv_within(&arelease_rx, "test should release parked anchor isolate");
        });
    });
    recv_within(&aready_rx, "anchor isolate should report installed");

    // Now the exact #1(b) refill regime, with NodeFull's RO heap already installed.
    const SLOTS: usize = 8;
    const REFILL_ROUNDS: usize = 24;
    let mut cmd_txs = Vec::new();
    let mut done_rxs = Vec::new();
    let mut handles = Vec::new();
    for _ in 0..SLOTS {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Option<bool>>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let bp = std::sync::Arc::clone(&bundle_path);
        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("slot runtime should build");
            rt.block_on(async move {
                let bundle = RuntimeBundle::new(&*bp);
                let mut current: Option<(NimbusRuntime, JsRuntime)> = None;
                while let Some(cmd) = recv_until_disconnected(
                    &cmd_rx,
                    "anchored refill slot should receive a command",
                ) {
                    match cmd {
                        Some(is_node) => {
                            drop(current.take());
                            let limits = if is_node {
                                crate::RuntimeLimits::application_node22()
                            } else {
                                crate::RuntimeLimits::default()
                            };
                            let owner = NimbusRuntime::with_policy(
                                std::sync::Arc::new(RecordingHost::default()),
                                std::sync::Arc::new(RuntimePolicy::new(limits)),
                                crate::RuntimeEgressPosture::CoarsePermissions,
                            );
                            let iso = if is_node {
                                let snap = owner
                                    .bootstrap_snapshot()
                                    .expect("cached nodefull snapshot");
                                owner
                                    .create_runtime_from_snapshot(&bundle, snap)
                                    .expect("nodefull isolate should build")
                            } else {
                                owner
                                    .create_runtime(&bundle, None, false)
                                    .expect("weblean isolate should build")
                            };
                            current = Some((owner, iso));
                            done_tx.send(()).expect("slot done signal");
                        }
                        None => break,
                    }
                }
                drop(current.take());
            });
        });
        cmd_txs.push(cmd_tx);
        done_rxs.push(done_rx);
        handles.push(handle);
    }

    let mut slot_is_node = [false; SLOTS];
    for i in 0..SLOTS {
        slot_is_node[i] = i % 2 == 0;
        cmd_txs[i]
            .send(Some(slot_is_node[i]))
            .expect("send initial build");
    }
    for rx in &done_rxs {
        recv_within(rx, "initial anchored slot build should complete");
    }
    for round in 0..REFILL_ROUNDS {
        let j = round % SLOTS;
        slot_is_node[j] = !slot_is_node[j];
        cmd_txs[j]
            .send(Some(slot_is_node[j]))
            .expect("send refill build");
        recv_within(&done_rxs[j], "anchored refill build should complete");
    }
    for tx in &cmd_txs {
        let _ = tx.send(None);
    }
    for h in handles {
        h.join().expect("slot thread should not panic");
    }

    arelease_tx.send(()).expect("release anchor");
    anchor.join().expect("anchor thread should not panic");
}

/// Correctness probe for RO-heap-resident objects: exercises interned property-name
/// strings, well-known symbols, prototype/map identity, builtins, and Node-leak
/// Profile-AGNOSTIC builtin smoke test (Array/String/JSON/Object — present on every V8
/// profile). THROWS (→ execute_script Err) if any ECMAScript builtin is broken. Lets a
/// build-only "does_not_abort" fix test assert the isolate EXECUTES correctly, not merely
/// that construction didn't crash. (Crash controls abort before JS would run, so they do NOT
/// use this.)
const BUILTIN_SMOKE_JS: &str = r#"(() => {
  if ([1, 2, 3].map(x => x * 2).join(',') !== '2,4,6') throw new Error('Array');
  if ('AB'.toLowerCase() !== 'ab') throw new Error('String');
  if (JSON.stringify({ a: 1 }) !== '{"a":1}') throw new Error('JSON');
  if (Object.keys({ x: 1, y: 2 }).length !== 2) throw new Error('Object');
  'ok'
})()"#;

/// sentinels. THROWS (→ execute_script Err) on ANY mismatch; returns 0 on full pass.
/// Used to detect SILENT cross-profile primordial aliasing — a snapshotted WebLean
/// isolate resolving an in-bounds-but-WRONG RO object against the NodeFull anchor.
const RO_INTRINSIC_CHECKS_JS: &str = r#"(() => {
  const fails = [];
  const ck = (c, m) => { if (!c) fails.push(m); };
  ck(typeof Object === 'function', 'Object');
  ck(typeof Array === 'function', 'Array');
  ck(typeof Function === 'function', 'Function');
  ck(typeof String === 'function', 'String');
  ck(typeof Number === 'function', 'Number');
  ck(typeof Symbol === 'function', 'Symbol');
  ck(typeof Map === 'function', 'Map');
  ck(typeof Set === 'function', 'Set');
  ck(typeof Promise === 'function', 'Promise');
  ck(typeof JSON === 'object', 'JSON');
  ck(typeof Math === 'object', 'Math');
  ck(Object.getPrototypeOf([]) === Array.prototype, 'array-proto');
  ck(Object.getPrototypeOf({}) === Object.prototype, 'object-proto');
  ck(Object.getPrototypeOf(() => {}) === Function.prototype, 'fn-proto');
  ck([].constructor === Array, 'array-ctor');
  ck(({}).constructor === Object, 'object-ctor');
  ck(Object.prototype.toString.call([]) === '[object Array]', 'tostr-array');
  ck(Object.prototype.toString.call(null) === '[object Null]', 'tostr-null');
  ck(typeof Symbol.iterator === 'symbol', 'sym-iterator');
  ck(typeof Symbol.asyncIterator === 'symbol', 'sym-asynciterator');
  ck(typeof Symbol.hasInstance === 'symbol', 'sym-hasinstance');
  ck(typeof Symbol.toStringTag === 'symbol', 'sym-tostringtag');
  ck(Array.prototype[Symbol.iterator] !== undefined, 'array-iter-method');
  ck('hello'.length === 5, 'str-len');
  ck('hello'.toUpperCase() === 'HELLO', 'str-upper');
  ck('a,b,c'.split(',').join('|') === 'a|b|c', 'str-split-join');
  ck('abc'.charCodeAt(0) === 97, 'str-charcode');
  ck(String.fromCharCode(98, 99) === 'bc', 'str-fromcharcode');
  ck('x'.repeat(3) === 'xxx', 'str-repeat');
  ck([3, 1, 2].sort().join('') === '123', 'arr-sort');
  ck([1, 2, 3].map(x => x * 2).join(',') === '2,4,6', 'arr-map');
  ck([1, 2, 3, 4].filter(x => x % 2 === 0).join('') === '24', 'arr-filter');
  ck([1, 2, 3].reduce((a, b) => a + b, 0) === 6, 'arr-reduce');
  ck(Array.from({ length: 3 }, (_, i) => i).join('') === '012', 'arr-from');
  ck(Array.isArray([]) === true && Array.isArray({}) === false, 'arr-isarray');
  ck(Object.keys({ a: 1, b: 2, c: 3 }).join('') === 'abc', 'obj-keys');
  ck(Object.values({ a: 1, b: 2 }).join('') === '12', 'obj-values');
  ck(Object.assign({}, { a: 1 }, { b: 2 }).b === 2, 'obj-assign');
  ck(JSON.stringify({ x: 1, y: [2, 3], z: 's' }) === '{"x":1,"y":[2,3],"z":"s"}', 'json-stringify');
  ck(JSON.parse('{"k":7,"a":[1,2]}').a[1] === 2, 'json-parse');
  const o = { length: 11, name: 'q', prototype: 5, constructor: 6, value: 7, message: 8 };
  ck(o.length === 11 && o.name === 'q' && o.prototype === 5 && o.constructor === 6 && o.value === 7 && o.message === 8, 'interned-prop-names');
  ck(Math.max(1, 5, 3) === 5 && Math.min(2, 0, 9) === 0, 'math-maxmin');
  ck(Number.parseInt('42', 10) === 42, 'num-parseint');
  ck((255).toString(16) === 'ff', 'num-tostring-radix');
  ck(new Map([['a', 1], ['b', 2]]).get('b') === 2, 'map-get');
  ck(new Set([1, 1, 2, 3, 3]).size === 3, 'set-size');
  ck([] instanceof Array && [] instanceof Object, 'instanceof-array');
  ck((() => {}) instanceof Function, 'instanceof-fn');
  ck(new Error('boom') instanceof Error, 'instanceof-error');
  ck((new Error('boom')).message === 'boom', 'error-message');
  if (!globalThis.__isNodeProfile) {
    ck(typeof process === 'undefined', 'leak-process');
    ck(typeof Buffer === 'undefined', 'leak-Buffer');
    ck(typeof require === 'undefined', 'leak-require');
  }
  if (fails.length) throw new Error('RO checks failed (' + fails.length + '): ' + fails.join(', '));
  return fails.length;
})()"#;

/// GATE BASELINE: snapshotted WebLean installs its OWN RO heap, then runs the
/// intrinsic checks. MUST PASS — proves the checks themselves are valid (not buggy),
/// so a gate failure is real aliasing, not a bad probe.
#[test]
#[ignore = "cage-isolated: run via isol_baseline_weblean"]
fn baseline_snapshotted_weblean_ro_intrinsics_correct() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");
    rt.block_on(async {
        // First (and only) isolate: snapshotted WebLean installs its own RO heap.
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let bundle = RuntimeBundle::new(&bundle_path);
        let snap = owner
            .bootstrap_snapshot()
            .expect("web standard snapshot should build");
        let mut web = owner
            .create_runtime_from_snapshot(&bundle, snap)
            .expect("snapshotted weblean should build on its own RO heap");
        let result = web.execute_script("ro_intrinsic_checks", RO_INTRINSIC_CHECKS_JS);
        assert!(
            result.is_ok(),
            "BASELINE checks FAILED on WebLean's own RO heap (probe is buggy, not aliasing): {:?}",
            result.err()
        );
    });
}

/// THE GATE (blocks the anchor fix): install the NodeFull superset RO heap via a
/// resident anchor, then build SNAPSHOTTED WebLean (real production path — WITH
/// WebLean's snapshot, so it DOES call ReadReadOnlyHeapRef) against that anchored RO
/// heap, and run the intrinsic checks. PASS = the RO heap is a clean prefix-superset,
/// the anchor fix is real (both snapshots kept, ship). FAIL (wrong-object read /
/// identity mismatch, even without a crash) = silent aliasing → anchor fix DEAD →
/// the snapshot question reopens toward a COMMON BASE RO heap.
#[test]
#[ignore = "cage-isolated CRASH CONTROL: run via isol_gate_snapshotted_weblean_crashes"]
fn gate_snapshotted_weblean_against_nodefull_anchor_ro_intrinsics_correct() {
    // NOT on the audit_build_weblean_on_nodefull_anchor helper: this is a CRASH CONTROL that
    // builds SNAPSHOTTED WebStandard (the helper builds unsnapshotted) and ABORTS — no check.
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            for limits in [
                crate::RuntimeLimits::application_node22(),
                crate::RuntimeLimits::default(),
            ] {
                let w = NimbusRuntime::with_policy(
                    std::sync::Arc::new(RecordingHost::default()),
                    std::sync::Arc::new(RuntimePolicy::new(limits)),
                    crate::RuntimeEgressPosture::CoarsePermissions,
                );
                w.bootstrap_snapshot()
                    .expect("startup snapshot should build");
            }
        });
    }

    // ANCHOR: NodeFull installs the superset RO heap first and stays resident.
    let (aready_tx, aready_rx) = std::sync::mpsc::channel::<()>();
    let (arelease_tx, arelease_rx) = std::sync::mpsc::channel::<()>();
    let abp = std::sync::Arc::clone(&bundle_path);
    let anchor = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("anchor runtime should build");
        rt.block_on(async move {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let bundle = RuntimeBundle::new(&*abp);
            let snap = owner
                .bootstrap_snapshot()
                .expect("cached nodefull snapshot");
            let _anchor_node = owner
                .create_runtime_from_snapshot(&bundle, snap)
                .expect("anchor nodefull should install the superset RO heap");
            aready_tx.send(()).expect("signal anchor installed");
            recv_within(&arelease_rx, "test should release parked anchor isolate");
        });
    });
    recv_within(&aready_rx, "anchor isolate should report installed");

    // Snapshotted WebLean against the NodeFull-anchored RO heap + correctness checks.
    let main_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("main runtime should build");
    main_rt.block_on(async {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
        crate::RuntimeEgressPosture::CoarsePermissions);
        let bundle = RuntimeBundle::new(&*bundle_path);
        let snap = owner
            .bootstrap_snapshot()
            .expect("cached web standard snapshot");
        // This snapshotted-WebStandard deserialize SIGBUSes against the
        // NodeFull anchor RO heap — WebLean's snapshot RO refs are in-bounds but
        // resolve to wrong/incompatible objects. The RO heaps are NOT a clean
        // prefix-superset, so the anchor fix is dead for snapshotted WebLean.
        let mut web = owner
            .create_runtime_from_snapshot(&bundle, snap)
            .expect("snapshotted weblean should build against the nodefull anchor RO heap");
        let result = web.execute_script("ro_intrinsic_checks", RO_INTRINSIC_CHECKS_JS);
        assert!(
            result.is_ok(),
            "GATE FAILED: snapshotted WebLean read WRONG RO objects against the NodeFull anchor (silent aliasing → anchor fix DEAD, need common-base RO heap): {:?}",
            result.err()
        );
    });

    arelease_tx.send(()).expect("release anchor");
    anchor.join().expect("anchor thread should not panic");
}

/// REACHABLE-FIX CORRECTNESS GATE (one-profile-code-cache): NodeFull anchor installs
/// the superset RO heap first, then UNSNAPSHOTTED WebLean (`create_runtime(None)` —
/// the code-cache production path, which NEVER calls ReadReadOnlyHeapRef) is built
/// against it and runs the full RO-intrinsic probe. This is the analog of
/// `gate_snapshotted_weblean_*` for the code-cache path. The snapshotted gate
/// SIGBUSed; this MUST PASS (correct reads, not just no-crash) for one-profile-code-
/// cache to be the real fix — proving unsnapshotted WebLean reads NodeFull's superset
/// RO builtins correctly.
#[test]
#[ignore = "cage-isolated: run via isol_reachable_fix"]
fn reachable_fix_unsnapshotted_weblean_against_nodefull_anchor_ro_intrinsics_correct() {
    // Shares the audit_build_weblean_on_nodefull_anchor scaffold (warmup + NodeFull anchor +
    // unsnapshotted WebStandard build); only the RO-intrinsic assertion is test-specific.
    audit_build_weblean_on_nodefull_anchor(|web| {
        let result = web.execute_script("ro_intrinsic_checks", RO_INTRINSIC_CHECKS_JS);
        assert!(
            result.is_ok(),
            "REACHABLE-FIX GATE FAILED: unsnapshotted WebLean read WRONG intrinsics against NodeFull anchor (one-profile-code-cache is NOT correct): {:?}",
            result.err()
        );
    });
}

/// PROVE-DON'T-ASSUME (anchor pinning, Step 2a — PART 1, the half-truth). On ONE long-lived
/// thread: (A) NodeFull snapshot installs the SUPERSET RO heap, then is DROPPED; (B)
/// UNSNAPSHOTTED WebStandard builds; (C) NodeFull snapshot builds again. PASSES — the cage RO
/// heap survives ISOLATE disposal *while the installing thread stays alive*. This fact is
/// REAL but MISLEADING for the anchor design: the production anchor runs on a SEPARATE thread,
/// so the relevant question is whether the install survives that thread's EXIT — answered by
/// `disposed_anchor_thread_exit_makes_crash_return` (it does NOT). Kept as the explicit
/// counter-evidence so a future maintainer does not re-derive "dispose is safe" from this
/// same-thread observation alone.
#[test]
#[ignore = "subprocess/--exact: V8-sensitive cross-profile builds; isolate from sibling tests"]
fn anchor_ro_heap_persists_past_isolate_disposal_same_thread() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");
    rt.block_on(async {
        let bundle = RuntimeBundle::new(&bundle_path);
        // (A) NodeFull snapshot installs the superset RO heap, then is dropped.
        {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let snap = owner.bootstrap_snapshot().expect("nodefull snapshot A");
            let a = owner
                .create_runtime_from_snapshot(&bundle, snap)
                .expect("nodefull A installs the superset RO heap");
            drop(a);
        }
        // (B) UNSNAPSHOTTED WebStandard builds on the same still-alive thread.
        {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let b = owner
                .create_runtime(&bundle, None, false)
                .expect("unsnapshotted weblean B builds");
            drop(b);
        }
        // (C) NodeFull snapshot again: builds (heap survived isolate disposal on this thread).
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(
                crate::RuntimeLimits::application_node22(),
            )),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let snap = owner.bootstrap_snapshot().expect("nodefull snapshot C");
        let mut c = owner
            .create_runtime_from_snapshot(&bundle, snap)
            .expect("nodefull C builds (RO heap survived isolate disposal on this thread)");
        let source = format!("globalThis.__isNodeProfile = true;\n{RO_INTRINSIC_CHECKS_JS}");
        let result = c.execute_script("ro_intrinsic_checks", source);
        assert!(
            result.is_ok(),
            "post-drop NodeFull read WRONG intrinsics: {:?}",
            result.err()
        );
    });
}

/// PROVE-DON'T-ASSUME (anchor pinning, Step 2a — PART 2, the DECIDER). Replicates the REAL
/// anchor: build NodeFull on a DEDICATED thread that then EXITS (dispose + join — exactly a
/// dispose-after-install anchor), then build cross-profile isolates on a DIFFERENT thread. The
/// cage RO-heap install does NOT survive the installing thread's exit, so a later WebStandard
/// re-installs the default SMALLER heap and the final NodeFull snapshot OOBs (`vector.h:415`)
/// and ABORTS. This is the proof that the pin is LOAD-BEARING: the anchor MUST keep its
/// isolate + thread resident. (Contrast the same-thread test, which passes.) Wired as a CRASH
/// CONTROL — the parent asserts this child dies by signal. If this ever STOPS crashing,
/// dispose-after-install just became safe and the parked anchor can be reclaimed.
#[test]
#[ignore = "CRASH CONTROL: aborts by cage signal by design; run only via the crash harness"]
fn disposed_anchor_thread_exit_makes_crash_return() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    // (A) NodeFull built + DISPOSED on a dedicated thread that then EXITS (join).
    let abp = std::sync::Arc::clone(&bundle_path);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("anchor runtime should build");
        rt.block_on(async move {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let snap = owner.bootstrap_snapshot().expect("nodefull snapshot");
            let a = owner
                .create_runtime_from_snapshot(&RuntimeBundle::new(&*abp), snap)
                .expect("nodefull installs the superset RO heap");
            drop(a);
        });
    })
    .join()
    .expect("anchor thread joins (install + dispose + EXIT)");

    // (B,C) cross-profile builds on a DIFFERENT thread: WebStandard re-installs the smaller
    // heap, then NodeFull snapshot OOB-aborts the process.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("main runtime should build");
    rt.block_on(async {
        let bundle = RuntimeBundle::new(&*bundle_path);
        let web_owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let _web = web_owner
            .create_runtime(&bundle, None, false)
            .expect("weblean builds (installs smaller heap after anchor thread exit)");
        let node_owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(
                crate::RuntimeLimits::application_node22(),
            )),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let snap = node_owner
            .bootstrap_snapshot()
            .expect("nodefull snapshot 2");
        let _node = node_owner
            .create_runtime_from_snapshot(&bundle, snap)
            .expect("unreachable: nodefull OOB-aborts against the smaller heap");
    });
}

/// OPTION C SAFETY GATE (both-code-cache): build BOTH profiles UNSNAPSHOTTED
/// (`create_runtime(None)`) concurrently in the warmed-no-drops regime that crashes
/// 16/16 WITH snapshots. With no snapshot in the cage, neither profile calls
/// ReadReadOnlyHeapRef and both bootstrap live against the SAME default V8 RO heap,
/// so there should be no cross-profile RO conflict. MUST PASS for (C) to be viable —
/// proves the no-snapshot path removes the cage incompatibility structurally (no
/// anchor invariant needed). Also runs the intrinsic probe in one isolate of each
/// profile to confirm unsnapshotted bootstrap is correct, not just non-crashing.
#[test]
#[ignore = "cage-isolated: run via isol_option_c"]
fn option_c_both_unsnapshotted_concurrent_does_not_abort() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    let thread_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(8);
    let barrier = std::sync::Arc::new(crate::test_support::BoundedTestBarrier::new(thread_count));
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let release_rx = std::sync::Arc::new(std::sync::Mutex::new(release_rx));

    let handles: Vec<_> = (0..thread_count)
        .map(|i| {
            let bp = std::sync::Arc::clone(&bundle_path);
            let barrier = std::sync::Arc::clone(&barrier);
            let release_rx = std::sync::Arc::clone(&release_rx);
            let is_node = i % 2 == 0;
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("worker runtime should build");
                rt.block_on(async move {
                    let limits = if is_node {
                        crate::RuntimeLimits::application_node22()
                    } else {
                        crate::RuntimeLimits::default()
                    };
                    let owner = NimbusRuntime::with_policy(
                        std::sync::Arc::new(RecordingHost::default()),
                        std::sync::Arc::new(RuntimePolicy::new(limits)),
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    );
                    let bundle = RuntimeBundle::new(&*bp);
                    barrier.wait();
                    // BOTH profiles unsnapshotted — no snapshot ever enters the cage.
                    let _runtime = owner
                        .create_runtime(&bundle, None, false)
                        .expect("unsnapshotted isolate should build (no snapshot in cage)");
                    let _guard = release_rx.lock().expect("release lock");
                    recv_within(&_guard, "test should release unsnapshotted parked isolate");
                });
            })
        })
        .collect();

    // Correctness: while the unsnapshotted pool is resident, build one isolate of
    // EACH profile unsnapshotted and run the intrinsic probe — assert correct reads.
    let main_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("main runtime should build");
    main_rt.block_on(async {
        for limits in [
            crate::RuntimeLimits::application_node22(),
            crate::RuntimeLimits::default(),
        ] {
            let is_node = limits.compatibility_target.is_node();
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(limits)),
            crate::RuntimeEgressPosture::CoarsePermissions);
            let bundle = RuntimeBundle::new(&*bundle_path);
            let mut iso = owner
                .create_runtime(&bundle, None, false)
                .expect("unsnapshotted isolate should build for correctness check");
            // WebLean asserts NO Node-global leak; NodeFull legitimately has process/Buffer.
            let source = format!("globalThis.__isNodeProfile = {is_node};\n{RO_INTRINSIC_CHECKS_JS}");
            let result = iso.execute_script("ro_intrinsic_checks", source);
            assert!(
                result.is_ok(),
                "OPTION C correctness FAILED ({}): unsnapshotted isolate read wrong intrinsics: {:?}",
                if is_node { "nodefull" } else { "weblean" },
                result.err()
            );
        }
    });

    for _ in 0..thread_count {
        release_tx.send(()).expect("release a parked isolate");
    }
    for handle in handles {
        handle.join().expect("worker thread should not panic");
    }
}

/// Enumerates the COMPLETE own-property + descriptor shape of every shared primordial
/// (constructors + their .prototype + own symbols), canonicalized + sorted, and THROWS
/// it as the error message (exfiltration channel). Used by the AUDIT #2 primordial-shape
/// diff: compare WebLean on its OWN (vanilla default) RO heap vs WebLean on the NodeFull
/// anchor RO heap. Any difference = a Node extension mutated a shared primordial before
/// it was frozen into NodeFull's RO heap, and WebLean inherits it = Node bleed = (A) dead.
const PRIMORDIAL_SHAPE_JS: &str = r#"(() => {
  const P = {
    Object, Array, Function, String, Number, Boolean, Symbol, BigInt,
    Error, TypeError, RangeError, SyntaxError, ReferenceError, EvalError, URIError,
    Promise, Map, Set, WeakMap, WeakSet, RegExp, Date, Proxy, Reflect, JSON, Math,
    ArrayBuffer, DataView, Int8Array, Uint8Array, Float64Array,
  };
  const targets = [];
  for (const k of Object.keys(P).sort()) {
    const v = P[k];
    if (v == null) { targets.push([k, null]); continue; }
    targets.push([k, v]);
    if (typeof v === 'function' && v.prototype) targets.push([k + '.prototype', v.prototype]);
  }
  const lines = [];
  for (const [name, obj] of targets) {
    if (obj == null) { lines.push(name + '|<ABSENT>'); continue; }
    const emit = (key, label) => {
      const d = Object.getOwnPropertyDescriptor(obj, key);
      if (!d) { lines.push(name + '|' + label + '|<no-desc>'); return; }
      const f = (d.writable ? 'w' : '-') + (d.enumerable ? 'e' : '-') + (d.configurable ? 'c' : '-') + (d.get ? 'G' : '-') + (d.set ? 'S' : '-');
      const vt = ('value' in d) ? (typeof d.value) : 'accessor';
      lines.push(name + '|' + label + '|' + f + '|' + vt);
    };
    for (const p of Object.getOwnPropertyNames(obj).sort()) emit(p, p);
    for (const s of Object.getOwnPropertySymbols(obj)) emit(s, String(s));
  }
  lines.sort();
  throw new Error('SHAPE\n' + lines.join('\n'));
})()"#;

/// AUDIT #2 capture harness (env-gated): build WebLean unsnapshotted and dump its
/// primordial shape to NIMBUS_SHAPE_OUT. With NIMBUS_SHAPE_ANCHORED=1, a NodeFull anchor
/// installs its (superset) RO heap FIRST so WebLean rides it; else WebLean installs the
/// vanilla default RO heap. Run twice + diff the two files: IDENTICAL = no Node bleed,
/// ANY diff = (A) dead.
#[test]
#[ignore = "env-gated DEV TOOL (NIMBUS_SHAPE_*), not a regression assertion; run manually"]
fn capture_weblean_primordial_shape() {
    let anchored = std::env::var("NIMBUS_SHAPE_ANCHORED")
        .map(|v| v == "1")
        .unwrap_or(false);
    let out = std::env::var("NIMBUS_SHAPE_OUT").expect("NIMBUS_SHAPE_OUT must be set");
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            for limits in [
                crate::RuntimeLimits::application_node22(),
                crate::RuntimeLimits::default(),
            ] {
                let w = NimbusRuntime::with_policy(
                    std::sync::Arc::new(RecordingHost::default()),
                    std::sync::Arc::new(RuntimePolicy::new(limits)),
                    crate::RuntimeEgressPosture::CoarsePermissions,
                );
                w.bootstrap_snapshot()
                    .expect("startup snapshot should build");
            }
        });
    }

    let (aready_tx, aready_rx) = std::sync::mpsc::channel::<()>();
    let (arelease_tx, arelease_rx) = std::sync::mpsc::channel::<()>();
    let anchor = if anchored {
        let abp = std::sync::Arc::clone(&bundle_path);
        Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("anchor runtime should build");
            rt.block_on(async move {
                let owner = NimbusRuntime::with_policy(
                    std::sync::Arc::new(RecordingHost::default()),
                    std::sync::Arc::new(RuntimePolicy::new(
                        crate::RuntimeLimits::application_node22(),
                    )),
                    crate::RuntimeEgressPosture::CoarsePermissions,
                );
                let bundle = RuntimeBundle::new(&*abp);
                let snap = owner
                    .bootstrap_snapshot()
                    .expect("cached nodefull snapshot");
                let _anchor_node = owner
                    .create_runtime_from_snapshot(&bundle, snap)
                    .expect("anchor nodefull installs superset RO heap");
                aready_tx.send(()).expect("signal anchor installed");
                recv_within(&arelease_rx, "test should release parked anchor isolate");
            })
        }))
    } else {
        None
    };
    if anchored {
        recv_within(&aready_rx, "anchor isolate should report installed");
    }

    let main_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("main runtime should build");
    main_rt.block_on(async {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let bundle = RuntimeBundle::new(&*bundle_path);
        let mut web = owner
            .create_runtime(&bundle, None, false)
            .expect("unsnapshotted weblean builds");
        let result = web.execute_script("primordial_shape", PRIMORDIAL_SHAPE_JS);
        let msg = result
            .err()
            .and_then(|e| e.message.clone())
            .expect("shape script throws to exfiltrate its message");
        let shape = msg.strip_prefix("SHAPE\n").unwrap_or(&msg).to_string();
        std::fs::write(&out, shape).expect("write shape file");
    });

    if let Some(anchor) = anchor {
        arelease_tx.send(()).expect("release anchor");
        anchor.join().expect("anchor thread should not panic");
    }
}

/// AUDIT #3 negative-capability + reachability probe: Node-only globals must be absent,
/// and a bounded BFS from globalThis (own props + prototypes, getters NOT invoked) must
/// not reach any Node-only powerful name. Throws NEGCAP-OK (with the reachable count) on
/// clean, NEGCAP-FAIL (with offenders) otherwise — both via the exfiltration channel.
const NEGCAP_JS: &str = r#"(() => {
  const fails = [];
  for (const g of ['process','Buffer','require','global','module','exports','__dirname','__filename','setImmediate']) {
    if (typeof globalThis[g] !== 'undefined') fails.push('global:' + g);
  }
  const blocklist = ['process','Buffer','require','module','child_process','spawn','spawnSync','execSync','createRequire','dlopen','binding','internalBinding','fork','readFileSync','writeFileSync','readdirSync'];
  const seen = new Set();
  const reachable = new Set();
  const queue = [{ o: globalThis, d: 0 }];
  let steps = 0;
  while (queue.length && steps < 60000) {
    steps++;
    const { o, d } = queue.pop();
    if (o === null || (typeof o !== 'object' && typeof o !== 'function')) continue;
    if (d > 3 || seen.has(o)) continue;
    seen.add(o);
    let names;
    try { names = Object.getOwnPropertyNames(o); } catch (e) { continue; }
    for (const n of names) {
      reachable.add(n);
      let desc;
      try { desc = Object.getOwnPropertyDescriptor(o, n); } catch (e) { continue; }
      if (desc && ('value' in desc)) {
        const v = desc.value;
        if (v !== null && (typeof v === 'object' || typeof v === 'function')) queue.push({ o: v, d: d + 1 });
      }
    }
    const proto = Object.getPrototypeOf(o);
    if (proto) queue.push({ o: proto, d: d + 1 });
  }
  for (const b of blocklist) if (reachable.has(b)) fails.push('reachable:' + b);
  if (fails.length) throw new Error('NEGCAP-FAIL (' + fails.length + '): ' + fails.join(', ') + ' | reachable=' + reachable.size);
  throw new Error('NEGCAP-OK | reachable=' + reachable.size + ' | steps=' + steps);
})()"#;

/// AUDIT #1 web-API correctness (behavioral, async): exercise the REAL WebStandard surface
/// in an unsnapshotted WebLean isolate riding the NodeFull anchor. Sets globalThis.__webResult.
const WEB_API_JS: &str = r#"(async () => {
  try {
    const fails = [];
    const ck = (c, m) => { if (!c) fails.push(m); };
    const enc = new TextEncoder(), dec = new TextDecoder();
    ck(dec.decode(enc.encode('hello-unicode-✓-€-😀')) === 'hello-unicode-✓-€-😀', 'textcodec');
    ck(typeof URL === 'function', 'URL-present');
    if (typeof URL === 'function') {
      const u = new URL('https://a.b:8080/p/q?x=1&y=2#frag');
      ck(u.hostname === 'a.b' && u.port === '8080' && u.pathname === '/p/q' && u.search === '?x=1&y=2' && u.hash === '#frag', 'url');
    }
    const ra = new Uint8Array(16); crypto.getRandomValues(ra); ck(ra.some(x => x !== 0), 'getRandomValues');
    ck(typeof Response === 'function', 'Response-present');
    if (typeof Response === 'function') {
      ck((await new Response('hello-body').text()) === 'hello-body', 'response.text');
      ck((await new Response(JSON.stringify({ k: 5 })).json()).k === 5, 'response.json');
    }
    ck(typeof Request === 'function', 'Request-present');
    if (typeof Request === 'function') {
      const req = new Request('https://x.y/z', { method: 'POST' });
      ck(req.method === 'POST' && req.url === 'https://x.y/z', 'request');
    }
    ck(typeof ReadableStream === 'function', 'ReadableStream-present');
    if (typeof ReadableStream === 'function') {
      const rs = new ReadableStream({ start(c) { c.enqueue('a'); c.enqueue('b'); c.close(); } });
      const rd = rs.getReader(); let acc = '';
      for (;;) { const { done, value } = await rd.read(); if (done) break; acc += value; }
      ck(acc === 'ab', 'stream');
    }
    ck(crypto.subtle && typeof crypto.subtle.digest === 'function', 'subtle-present');
    if (crypto.subtle && typeof crypto.subtle.digest === 'function') {
      const dig = await crypto.subtle.digest('SHA-256', new TextEncoder().encode('abc'));
      const hex = [...new Uint8Array(dig)].map(b => b.toString(16).padStart(2, '0')).join('');
      ck(hex === 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad', 'subtle.digest:' + hex.slice(0, 12));
    }
    globalThis.__webResult = fails.length ? ('FAIL: ' + fails.join(', ')) : 'OK';
  } catch (e) { globalThis.__webResult = 'THREW: ' + ((e && e.stack) || String(e)); }
})()"#;

fn audit_build_weblean_on_nodefull_anchor<F>(check: F)
where
    F: FnOnce(&mut JsRuntime) + Send + 'static,
{
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");
    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            let w = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            w.bootstrap_snapshot()
                .expect("nodefull snapshot should build");
        });
    }
    let (aready_tx, aready_rx) = std::sync::mpsc::channel::<()>();
    let (arelease_tx, arelease_rx) = std::sync::mpsc::channel::<()>();
    let abp = std::sync::Arc::clone(&bundle_path);
    let anchor = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("anchor runtime should build");
        rt.block_on(async move {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let bundle = RuntimeBundle::new(&*abp);
            let snap = owner
                .bootstrap_snapshot()
                .expect("cached nodefull snapshot");
            let _anchor_node = owner
                .create_runtime_from_snapshot(&bundle, snap)
                .expect("anchor nodefull installs superset RO heap");
            aready_tx.send(()).expect("signal anchor installed");
            recv_within(&arelease_rx, "test should release parked anchor isolate");
        })
    });
    recv_within(&aready_rx, "anchor isolate should report installed");
    let main_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("main runtime should build");
    main_rt.block_on(async {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let bundle = RuntimeBundle::new(&*bundle_path);
        let mut web = owner
            .create_runtime(&bundle, None, false)
            .expect("unsnapshotted weblean builds on anchor");
        check(&mut web);
    });
    arelease_tx.send(()).expect("release anchor");
    anchor.join().expect("anchor thread should not panic");
}

#[test]
#[ignore = "cage-isolated: run via isol_audit3_negcap"]
fn audit3_unsnapshotted_weblean_negative_capability_isolated() {
    audit_build_weblean_on_nodefull_anchor(|web| {
        let result = web.execute_script("negcap", NEGCAP_JS);
        let msg = result
            .err()
            .and_then(|e| e.message.clone())
            .expect("negcap script throws to exfiltrate");
        eprintln!("AUDIT3 negcap: {msg}");
        assert!(
            msg.starts_with("NEGCAP-OK"),
            "AUDIT #3 isolation FAILED — Node capability reachable in WebLean: {msg}"
        );
    });
}

#[test]
#[ignore = "cage-isolated: run via isol_audit1_web_api"]
fn audit1_unsnapshotted_weblean_web_api_correct() {
    // Scaffold matches audit_build_weblean_on_nodefull_anchor, but the check is ASYNC
    // (run_event_loop drain between scripts) and the helper's check is sync — left inline.
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");
    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            let w = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            w.bootstrap_snapshot()
                .expect("nodefull snapshot should build");
        });
    }
    let (aready_tx, aready_rx) = std::sync::mpsc::channel::<()>();
    let (arelease_tx, arelease_rx) = std::sync::mpsc::channel::<()>();
    let abp = std::sync::Arc::clone(&bundle_path);
    let anchor = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("anchor runtime should build");
        rt.block_on(async move {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let bundle = RuntimeBundle::new(&*abp);
            let snap = owner
                .bootstrap_snapshot()
                .expect("cached nodefull snapshot");
            let _anchor_node = owner
                .create_runtime_from_snapshot(&bundle, snap)
                .expect("anchor nodefull installs superset RO heap");
            aready_tx.send(()).expect("signal anchor installed");
            recv_within(&arelease_rx, "test should release parked anchor isolate");
        })
    });
    recv_within(&aready_rx, "anchor isolate should report installed");

    let main_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("main runtime should build");
    main_rt.block_on(async {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
        crate::RuntimeEgressPosture::CoarsePermissions);
        let bundle = RuntimeBundle::new(&*bundle_path);
        let mut web = owner
            .create_runtime(&bundle, None, false)
            .expect("unsnapshotted weblean builds on anchor");
        let _ = web.execute_script("web_api_kickoff", WEB_API_JS);
        web.run_event_loop(PollEventLoopOptions::default())
            .await
            .expect("web-api event loop should drain");
        let check = web.execute_script(
            "web_api_check",
            "if (globalThis.__webResult !== 'OK') throw new Error(globalThis.__webResult || 'no-result'); 'OK'",
        );
        let detail = check
            .as_ref()
            .err()
            .and_then(|e| e.message.clone())
            .unwrap_or_default();
        eprintln!("AUDIT1 web-api: {}", if check.is_ok() { "OK" } else { &detail });
        assert!(
            check.is_ok(),
            "AUDIT #1 web-API FAILED (unsnapshotted WebLean): {detail}"
        );
    });

    arelease_tx.send(()).expect("release anchor");
    anchor.join().expect("anchor thread should not panic");
}

/// Disambiguation probe (env-gated): is the missing web-API surface UNSNAPSHOTTED-specific
/// (snapshotted has it, unsnapshotted loses it = a wiring gap), or WebStandard-by-design
/// (neither has it)? NIMBUS_PROBE_SNAPSHOT=1 builds snapshotted, else unsnapshotted;
/// NIMBUS_PROBE_NODE=1 builds NodeFull, else WebStandard. Throws the present web-API set.
#[test]
#[ignore = "env-gated DEV TOOL (NIMBUS_PROBE_*), not a regression assertion; run manually"]
fn web_api_presence_probe() {
    let snapshot = std::env::var("NIMBUS_PROBE_SNAPSHOT")
        .map(|v| v == "1")
        .unwrap_or(false);
    let is_node = std::env::var("NIMBUS_PROBE_NODE")
        .map(|v| v == "1")
        .unwrap_or(false);
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");
    rt.block_on(async {
        let limits = if is_node {
            crate::RuntimeLimits::application_node22()
        } else {
            crate::RuntimeLimits::default()
        };
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(limits)),
        crate::RuntimeEgressPosture::CoarsePermissions);
        let bundle = RuntimeBundle::new(&bundle_path);
        let mut iso = if snapshot {
            let snap = owner.bootstrap_snapshot().expect("snapshot should build");
            owner
                .create_runtime_from_snapshot(&bundle, snap)
                .expect("snapshotted isolate")
        } else {
            owner.create_runtime(&bundle, None, false).expect("unsnapshotted isolate")
        };
        let probe = r#"(() => {
            const apis = ['TextEncoder','TextDecoder','URL','URLSearchParams','Response','Request','Headers','fetch','ReadableStream','WritableStream','crypto','Blob','structuredClone','queueMicrotask','setTimeout','btoa','atob'];
            const present = apis.filter(n => typeof globalThis[n] !== 'undefined');
            const subtle = (typeof crypto !== 'undefined' && crypto.subtle) ? 'crypto.subtle' : 'NO-crypto.subtle';
            throw new Error('PRESENT[' + present.length + '/' + apis.length + ']: ' + present.join(',') + ' | ' + subtle);
        })()"#;
        let msg = iso
            .execute_script("probe", probe)
            .err()
            .and_then(|e| e.message.clone())
            .unwrap_or_default();
        eprintln!(
            "PROBE profile={} snapshot={} :: {msg}",
            if is_node { "NodeFull" } else { "WebStandard" },
            snapshot
        );
    });
}

/// ANCHOR REGRESSION (i) — WebStandard-first startup is FORCED to NodeFull-first.
/// Install the anchor at "process init" (production order), then build WebStandard first
/// ("WebStandard traffic arrives first"). The anchor already installed NodeFull's superset
/// RO heap, so WebStandard rides it: no vector.h/SIGBUS AND no anchor-floor panic — proving
/// the ordering is FORCED, not that a crash was caught. Inverse of the proven
/// `weblean_installed_first_then_nodefull` crash.
#[test]
#[ignore = "cage-isolated (mutates anchor globals): run via isol_anchor_regression_i"]
fn anchor_regression_i_weblean_first_forced_nodefull_first() {
    crate::runtime::driver::anchor::install_nodefull_anchor(std::sync::Arc::new(
        RecordingHost::default(),
    ));
    assert!(
        crate::runtime::driver::anchor::anchor_installed_for_test(),
        "anchor must be installed before serving"
    );
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");
    rt.block_on(async {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let bundle = RuntimeBundle::new(&bundle_path);
        let mut web = owner
            .create_runtime(&bundle, None, false)
            .expect("weblean builds against the forced NodeFull anchor");
        let src = format!("globalThis.__isNodeProfile = false;\n{RO_INTRINSIC_CHECKS_JS}");
        let r = web.execute_script("checks", src);
        assert!(r.is_ok(), "weblean correct on anchor: {:?}", r.err());
    });
}

/// ANCHOR REGRESSION (ii) — NodeFull scale-to-zero does NOT reap the anchor.
/// Install the anchor, build+drop several NodeFull workload isolates (scale to zero), then
/// build a fresh WebStandard and a fresh NodeFull. The pinned anchor kept NodeFull's RO heap
/// installed across the drain → no crash.
#[test]
#[ignore = "cage-isolated (mutates anchor globals): run via isol_anchor_regression_ii"]
fn anchor_regression_ii_nodefull_scale_to_zero_anchor_pinned() {
    crate::runtime::driver::anchor::install_nodefull_anchor(std::sync::Arc::new(
        RecordingHost::default(),
    ));
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");
    rt.block_on(async {
        let bundle = RuntimeBundle::new(&bundle_path);
        // Scale NodeFull workloads to zero: build + drop several NodeFull isolates.
        for _ in 0..4 {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let snap = owner
                .bootstrap_snapshot()
                .expect("cached nodefull snapshot");
            let node = owner
                .create_runtime_from_snapshot(&bundle, snap)
                .expect("nodefull workload builds");
            drop(node);
        }
        // Anchor still holds NodeFull's RO heap: fresh WebStandard (rides it) + fresh
        // NodeFull (matches it) both build without crash.
        let web_owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let _web = web_owner
            .create_runtime(&bundle, None, false)
            .expect("weblean builds after nodefull scale-to-zero (anchor pinned)");
        let node_owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(
                crate::RuntimeLimits::application_node22(),
            )),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let snap = node_owner
            .bootstrap_snapshot()
            .expect("cached nodefull snapshot");
        let _node = node_owner
            .create_runtime_from_snapshot(&bundle, snap)
            .expect("fresh nodefull builds after scale-to-zero (anchor pinned)");
    });
}

/// ANCHOR REGRESSION (iii) — cross-profile refill is green WITH the structural anchor.
/// The `cross_profile_refill_into_resident_mixed_pool` regime crashed 5/12 WITHOUT an
/// anchor; installing the structural anchor at init makes it safe (the manual-anchor
/// `nodefull_anchor_first_then_cross_profile_refill` proved 12/12 — this is the same via
/// the production guard).
#[test]
#[ignore = "cage-isolated (mutates anchor globals): run via isol_anchor_regression_iii"]
fn anchor_regression_iii_cross_profile_refill_green() {
    crate::runtime::driver::anchor::install_nodefull_anchor(std::sync::Arc::new(
        RecordingHost::default(),
    ));
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");

    const SLOTS: usize = 8;
    const REFILL_ROUNDS: usize = 24;
    let mut cmd_txs = Vec::new();
    let mut done_rxs = Vec::new();
    let mut handles = Vec::new();
    for _ in 0..SLOTS {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Option<bool>>();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        let bp = std::sync::Arc::clone(&bundle_path);
        let handle = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("slot runtime should build");
            rt.block_on(async move {
                let bundle = RuntimeBundle::new(&*bp);
                let mut current: Option<(NimbusRuntime, JsRuntime)> = None;
                while let Some(cmd) = recv_until_disconnected(
                    &cmd_rx,
                    "anchor regression slot should receive a command",
                ) {
                    match cmd {
                        Some(is_node) => {
                            drop(current.take());
                            let limits = if is_node {
                                crate::RuntimeLimits::application_node22()
                            } else {
                                crate::RuntimeLimits::default()
                            };
                            let owner = NimbusRuntime::with_policy(
                                std::sync::Arc::new(RecordingHost::default()),
                                std::sync::Arc::new(RuntimePolicy::new(limits)),
                                crate::RuntimeEgressPosture::CoarsePermissions,
                            );
                            let iso = if is_node {
                                let snap = owner
                                    .bootstrap_snapshot()
                                    .expect("cached nodefull snapshot");
                                owner
                                    .create_runtime_from_snapshot(&bundle, snap)
                                    .expect("nodefull isolate should build")
                            } else {
                                owner
                                    .create_runtime(&bundle, None, false)
                                    .expect("weblean isolate should build")
                            };
                            current = Some((owner, iso));
                            done_tx.send(()).expect("slot done signal");
                        }
                        None => break,
                    }
                }
                drop(current.take());
            });
        });
        cmd_txs.push(cmd_tx);
        done_rxs.push(done_rx);
        handles.push(handle);
    }
    let mut slot_is_node = [false; SLOTS];
    for i in 0..SLOTS {
        slot_is_node[i] = i % 2 == 0;
        cmd_txs[i]
            .send(Some(slot_is_node[i]))
            .expect("send initial build");
    }
    for rx in &done_rxs {
        recv_within(rx, "initial anchor regression build should complete");
    }
    for round in 0..REFILL_ROUNDS {
        let j = round % SLOTS;
        slot_is_node[j] = !slot_is_node[j];
        cmd_txs[j]
            .send(Some(slot_is_node[j]))
            .expect("send refill build");
        recv_within(&done_rxs[j], "anchor regression refill should complete");
    }
    for tx in &cmd_txs {
        let _ = tx.send(None);
    }
    for h in handles {
        h.join().expect("slot thread should not panic");
    }
}

/// ANCHOR REGRESSION (1B) — the fail-closed FLOOR actually FIRES. Arm the anchor system
/// (ANCHOR_ENABLED) WITHOUT installing the anchor, then attempt a WebStandard build: it MUST
/// panic at the floor. Proves the regression-catch is LIVE, not permanently dormant (a guard
/// that never catches the regression it exists for would be discovered only in production).
#[test]
#[ignore = "cage-isolated (mutates anchor globals): run via isol_anchor_floor_fires"]
fn anchor_floor_fires_when_armed_but_not_installed() {
    crate::runtime::driver::anchor::arm_floor_without_install_for_test();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    // Suppress the panic print so the caught violation doesn't pollute stderr/classification.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        rt.block_on(async {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let bundle = RuntimeBundle::new(&bundle_path);
            let _ = owner.create_runtime(&bundle, None, false); // must panic at the floor
        });
    }));
    std::panic::set_hook(prev_hook);
    assert!(
        result.is_err(),
        "anchor floor MUST panic on a non-anchor build when armed but not installed"
    );
    let payload = result.err().unwrap();
    let msg = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert!(
        msg.contains("ANCHOR INVARIANT VIOLATED"),
        "the panic must be the anchor floor; got: {msg}"
    );
}

/// ANCHOR (1A) — backend creation ARMS + GATES on install. Creating the V8 runtime backend
/// (worker/server startup) must BLOCK until the NodeFull RO-heap anchor is installed, so the
/// pool can't fill or serve before NodeFull-first is guaranteed. Proves the fix is actually
/// armed in production, not just in tests.
#[test]
#[ignore = "cage-isolated (mutates anchor globals): run via isol_anchor_armed_and_gated"]
fn anchor_armed_and_gated_at_v8_backend_creation() {
    use crate::backends::RuntimeBackendFactory;
    assert!(
        !crate::runtime::driver::anchor::anchor_installed_for_test(),
        "anchor must not be installed before backend creation"
    );
    let _backend = crate::backends::v8::V8RuntimeBackendFactory.create();
    // create() returned → the anchor MUST already be installed (it blocked on install).
    assert!(
        crate::runtime::driver::anchor::anchor_installed_for_test(),
        "V8 backend creation must BLOCK until the anchor is installed (serving gated on install)"
    );
}

/// ANCHOR (item A) — is the NodeFull bootstrap HOST-CALL-FREE during construction? Build a
/// NodeFull isolate (the anchor's exact build) with a host that COUNTS calls. The count MUST
/// be 0: the production anchor's `AnchorNoopHost` fails LOUD (panics) if invoked, so a
/// host-call during construction would crash startup. This test makes the host-call-free
/// property a hard, asserted invariant rather than a measured observation.
#[test]
#[ignore = "cage-isolated: run via isol_anchor_host_call_count"]
fn anchor_nodefull_build_host_call_count() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static BUILD_HOST_CALLS: AtomicUsize = AtomicUsize::new(0);
    #[derive(Debug)]
    struct CountingHost;
    impl crate::host::HostBridge for CountingHost {
        fn call(
            &self,
            _request: crate::host::HostCallRequest,
        ) -> crate::error::Result<serde_json::Value> {
            BUILD_HOST_CALLS.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::Value::Null)
        }
    }
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};\n").expect("bundle should write");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");
    rt.block_on(async {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(CountingHost),
            std::sync::Arc::new(RuntimePolicy::new(
                crate::RuntimeLimits::application_node22(),
            )),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let bundle = RuntimeBundle::new(&bundle_path);
        let snap = owner
            .bootstrap_snapshot()
            .expect("nodefull snapshot should build");
        let _node = owner
            .create_runtime_from_snapshot(&bundle, snap)
            .expect("nodefull anchor build should succeed");
    });
    let calls = BUILD_HOST_CALLS.load(Ordering::SeqCst);
    eprintln!("ANCHOR-A: NodeFull bootstrap host calls during construction = {calls}");
    // The anchor's production host (AnchorNoopHost) fails LOUD if invoked, so the
    // host-call-free property is now load-bearing, not merely measured: assert it.
    assert_eq!(
        calls, 0,
        "NodeFull anchor construction made {calls} host call(s); the anchor's fail-loud \
         host would PANIC in production. Construction must stay host-call-free."
    );
}

/// AUDIT #1b — fetch is PRESENT and DENY-BY-DEFAULT (presence != capability). Unsnapshotted
/// WebStandard on the anchor: `fetch` must be a function AND reject for PERMISSION when there
/// is no net grant — not absent, and not silently succeeding. Locks the presence-only fix:
/// the binding exists so the EXISTING permission path has something to gate.
#[test]
#[ignore = "cage-isolated: run via isol_audit1b_fetch_deny"]
fn audit1b_weblean_fetch_present_and_deny_by_default() {
    // Scaffold matches audit_build_weblean_on_nodefull_anchor, but the check is ASYNC (fetch +
    // run_event_loop) and the helper's check is sync — left inline.
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = std::sync::Arc::new(tempdir.path().join("bundle.mjs"));
    std::fs::write(
        &*bundle_path,
        "globalThis.__nimbusInvoke = function () { return { ok: true }; };\nexport {};\n",
    )
    .expect("bundle should write");
    {
        let warmup_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("warmup runtime should build");
        warmup_rt.block_on(async {
            let w = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            w.bootstrap_snapshot()
                .expect("nodefull snapshot should build");
        });
    }
    let (aready_tx, aready_rx) = std::sync::mpsc::channel::<()>();
    let (arelease_tx, arelease_rx) = std::sync::mpsc::channel::<()>();
    let abp = std::sync::Arc::clone(&bundle_path);
    let anchor = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("anchor runtime should build");
        rt.block_on(async move {
            let owner = NimbusRuntime::with_policy(
                std::sync::Arc::new(RecordingHost::default()),
                std::sync::Arc::new(RuntimePolicy::new(
                    crate::RuntimeLimits::application_node22(),
                )),
                crate::RuntimeEgressPosture::CoarsePermissions,
            );
            let bundle = RuntimeBundle::new(&*abp);
            let snap = owner
                .bootstrap_snapshot()
                .expect("cached nodefull snapshot");
            let _anchor = owner
                .create_runtime_from_snapshot(&bundle, snap)
                .expect("anchor installs RO heap");
            aready_tx.send(()).expect("signal anchor");
            recv_within(&arelease_rx, "test should release parked anchor isolate");
        })
    });
    recv_within(&aready_rx, "anchor isolate should report ready");
    let main_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("main runtime should build");
    main_rt.block_on(async {
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(crate::RuntimeLimits::default())),
        crate::RuntimeEgressPosture::CoarsePermissions);
        let bundle = RuntimeBundle::new(&*bundle_path);
        let mut web = owner
            .create_runtime(&bundle, None, false)
            .expect("unsnapshotted weblean builds on anchor");
        let probe = r#"(async () => {
            if (typeof fetch !== 'function') { globalThis.__fetchProbe = 'ABSENT'; return; }
            try {
                const r = await fetch('http://example.com/deny-by-default-probe');
                globalThis.__fetchProbe = 'SUCCEEDED-' + r.status;
            } catch (e) {
                globalThis.__fetchProbe = 'REJECTED|' + String((e && e.name) || '') + '|' + String((e && e.message) || e).slice(0, 120);
            }
        })()"#;
        let _ = web.execute_script("fetch_probe", probe);
        web.run_event_loop(PollEventLoopOptions::default())
            .await
            .ok();
        let read = web.execute_script(
            "read",
            "throw new Error(globalThis.__fetchProbe || 'NO-RESULT');",
        );
        let msg = read
            .err()
            .and_then(|e| e.message.clone())
            .unwrap_or_default();
        eprintln!("AUDIT1b FETCH-PROBE: {msg}");
        assert!(!msg.contains("ABSENT"), "fetch must be PRESENT, got: {msg}");
        assert!(
            msg.contains("REJECTED"),
            "fetch must DENY-BY-DEFAULT (reject without a net grant), got: {msg}"
        );
        assert!(
            regex_like_permission(&msg),
            "fetch rejection must be a PERMISSION denial, not a parse/other error: {msg}"
        );
    });
    arelease_tx.send(()).expect("release anchor");
    anchor.join().expect("anchor thread should not panic");
}

fn regex_like_permission(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("permission")
        || m.contains("net access")
        || m.contains("notcapable")
        || m.contains("denied")
        || m.contains("allow-net")
        || m.contains("requires")
        || m.contains("not allowed")
}

#[tokio::test]
#[ignore = "cage-isolated (pre-existing): run via its isol_ parent (own process)"]
async fn pooled_runtime_invocations_keep_module_state_fresh() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__moduleLoadCount = (globalThis.__moduleLoadCount ?? 0) + 1;

globalThis.__nimbusInvoke = async function () {
  return { moduleLoadCount: globalThis.__moduleLoadCount };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        policy,
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first = invoke_on_single_worker(&executor, runtime.clone(), &bundle, request.clone())
        .await
        .expect("first pooled invocation should succeed");
    let second = invoke_on_single_worker(&executor, runtime, &bundle, request)
        .await
        .expect("second pooled invocation should succeed");

    assert_eq!(first, serde_json::json!({ "moduleLoadCount": 1 }));
    assert_eq!(second, serde_json::json!({ "moduleLoadCount": 1 }));
    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
}

#[tokio::test]
#[ignore = "cage-isolated (pre-existing): run via its isol_ parent (own process)"]
async fn pooled_runtime_invocations_reset_auth_and_host_call_session_state() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  const user = await ctx.auth.getUserIdentity();
  const host = await ctx.db.get("messages", "doc-1");
  return {
    token: user?.tokenIdentifier ?? null,
    session: host.payload.host_call_session_id,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        policy,
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let bundle = RuntimeBundle::new(&bundle_path);

    let first = invoke_on_single_worker(
        &executor,
        runtime.clone(),
        &bundle,
        InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "auth:first".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: Some(test_invocation_auth("token-1")),
            services: Default::default(),
        },
    )
    .await
    .expect("first pooled invocation should succeed");
    let second = invoke_on_single_worker(
        &executor,
        runtime,
        &bundle,
        InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "auth:second".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: Some(test_invocation_auth("token-2")),
            services: Default::default(),
        },
    )
    .await
    .expect("second pooled invocation should succeed");

    assert_eq!(
        first,
        serde_json::json!({
            "token": "token-1",
            "session": "query:auth:first",
        })
    );
    assert_eq!(
        second,
        serde_json::json!({
            "token": "token-2",
            "session": "query:auth:second",
        })
    );
    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
    assert_eq!(metrics.runtime_pool_replacements, 0);
}

#[derive(Clone)]
struct TaggedAsyncDbGetHost {
    host_id: &'static str,
}

impl HostBridge for TaggedAsyncDbGetHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "sync host bridge path should not be used for async ops".to_string(),
        ))
    }

    fn call_async(
        &self,
        _request: HostCallRequest,
        _cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let host_id = self.host_id;
        Box::pin(async move {
            Ok(serde_json::json!({
                "status": "ok",
                "value": {
                    "host_id": host_id,
                },
            }))
        })
    }
}

impl crate::EgressGateway for TaggedAsyncDbGetHost {
    fn authorize(&self, _request: &crate::EgressRequest) -> crate::EgressAuthorization {
        crate::EgressAuthorization::allow("tagged warm-pool test host")
    }
}

#[tokio::test]
#[ignore = "cage-isolated (pre-existing): run via its isol_ parent (own process)"]
async fn warm_pooled_runtime_rebinds_host_bridge_per_invocation() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:get".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first_host = Arc::new(TaggedAsyncDbGetHost { host_id: "first" });
    let first_host_weak = Arc::downgrade(&first_host);
    let first = invoke_on_single_worker(
        &executor,
        NimbusRuntime::with_policy(
            first_host.clone(),
            policy.clone(),
            crate::RuntimeEgressPosture::Gateway(first_host.clone()),
        ),
        &bundle,
        request.clone(),
    )
    .await
    .expect("first warm pooled invocation should succeed");
    drop(first_host);
    assert!(
        first_host_weak.upgrade().is_none(),
        "a retained warm runtime must release the completed invocation's host and egress bindings"
    );

    let second_host = Arc::new(TaggedAsyncDbGetHost { host_id: "second" });
    let second_host_weak = Arc::downgrade(&second_host);
    let second = invoke_on_single_worker(
        &executor,
        NimbusRuntime::with_policy(
            second_host.clone(),
            policy,
            crate::RuntimeEgressPosture::Gateway(second_host.clone()),
        ),
        &bundle,
        request,
    )
    .await
    .expect("second warm pooled invocation should succeed");
    drop(second_host);
    assert!(
        second_host_weak.upgrade().is_none(),
        "returning the rebound runtime must release the second invocation's bindings too"
    );

    assert_eq!(first, serde_json::json!({ "host_id": "first" }));
    assert_eq!(second, serde_json::json!({ "host_id": "second" }));
    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
}

#[tokio::test]
#[ignore = "cage-isolated (pre-existing): run via its isol_ parent (own process)"]
async fn reused_runtime_refreshes_invocation_cancellation_state_before_next_invoke() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
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
        function_name: "messages:get".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant_for_test(&request, "tenant-a");
    let runtime_instance = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let mut runtime = v8_runtime_pool
        .take_runtime(&runtime_instance, &bundle)
        .expect("runtime should build from snapshot")
        .runtime;
    runtime_instance
        .load_bundle(&mut runtime, &bundle)
        .await
        .expect("bundle should load");

    let previous_cancel_handle = {
        let op_state = runtime.op_state();
        let state = op_state.borrow();
        let cancellation_state = state.borrow::<RuntimeCancellationState>();
        cancellation_state.signal.cancel();
        assert!(
            cancellation_state.signal.is_cancelled(),
            "test should poison the previous invocation state"
        );
        cancellation_state.cancel_handle.clone()
    };

    let watchdog = WatchdogTimer::new();
    let mut permit =
        SharedInvocationPermit::new(runtime_instance.policy(), None, None, false, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");

    let mut driver = runtime_instance
        .prepare_runtime_invocation_driver(RuntimeInvocationDriverPrepare {
            runtime: ReusableV8Runtime::fresh(runtime, V8RuntimeConstructionMode::StartupSnapshot),
            watchdog: watchdog.clone(),
            external_cancellation: None,
            permit: permit.clone(),
            context: &context,
            execution_plan: None,
            record_replacement_on_error: false,
            activity_signal: None,
        })
        .expect("driver preparation should reset invocation state");

    {
        let op_state = driver.runtime.op_state();
        let state = op_state.borrow();
        let cancellation_state = state.borrow::<RuntimeCancellationState>();
        assert!(
            !cancellation_state.signal.is_cancelled(),
            "fresh invocation state should not inherit the previous cancelled signal"
        );
        assert!(
            !Rc::ptr_eq(&previous_cancel_handle, &cancellation_state.cancel_handle),
            "fresh invocation state should replace the previous cancel handle"
        );
    }

    let result = runtime_instance
        .invoke_loaded_bundle(&mut driver.runtime, &request)
        .await
        .expect("fresh invocation state should allow async host work to complete");
    let result = driver
        .finalize(Ok(result))
        .await
        .expect("result should finalize");
    let ready_jobs = permit.finish_invocation().await;

    assert!(ready_jobs.is_empty());
    assert_eq!(
        result,
        serde_json::json!({
            "operation": "document_get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "host_call_session_id": "query:messages:get",
            },
        })
    );
    watchdog.shutdown();
}

#[tokio::test]
#[ignore = "cage-isolated (pre-existing): run via its isol_ parent (own process)"]
async fn reused_runtime_uses_bound_host_call_session_before_next_invoke() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:get".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant_for_test(&request, "tenant-a");
    let runtime_instance = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let mut runtime = v8_runtime_pool
        .take_runtime(&runtime_instance, &bundle)
        .expect("runtime should build from snapshot")
        .runtime;
    let mut permit = SharedInvocationPermit::new(runtime_instance.policy(), None, None, true, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");
    bootstrap::reset_runtime_invocation_state(&mut runtime, permit.clone(), Some(&context), None);

    async fn issue_default_context_get(runtime: &mut JsRuntime) -> Value {
        let value = runtime
            .execute_script(
                "<nimbus-runtime:test-default-context-get>",
                r#"(async () => {
  const ctx = globalThis.__nimbusCreateContext();
  return await ctx.db.get("messages", "doc-1");
})()"#,
            )
            .expect("test script should execute");
        let resolve = runtime.resolve(value);
        let value = runtime
            .with_event_loop_promise(resolve, PollEventLoopOptions::default())
            .await
            .expect("promise should resolve");
        deserialize_json_value(runtime, value).expect("result should deserialize")
    }

    let first = issue_default_context_get(&mut runtime).await;
    let second_without_reset = issue_default_context_get(&mut runtime).await;

    bootstrap::reset_bootstrap_invocation_state(&mut runtime)
        .expect("bootstrap reset should succeed on reused runtime");

    let third_after_reset = issue_default_context_get(&mut runtime).await;

    assert_eq!(
        first,
        serde_json::json!({
            "operation": "document_get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "host_call_session_id": "query:messages:get",
            },
        })
    );
    assert_eq!(
        second_without_reset,
        serde_json::json!({
            "operation": "document_get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "host_call_session_id": "query:messages:get",
            },
        })
    );
    assert_eq!(
        third_after_reset,
        serde_json::json!({
            "operation": "document_get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "host_call_session_id": "query:messages:get",
            },
        })
    );
}

// ============================================================================================
// deno_core::shared_ro_heap_serialize_lock GATE TESTS.
// The fork patch acquires this lock inside InnerIsolateState::drop. These exercise the REAL
// Drop path (not just the type) for the two properties the patch turns on.
// ============================================================================================

/// SELF-DEADLOCK: the isolate Drop re-acquires the serialize lock. In production, construction.rs
/// holds that lock across the whole construction body, so a failed construction dropping its
/// partial runtime re-enters the lock on the SAME thread. A non-reentrant lock would self-deadlock
/// teardown. This holds the lock, builds a runtime (which re-acquires it), then drops the runtime
/// (whose isolate Drop re-acquires it) — all on one thread with the lock held throughout.
#[test]
fn ro_heap_serialize_lock_isolate_drop_while_held_does_not_self_deadlock() {
    run_v8_sensitive_runtime_test_in_subprocess(IsolatedRuntimeTestCase::new(
        "runtime-ro-heap-serialize-lock-drop-while-held",
        "pool-reuse",
        "isolate Drop re-enters the shared RO-heap serialize lock without self-deadlock",
        "runtime::tests::pool_reuse::ro_heap_serialize_lock_isolate_drop_while_held_does_not_self_deadlock_subprocess",
    ));
}

#[test]
#[ignore = "runs in a subprocess to isolate shared RO-heap/V8 teardown state"]
fn ro_heap_serialize_lock_isolate_drop_while_held_does_not_self_deadlock_subprocess() {
    let _outer = deno_core::shared_ro_heap_serialize_lock().lock();
    let owner = NimbusRuntime::with_policy(
        std::sync::Arc::new(RecordingHost::default()),
        std::sync::Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let snap = owner
        .bootstrap_snapshot()
        .expect("nodefull snapshot builds");
    let bundle = RuntimeBundle::virtual_anchor();
    // create_runtime_from_snapshot re-acquires the lock (construction.rs) while _outer is held.
    let runtime = owner
        .create_runtime_from_snapshot(&bundle, snap)
        .expect("nodefull runtime builds while the serialize lock is held (reentrant create)");
    // The isolate Drop re-acquires the lock while _outer is STILL held. Non-reentrant => deadlock.
    drop(runtime);
    // Reaching here proves the in-Drop re-acquire did not self-deadlock against the held lock.
}

/// PANIC-IN-DROP: a panic that unwinds through a held serialize lock AND a live isolate must tear
/// down cleanly. The in-Drop `.lock()` is a parking_lot ReentrantMutex (NON-poisoning), so it
/// cannot panic on poison; a panicking destructor mid-unwind would otherwise abort the process.
/// This drives the real Drop path DURING unwind and confirms the panic is CAUGHT, not aborted.
#[test]
fn ro_heap_serialize_lock_isolate_drop_during_unwind_does_not_abort() {
    run_v8_sensitive_runtime_test_in_subprocess(IsolatedRuntimeTestCase::new(
        "runtime-ro-heap-serialize-lock-drop-during-unwind",
        "pool-reuse",
        "isolate Drop during unwind re-enters the shared RO-heap serialize lock without aborting",
        "runtime::tests::pool_reuse::ro_heap_serialize_lock_isolate_drop_during_unwind_does_not_abort_subprocess",
    ));
}

#[test]
#[ignore = "runs in a subprocess to isolate shared RO-heap/V8 teardown state"]
fn ro_heap_serialize_lock_isolate_drop_during_unwind_does_not_abort_subprocess() {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _outer = deno_core::shared_ro_heap_serialize_lock().lock();
        let owner = NimbusRuntime::with_policy(
            std::sync::Arc::new(RecordingHost::default()),
            std::sync::Arc::new(RuntimePolicy::new(
                crate::RuntimeLimits::application_node22(),
            )),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let snap = owner
            .bootstrap_snapshot()
            .expect("nodefull snapshot builds");
        let bundle = RuntimeBundle::virtual_anchor();
        let _runtime = owner
            .create_runtime_from_snapshot(&bundle, snap)
            .expect("nodefull runtime builds");
        // Unwinding from here drops _runtime (isolate Drop re-acquires the lock mid-unwind), then
        // _outer. If that in-Drop acquire panicked, the double-panic would abort the process.
        panic!("simulated mid-construction failure while holding the RO-heap serialize lock");
    }));
    assert!(
        result.is_err(),
        "panic must unwind and tear the isolate down cleanly, not abort the process"
    );
}

/// Guard for the embedded NodeFull(Node22) anchor snapshot. A freshly built blob round-trips (build
/// -> serialize -> deserialize) into a usable, cage-correct NodeFull runtime. The eager release gate
/// validates source content while the build checkout exists. The serving path validates the second,
/// runtime-safe provenance header without reopening build-only source paths, and refuses a corrupt
/// or truncated portable header. The blob is built with this build's provenance (which under
/// `cfg(test)` includes the test-only extension), so the match path is non-vacuous.
#[tokio::test(flavor = "current_thread")]
async fn embedded_node22_snapshot_roundtrips_and_guard_is_fail_safe() {
    let blob = crate::backends::v8::build_embeddable_node22_snapshot_blob()
        .expect("build embeddable node22 blob");

    // GUARD MATCH PATH: fresh blob -> provenance matches -> deserialize (the serving path).
    let snapshot = crate::backends::v8::try_embedded_node22_anchor_snapshot(&blob)
        .expect("fresh embedded blob must pass the provenance guard and deserialize");

    // A source checkout still validates content when the build-only files are present. An in-place
    // source edit or a corrupt content header must not silently reuse a stale snapshot.
    let mut content_header_changed = blob.clone();
    content_header_changed[0] ^= 0xff;
    assert!(
        crate::backends::v8::try_embedded_node22_anchor_snapshot(&content_header_changed).is_none(),
        "source-checkout validation must reject stale content provenance"
    );

    // Corrupt the portable provenance header. The serving guard must refuse it and never install a
    // snapshot whose runtime-safe identity does not match the current binary.
    let mut stale = blob.clone();
    stale[8] ^= 0xff;
    assert!(
        crate::backends::v8::try_embedded_node22_anchor_snapshot(&stale).is_none(),
        "stale portable provenance must fail the serving guard"
    );
    // A truncated portable header must also fail the guard, not panic.
    assert!(
        crate::backends::v8::try_embedded_node22_anchor_snapshot(&blob[..12]).is_none(),
        "truncated blob MUST fail the guard"
    );

    // Cage-critical: constructing FROM the guarded embedded snapshot must succeed (a mismatched RO
    // heap aborts during deserialize), and a builtin must work (the NodeFull RO heap installed).
    let owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        Arc::new(RuntimePolicy::new(
            crate::RuntimeLimits::application_node22(),
        )),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let bundle = RuntimeBundle::virtual_anchor();
    let mut runtime = owner
        .create_runtime_from_snapshot(&bundle, &snapshot)
        .expect("construct NodeFull runtime FROM EMBEDDED snapshot");
    let probe = runtime
        .execute_script("<embedded-snapshot-smoke>", "typeof globalThis.Object")
        .expect("smoke script executes on embedded snapshot");
    let value = deserialize_json_value(&mut runtime, probe).expect("probe deserializes");
    assert_eq!(
        value,
        serde_json::json!("function"),
        "embedded snapshot must install a working RO heap (globalThis.Object)"
    );
}
