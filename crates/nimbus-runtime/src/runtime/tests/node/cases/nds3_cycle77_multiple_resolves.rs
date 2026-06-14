// NDS3 cycle-77 duplicate promise settle parity promotion.
//
// Deno fork tag v2.8.3-nimbus.26 preserves V8's duplicate promise settle
// callbacks through deno_core and maps them to Node's deprecated
// process "multipleResolves" event. The official fixture asserts the four
// duplicate resolve/reject notifications and their order.

const NDS3_CYCLE77_MULTIPLE_RESOLVES_PATHS: &[&str] =
    &["test/parallel/test-promise-swallowed-event.js"];
const NDS3_CYCLE77_MULTIPLE_RESOLVES_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle77_multiple_resolves_batch() {
    let fixture_paths = NDS3_CYCLE77_MULTIPLE_RESOLVES_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle77-multiple-resolves-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE77_MULTIPLE_RESOLVES_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle77_multiple_resolves_batch() {
    let fixture_paths = NDS3_CYCLE77_MULTIPLE_RESOLVES_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle77-multiple-resolves-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE77_MULTIPLE_RESOLVES_EXTRA_DIRS,
    );
}
