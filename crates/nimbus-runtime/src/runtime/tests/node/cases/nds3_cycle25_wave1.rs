// NDS3 cycle-25 wave-1: AbortController/AbortSignal parity.
// Fork fix (nimbus/deno v2.8.2-nimbus.33, ext/web): AbortSignal illegal
// construction now carries Node's ERR_ILLEGAL_CONSTRUCTOR code; timeout signals
// no longer stay strongly reachable without listeners; AbortController and
// AbortSignal private inspect output matches Node's official fixture shape.
const NDS3_CYCLE25_ABORT_CONTROLLER_PATHS: &[&str] =
    &["test/parallel/test-abortcontroller.js"];
const NDS3_CYCLE25_ABORT_CONTROLLER_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle25_abortcontroller_batch() {
    let fp = NDS3_CYCLE25_ABORT_CONTROLLER_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle25-abortcontroller-batch",
        NodeCompatLane::Node22,
        &fp,
        &[],
        NDS3_CYCLE25_ABORT_CONTROLLER_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle25_abortcontroller_batch() {
    let fp = NDS3_CYCLE25_ABORT_CONTROLLER_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle25-abortcontroller-batch",
        NodeCompatLane::Node24,
        &fp,
        &[],
        NDS3_CYCLE25_ABORT_CONTROLLER_EXTRA_DIRS,
    );
}
