// NDS3 wave-25 support-file staging green-guards.
//
// This file promotes only fixtures that produce a real dynamic green-guard
// PASS. It is NON-`#[ignore]`: the test actually runs the fixture and asserts it
// passes, which is the honesty contract for promoting a gap out of the
// required-surface catalog. The `test/common/{index.js,fixtures.js,tmpdir.js}`
// helpers are auto-staged by the bundle writer.
//
// Staging-census result (2026-06-09): a 17-fixture wave-25 staging candidate set
// was authored and dynamically exercised one fixture per isolate. Only
// test-esm-import-attributes-errors.js (node22) passed. The other 16 each failed
// for a genuine runtime reason, NOT a missing support file — so they are not
// staging false-negatives and are deferred to fork-fix / runtime-semantics
// waves. Recorded failure modes (source-confirmed, for the next fork cycle):
//
//   * webcrypto sign/verify (rsa node22+node24, hmac node22): verify rejects
//     with message `Key algorithm mismatch`; upstream expects
//     `/Unable to use this key to verify/`. deno_crypto verify-path message
//     parity — a single shared fix likely clears the whole sign-verify cluster.
//   * webcrypto sign/verify (eddsa node22): importKey throws
//     `NotSupportedError: Unrecognized algorithm name` for Ed25519/Ed448 in
//     deno_crypto `normalizeAlgorithm`.
//   * crypto-authenticated (node22+node24): throws `Cannot change encoding`
//     where upstream expects an error matching `/ auth/` — cipher setEncoding
//     ordering parity.
//   * webcrypto encrypt/decrypt aes (node24): native panic
//     `assertion left == right failed (15 vs 12)` in generic-array via deno
//     `ext/crypto/encrypt.rs:166` (AES-OCB tag length), which hangs the isolate.
//   * webcrypto supports (node24): `crypto.subtle.supports(...)` returns false
//     where upstream expects true — missing algorithm registrations.
//   * esm-dynamic-import-commonjs (.js + .mjs, both lanes): `assert(
//     !tickDuringCJSImport)` fails — a `process.nextTick` fires during a dynamic
//     import() of a CommonJS module. Event-loop ordering semantics.
//   * esm-dynamic-import.js (both lanes): `mustCall <anonymous> expected 1
//     actual 0` — a dynamic-import module error is not surfaced as upstream
//     expects.
//   * esm-snapshot.mjs (both lanes): after staging common/index.mjs +
//     esm-snapshot{,-mutator}.js the module resolves, but the ESM default
//     binding reflects the post-mutation value (`2 !== 1`) — the CJS export is
//     live-bound instead of snapshotted at import evaluation. ESM/CJS interop
//     semantics.

// test-esm-import-attributes-errors.js requires only the auto-staged
// `../common`; it was already promoted on node24 and observed passing on node22
// in the wave-25 gap census, then re-confirmed passing here as a dynamic
// green-guard. Promote it on node22.
const NDS3_WAVE25_ESM_IMPORT_ATTRIBUTES_ERRORS_N22_BATCH: &[NodeCompatBatchEntry] =
    &[NodeCompatBatchEntry {
        test_relative_path: "test/es-module/test-esm-import-attributes-errors.js",
        node20_fixture_source_path: None,
        node22_fixture_source_path: Some(
            "node22/test/es-module/test-esm-import-attributes-errors.js",
        ),
        node24_fixture_source_path: None,
        shared_extra_files: &[],
        node20_extra_files: &[],
        node22_extra_files: &[],
        node24_extra_files: &[],
    }];

#[test]
fn node22_nds3_wave25_esm_import_attributes_errors_batch_fixture() {
    run_node_compat_watchpoint_entry_batch(
        "node22-nds3-wave25-esm-import-attributes-errors-batch",
        NodeCompatLane::Node22,
        NDS3_WAVE25_ESM_IMPORT_ATTRIBUTES_ERRORS_N22_BATCH,
    );
}
