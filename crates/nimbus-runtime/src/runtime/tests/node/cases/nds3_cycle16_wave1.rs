// NDS3 cycle-16 wave-1: domain / setUncaughtExceptionCaptureCallback
// mutual-exclusion fork-fix promotion (fork v2.8.2-nimbus.29).
//
// `test/parallel/test-domain-load-after-set-uncaught-exception-capture.js`
// exercises the mutual exclusion between the `domain` module and a registered
// process.setUncaughtExceptionCaptureCallback(). Through Node 24, requiring
// `domain` while a callback is set throws ERR_DOMAIN_CALLBACK_NOT_AVAILABLE;
// Node 25+ removed the restriction so the two coexist. The fork now enforces
// this version-gated at the user require() of node:domain inside Module._load
// (the .29 fork edit), so the node20/node22/node24 lanes get the throw and the
// node26 lane gets coexistence -- the fixture passes on all four lanes.
//
// Confirmed by the cycle-16 single-probe harness (one process per lane,
// passed=1/skipped=0/failed=0) and re-confirmed by the enforced batches below
// on the repinned published v2.8.2-nimbus.29 baseline (4 passed / 0 failed).
// The batch helper re-executes the fixture and fails on any assertion mismatch
// or skip-to-empty -- the dynamic green-guard that keeps the promotion honest.
// node22/node24 are the v8-isolate-required gate lanes; node20/node26 are
// promoted for the same fix.
//
// The reverse direction
// (test-domain-set-uncaught-exception-capture-after-load.js) additionally
// requires the 40-dash stack-trace decoration Node attaches when domain is in
// use, and is deferred to a future fork-tag cycle.

const NDS3_CYCLE16_DOMAIN_PATHS: &[&str] =
    &["test/parallel/test-domain-load-after-set-uncaught-exception-capture.js"];

const NDS3_CYCLE16_DOMAIN_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node20_legacy_lane_executes_cycle16_domain_batch() {
    let fixture_paths = NDS3_CYCLE16_DOMAIN_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node20-legacy-lane-executes-cycle16-domain-batch",
        NodeCompatLane::Node20,
        &fixture_paths,
        &[],
        NDS3_CYCLE16_DOMAIN_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle16_domain_batch() {
    let fixture_paths = NDS3_CYCLE16_DOMAIN_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle16-domain-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE16_DOMAIN_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle16_domain_batch() {
    let fixture_paths = NDS3_CYCLE16_DOMAIN_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle16-domain-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE16_DOMAIN_EXTRA_DIRS,
    );
}

#[test]
fn node26_current_lane_executes_cycle16_domain_batch() {
    let fixture_paths = NDS3_CYCLE16_DOMAIN_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node26-current-lane-executes-cycle16-domain-batch",
        NodeCompatLane::Node26,
        &fixture_paths,
        &[],
        NDS3_CYCLE16_DOMAIN_EXTRA_DIRS,
    );
}
