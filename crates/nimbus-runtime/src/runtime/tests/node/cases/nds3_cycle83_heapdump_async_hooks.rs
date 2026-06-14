// NDS3 cycle-83 heap snapshot async_hooks Promise lifecycle promotion.
//
// Deno fork tag v2.8.3-nimbus.32 hides native parentless implementation
// promises from user async_hooks while keeping real user-created nested
// promises visible, dynamically greening this fixture in both required lanes.

const NDS3_CYCLE83_HEAPDUMP_ASYNC_HOOKS_PATHS: &[&str] =
    &["test/parallel/test-heapdump-async-hooks-init-promise.js"];

const NDS3_CYCLE83_HEAPDUMP_ASYNC_HOOKS_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle83_heapdump_async_hooks_batch() {
    let fixture_paths = NDS3_CYCLE83_HEAPDUMP_ASYNC_HOOKS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle83-heapdump-async-hooks-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE83_HEAPDUMP_ASYNC_HOOKS_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle83_heapdump_async_hooks_batch() {
    let fixture_paths = NDS3_CYCLE83_HEAPDUMP_ASYNC_HOOKS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle83-heapdump-async-hooks-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE83_HEAPDUMP_ASYNC_HOOKS_EXTRA_DIRS,
    );
}
