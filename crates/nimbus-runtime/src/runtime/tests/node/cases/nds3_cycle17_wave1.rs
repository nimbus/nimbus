// NDS3 cycle-17 wave-1: support-file-staging free-pass promotion.
//
// The cycle-13 bulk census under-staged `test/common`, so several gate
// fixtures recorded a "Cannot find module .../test/common/index.js" staging
// artifact rather than their true dynamic outcome. The cycle-17 fresh census
// (one process per fixture, KILL-guarded, staged with test/common +
// test/fixtures/{crypto,keys}) re-ran every node22/node24 v8_isolate_required
// gap. Of the four fixtures whose Rust probe returned green, only this one is
// a real dynamic pass; the other three SELF-SKIP in the multi-tenant isolate
// and are deliberately NOT promoted here (a skip is not a pass):
//
//   test-crypto-des3-wrap.js   -> skipped: "des3-wrap cipher is not available"
//   test-fs-utimes-y2K38.js    -> skipped: "File system appears to lack Y2K38
//                                  support (touch failed)"
//   test-util-styletext.js     -> skipped: "Could not create TTY fd"
//
// The single genuine promotion:
//
//   test-webcrypto-sign-verify-rsa.js  node22 (WebCrypto RSA sign/verify) ->
//                                      passed=1, skipped=0, failed=0
//
// The promotion is gated by the dynamic green-guard in
// run_node_compat_watchpoint_path_batch_with_lane_extra_dirs: the helper
// re-executes the fixture under the lane snapshot and fails on any skip,
// assertion mismatch, or empty run, so a green test here means the fixture
// genuinely passed. Re-asserted on the repinned published v2.8.2-nimbus.29
// baseline (node22 batch: selected=1, passed=1, skipped=0, failed=0). node22
// is one of the two v8-isolate-required gate lanes; sign-verify-rsa is a
// node22-only gap.

const NDS3_CYCLE17_NODE22_PATHS: &[&str] = &["test/parallel/test-webcrypto-sign-verify-rsa.js"];

const NDS3_CYCLE17_EXTRA_DIRS: &[&str] =
    &["test/common", "test/fixtures/crypto", "test/fixtures/keys"];

#[test]
fn node22_supported_lane_executes_cycle17_staging_batch() {
    let fixture_paths = NDS3_CYCLE17_NODE22_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle17-staging-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE17_EXTRA_DIRS,
    );
}
