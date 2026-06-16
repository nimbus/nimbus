// NDS3 cycle-13 wave-3 support-file staging promotions.
//
// `test/parallel/test-vm-module-evaluate-while-evaluating.js` was previously
// excluded from the loader-context/vm runnable batch via
// `LOADER_CONTEXT_VM_FATAL_ABORT_PATHS` / `_PREFIXES` (it aborted under an
// older fork). Against the cycle-13 fork it now evaluates cleanly on both
// lanes: an adversarial one-process-per-lane probe reported passed=1,
// skipped=0, failed=0 with `test/common`, `test/fixtures/es-modules`, and
// `test/fixtures/keys` staged. It is a genuine v8_isolate_required gap on both
// node22 and node24, so promoting it drops each lane's gap count by one with no
// fork edit required.
//
// The #[test] calls run_node_compat_watchpoint_path_batch_with_lane_extra_dirs
// directly (not through a wrapper) so the classifier's static execution-marker
// scan attributes the fixture to each lane, and the batch helper re-executes it
// and fails on any assertion mismatch or skip-to-empty -- the dynamic
// green-guard that keeps the promotion honest.

const NDS3_CYCLE13_W3_VM_EVAL_PATHS: &[&str] =
    &["test/parallel/test-vm-module-evaluate-while-evaluating.js"];

const NDS3_CYCLE13_W3_VM_EVAL_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures/es-modules", "test/fixtures/keys"];

#[test]
fn node22_supported_lane_executes_cycle13_w3_vm_eval_batch() {
    let fixture_paths = NDS3_CYCLE13_W3_VM_EVAL_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle13-w3-vm-eval-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE13_W3_VM_EVAL_EXTRA_DIRS,
    );
}

#[test]
fn node24_supported_lane_executes_cycle13_w3_vm_eval_batch() {
    let fixture_paths = NDS3_CYCLE13_W3_VM_EVAL_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-supported-lane-executes-cycle13-w3-vm-eval-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE13_W3_VM_EVAL_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_cycle13_w3_vm_eval_batch() {
    let fixture_paths = NDS3_CYCLE13_W3_VM_EVAL_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-cycle13-w3-vm-eval-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        NDS3_CYCLE13_W3_VM_EVAL_EXTRA_DIRS,
    );
}

// `test/parallel/test-webcrypto-sign-verify.js` (node22) was held back: a
// per-fixture probe against the pinned `v2.8.2-nimbus.26` fork (the tag CI
// builds) shows it still fails even with the cycle-13 fork edits applied, so
// promoting it now would CI-red. It is deferred to a future fork-tag cycle
// alongside `test-assert-deep.js`.
