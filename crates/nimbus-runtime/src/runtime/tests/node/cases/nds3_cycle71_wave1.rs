const NDS3_CYCLE71_WEBCRYPTO_PROMISE_PROTOTYPE_POLLUTION_PATHS: &[&str] =
    &["test/parallel/test-webcrypto-promise-prototype-pollution.mjs"];

const NDS3_CYCLE71_WEBCRYPTO_PROMISE_PROTOTYPE_POLLUTION_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures/crypto",
    "test/fixtures/keys",
    "test/fixtures/webcrypto",
];

#[test]
fn node24_default_lane_executes_cycle71_webcrypto_promise_prototype_pollution_batch() {
    let fixture_paths = NDS3_CYCLE71_WEBCRYPTO_PROMISE_PROTOTYPE_POLLUTION_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle71-webcrypto-promise-prototype-pollution-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE71_WEBCRYPTO_PROMISE_PROTOTYPE_POLLUTION_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_cycle71_webcrypto_promise_prototype_pollution_batch() {
    let fixture_paths = NDS3_CYCLE71_WEBCRYPTO_PROMISE_PROTOTYPE_POLLUTION_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-cycle71-webcrypto-promise-prototype-pollution-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        NDS3_CYCLE71_WEBCRYPTO_PROMISE_PROTOTYPE_POLLUTION_EXTRA_DIRS,
    );
}
