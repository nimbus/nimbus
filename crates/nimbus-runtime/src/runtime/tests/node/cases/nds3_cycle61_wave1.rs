const NDS3_CYCLE61_PUBLIC_PROMISE_HOOKS_PATHS: &[&str] = &[
    "test/parallel/test-promise-hook-create-hook.js",
    "test/parallel/test-promise-hook-exceptions.js",
    "test/parallel/test-promise-hook-on-after.js",
    "test/parallel/test-promise-hook-on-resolve.js",
];

const NDS3_CYCLE61_PUBLIC_PROMISE_HOOKS_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node24_default_lane_executes_cycle61_public_promise_hooks_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE61_PUBLIC_PROMISE_HOOKS_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle61-public-promise-hooks-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE61_PUBLIC_PROMISE_HOOKS_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle61_public_promise_hooks_batch() {
    let fixture_paths: Vec<String> = NDS3_CYCLE61_PUBLIC_PROMISE_HOOKS_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle61-public-promise-hooks-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE61_PUBLIC_PROMISE_HOOKS_EXTRA_DIRS,
    );
}
