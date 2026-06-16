// NDS3 node26 cycle-36 fs-host-io residual diagnostic batch.
//
// The generic fs-host-io watchpoint intentionally filtered these low-ROI paths
// while the required-gap inventory was large. They are now the only remaining
// Node26 fs-host-io required gaps, so keep them grouped for root-cause work.

const NDS3_NODE26_CYCLE36_FS_RESIDUAL_PATHS: &[&str] = &[
    "test/parallel/test-fs-promises-watch-ignore-invalid.mjs",
    "test/parallel/test-fs-promises-watch.js",
    "test/parallel/test-fs-sir-writes-alot.js",
    "test/parallel/test-fs-stat-temporal.mjs",
    "test/parallel/test-fs-write-buffer-large.js",
];

#[test]
#[ignore = "NDS3 node26 cycle36 diagnostic: remaining fs-host-io required gaps"]
fn node26_current_lane_fs_host_io_residual_watchpoint() {
    let fixture_paths = NDS3_NODE26_CYCLE36_FS_RESIDUAL_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-fs-host-io-residual-watchpoint",
        NodeCompatLane::Node26,
        &fixture_paths,
        FS_HOST_IO_EXTRA_RUNTIME_FILES,
        FS_HOST_IO_EXTRA_DIRS,
    );
}
