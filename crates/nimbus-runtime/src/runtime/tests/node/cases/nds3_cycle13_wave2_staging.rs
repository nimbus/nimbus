// NDS3 cycle-13 wave-2 support-file staging promotions.
//
// These four node22 webcrypto fixtures green via a real dynamic green-guard
// PASS against the cycle-13 wave-1 binary -- no new fork edit is required. They
// were surfaced by the wave-2 triage as cheap staging candidates and then
// adversarially probe-verified one process per fixture: each reported
// passed=1, skipped=0, failed=0 with `test/common` as the only staged support
// directory. The triage's broader 17-candidate staging/harness-postlude set
// collapsed to exactly these four under probe verification -- the other
// candidates failed on genuine behavioral assertions (process env getter/setter
// TypeError, process.getBuiltinModule reference equality, dynamic-import CJS
// nextTick ordering, tlswrap async-hook counts, https-agent protocol, intl
// v8BreakIterator) and are deferred to the fork-fix / runtime-semantics waves.
//
// The derivebits/derivekey CFRG (X25519/X448) and ECDH paths only need
// `test/common`; unlike the wave-1 AEAD detached-buffer fixture they do not
// require the webcrypto/crypto fixture trees. This is the node22 supported lane;
// these drop node22 v8_isolate_required gaps by four with no node24 movement.
//
// The #[test] calls run_node_compat_watchpoint_path_batch_with_lane_extra_dirs
// directly (not through a wrapper) so the classifier's static execution-marker
// scan attributes these fixtures to the node22 lane, and the batch helper
// re-executes each fixture and fails on any assertion mismatch or skip-to-empty
// -- the dynamic green-guard that keeps the promotion honest.

const NDS3_CYCLE13_W2_WCRYPTO_NODE22_PATHS: &[&str] = &[
    "test/parallel/test-webcrypto-derivebits-cfrg.js",
    "test/parallel/test-webcrypto-derivebits-ecdh.js",
    "test/parallel/test-webcrypto-derivekey-cfrg.js",
    "test/parallel/test-webcrypto-derivekey-ecdh.js",
];

const NDS3_CYCLE13_W2_WCRYPTO_EXTRA_DIRS: &[&str] = &["test/common"];

#[test]
fn node22_supported_lane_executes_cycle13_w2_wcrypto_batch() {
    let fixture_paths = NDS3_CYCLE13_W2_WCRYPTO_NODE22_PATHS
        .iter()
        .copied()
        .map(str::to_string)
        .collect::<Vec<_>>();
    run_node_compat_watchpoint_path_batch_with_lane_extra_dirs(
        "node22-supported-lane-executes-cycle13-w2-wcrypto-batch",
        NodeCompatLane::Node22,
        &fixture_paths,
        &[],
        NDS3_CYCLE13_W2_WCRYPTO_EXTRA_DIRS,
    );
}
