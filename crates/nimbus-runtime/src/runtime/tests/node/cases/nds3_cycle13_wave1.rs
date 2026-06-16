// NDS3 cycle-13 wave-1 fork-fix promotions.
//
// These official fixtures green via real dynamic green-guard execution after the
// cycle-13 wave-1 deno-fork polyfill batch (plus its init-crash repair: the
// `process.domain = null` module-load default moved from domain.ts -- which is
// evaluated into the startup snapshot before the `process` global exists -- to
// process.ts where the singleton is in scope). The batch touched async_hooks /
// AsyncResource bind, _events, the readable/pipeline/destroy stream paths, the
// web message_port + event brand checks, webcrypto AEAD detached-buffer decode,
// the vm synthetic-module path, fs WriteStream flush/autoclose/eagain, and the
// v8 one-byte-string representation helper.
//
// Every pair below was confirmed by the isolated cycle-13 single-probe harness
// running one process per fixture (passed=1, skipped=0, failed=0 per lane), then
// re-confirmed by the enforced batches in this file (node22 7/7, node24 14/14,
// all skipped=0/failed=0). The batch helper re-executes each fixture and fails
// the test on any assertion mismatch or skip-to-empty, so this is the dynamic
// green-guard that keeps the promotion honest. node22 is the supported lane;
// node24 is the default lane.
//
// `test/parallel/test-assert-deep.js` (node24) was held back: a per-fixture
// probe against the pinned `v2.8.2-nimbus.26` fork (the tag CI builds) shows it
// still fails the deep-equal matrix even with the cycle-13 fork edits applied,
// so promoting it now would CI-red. It is deferred to a future fork-tag cycle
// alongside `test-webcrypto-sign-verify.js`.
//
// Each #[test] calls run_node_compat_watchpoint_path_batch_with_lane_extra_dirs
// directly (not through a wrapper) so the classifier's static execution-marker
// scan and per-test lane inference attribute these fixtures to the right lane.
//
// Staging groups (extra_dirs):
//   * common  -> test/common
//   * ahooks  -> test/common + test/async-hooks
//   * wcrypto -> test/common + test/fixtures/webcrypto + test/fixtures/crypto

const NDS3_CYCLE13_W1_COMMON_NODE22_PATHS: &[&str] = &[
    "test/parallel/test-asyncresource-bind.js",
    "test/parallel/test-fs-write-stream-eagain.mjs",
    "test/parallel/test-fs-write-stream-flush.js",
    "test/parallel/test-messageevent-brandcheck.js",
    "test/parallel/test-v8-string-is-one-byte-representation.js",
    "test/parallel/test-vm-module-synthetic.js",
];

const NDS3_CYCLE13_W1_COMMON_NODE24_PATHS: &[&str] = &[
    "test/parallel/test-asyncresource-bind.js",
    "test/parallel/test-eventemitter-asyncresource.js",
    "test/parallel/test-fs-write-stream-autoclose-option.js",
    "test/parallel/test-fs-write-stream-eagain.mjs",
    "test/parallel/test-fs-write-stream-flush.js",
    "test/parallel/test-messageevent-brandcheck.js",
    "test/parallel/test-stream-destroy.js",
    "test/parallel/test-stream-pipeline.js",
    "test/parallel/test-stream-readable-to-web-termination-byob.js",
    "test/parallel/test-v8-string-is-one-byte-representation.js",
    "test/parallel/test-vm-module-synthetic.js",
    "test/parallel/test-whatwg-webstreams-transform-stream-members.js",
];

const NDS3_CYCLE13_W1_AHOOKS_PATHS: &[&str] =
    &["test/parallel/test-async-hooks-http-parser-destroy.js"];

const NDS3_CYCLE13_W1_WCRYPTO_NODE24_PATHS: &[&str] =
    &["test/parallel/test-webcrypto-aead-decrypt-detached-buffer.js"];

const NDS3_CYCLE13_W1_COMMON_EXTRA_DIRS: &[&str] = &["test/common"];
const NDS3_CYCLE13_W1_AHOOKS_EXTRA_DIRS: &[&str] = &["test/common", "test/async-hooks"];
const NDS3_CYCLE13_W1_WCRYPTO_EXTRA_DIRS: &[&str] = &[
    "test/common",
    "test/fixtures/webcrypto",
    "test/fixtures/crypto",
];

#[test]
fn node22_supported_lane_executes_cycle13_w1_common_batch() {
    let fixture_paths = NDS3_CYCLE13_W1_COMMON_NODE22_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle13-w1-common-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE13_W1_COMMON_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle13_w1_common_batch() {
    let fixture_paths = NDS3_CYCLE13_W1_COMMON_NODE24_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle13-w1-common-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE13_W1_COMMON_EXTRA_DIRS,
    );
}

#[test]
fn node22_supported_lane_executes_cycle13_w1_ahooks_batch() {
    let fixture_paths = NDS3_CYCLE13_W1_AHOOKS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle13-w1-ahooks-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE13_W1_AHOOKS_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle13_w1_ahooks_batch() {
    let fixture_paths = NDS3_CYCLE13_W1_AHOOKS_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle13-w1-ahooks-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE13_W1_AHOOKS_EXTRA_DIRS,
    );
}

#[test]
fn node24_default_lane_executes_cycle13_w1_wcrypto_batch() {
    let fixture_paths = NDS3_CYCLE13_W1_WCRYPTO_NODE24_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node24-default-lane-executes-cycle13-w1-wcrypto-batch",
        NodeCompatLane::Node24,
        &fixture_paths,
        &[],
        NDS3_CYCLE13_W1_WCRYPTO_EXTRA_DIRS,
    );
}
