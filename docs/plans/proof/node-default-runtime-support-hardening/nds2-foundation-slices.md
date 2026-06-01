# NDS2 Foundation Slices Proof

status: done
date: 2026-06-01
branch: codex/node-default-runtime-support-hardening
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening
pr: https://github.com/nimbus/nimbus/pull/10
verifier: scripts/verify-node-default-runtime-support-hardening.sh

## Row And Status

NDS2 is done. The five canonical foundation slices were run broadly across
Node22 and Node24 before fixes, the current failures were classified and fixed,
and the same broad slice set was rerun green.

This row made the runtime contract lane-aware for `process.features`, matched
Node's user-facing `process.features` descriptor shape, and fixed Node object
identity metadata exposed through `Symbol.toStringTag`. It did not add silent
quarantine, ignored fixture escapes, fake-success stubs, or ambient host
process privileges.

## Broad Pre-Run

Commands:

```console
make node-compat-report FAMILY=core-semantics SLICE=assert-and-buffer-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/baseline
make node-compat-report FAMILY=process-and-timing SLICE=process-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/baseline
make node-compat-report FAMILY=streams-and-local-io SLICE=os-tty-readline-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/baseline
make node-compat-report FAMILY=networking SLICE=dns-net-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/baseline
make node-compat-report FAMILY=loader-context SLICE=module-and-async-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/baseline
```

Baseline Node22/Node24 slice results:

| Family | Slice | Node22 | Node24 |
| --- | --- | --- | --- |
| `core-semantics` | `assert-and-buffer-foundation` | `9 passed / 1 failed / 0 skipped / 0 missing` | `9 passed / 1 failed / 0 skipped / 0 missing` |
| `process-and-timing` | `process-foundation` | `9 passed / 1 failed / 0 skipped / 0 missing` | `9 passed / 1 failed / 0 skipped / 0 missing` |
| `streams-and-local-io` | `os-tty-readline-foundation` | `10 passed / 0 failed / 0 skipped / 0 missing` | `10 passed / 0 failed / 0 skipped / 0 missing` |
| `networking` | `dns-net-foundation` | `10 passed / 0 failed / 0 skipped / 0 missing` | `9 passed / 0 failed / 0 skipped / 0 missing` |
| `loader-context` | `module-and-async-foundation` | `10 passed / 0 failed / 0 skipped / 0 missing` | `10 passed / 0 failed / 0 skipped / 0 missing` |

Baseline failure diagnostics emitted:

| Fixture | Lane | Artifact |
| --- | --- | --- |
| `test/parallel/test-assert-checktag.js` | `node22` | `target/node-compat-nds2/baseline/diagnostics/general/node22__test_parallel_test_assert_checktag_js.json` |
| `test/parallel/test-assert-checktag.js` | `node24` | `target/node-compat-nds2/baseline/diagnostics/general/node24__test_parallel_test_assert_checktag_js.json` |
| `test/parallel/test-process-features.js` | `node22` | `target/node-compat-nds2/baseline/diagnostics/subprocess/node22__test_parallel_test_process_features_js.json` |
| `test/parallel/test-process-features.js` | `node24` | `target/node-compat-nds2/baseline/diagnostics/subprocess/node24__test_parallel_test_process_features_js.json` |

The Node24 `process-and-timing:process-foundation`
`test/parallel/test-process-features.js` failure was the explicit NDS2
watchpoint. Its baseline diagnostic showed `process.features` missing
`openssl_is_boringssl` and `quic` from the expected Node24 key set.

## Failure Grouping

Current NDS2 failures:

| Fixture | Lanes | Classification | Root cause | Resolution |
| --- | --- | --- | --- | --- |
| `test/parallel/test-assert-checktag.js` | `node22`, `node24` | `bootstrap-shim` | Nimbus' embedded global and wrapped process object lacked Node's non-enumerable `Symbol.toStringTag` identity metadata, so Node's assert fixture treated a fake global as loosely deep-equal to `globalThis`. | Define `globalThis[Symbol.toStringTag] = "global"` and `process[Symbol.toStringTag] = "process"` in the bootstrap/runtime contract. |
| `test/parallel/test-process-features.js` | `node22`, `node24` | `bootstrap-shim` | The Node runtime contract exposed a single flattened `process.features` shape instead of the lane-specific key set expected by each official fixture line. Node22 currently expects `openssl_is_boringssl`; Node24 expects `openssl_is_boringssl` and `quic`. | Build a lane-aware, enumerable `process.features` object from the runtime contract, preserve key presence separately from boolean support values, and expose the user-facing `process.features` property as non-configurable like Node. |

NCG loader-context fixture fidelity:

The archived NCG plan recorded
`loader-context:module-and-async-foundation` as `6 passed / 4 failed` across
Node20/Node22/Node24 in run `26328664800`, then explicitly said NCG2 would name
the same four failing fixtures from local JSON reports. No NCG2 proof artifact
is checked in. NDS2 therefore does not fabricate those four historical names.
Instead, it records the current local JSON input for this row: the current
`target/node-compat-nds2/baseline/loader-context/module-and-async-foundation/`
reports name zero current failing loader-context fixtures on Node22 and Node24.

The 10 `loader-context:module-and-async-foundation` fixtures from NCG are:

| Fixture | Current Node22/Node24 status | Classification |
| --- | --- | --- |
| `test/parallel/test-module-builtin.js` | passing | `runtime-op` already resolved by prior module builtin surface work |
| `test/parallel/test-module-cache.js` | passing | `runtime-op` already resolved by prior CommonJS cache surface work |
| `test/parallel/test-module-children.js` | passing | `runtime-op` already resolved by prior CommonJS graph surface work |
| `test/parallel/test-module-create-require.js` | passing | `runtime-op` already resolved by prior module bridge work |
| `test/parallel/test-module-create-require-multibyte.js` | passing | `runtime-op` already resolved by prior module bridge work |
| `test/parallel/test-module-isBuiltin.js` | passing | `runtime-op` already resolved by prior builtin classification work |
| `test/parallel/test-module-loading-deprecated.js` | passing | `runtime-op` already resolved by prior loader behavior work |
| `test/parallel/test-module-nodemodulepaths.js` | passing | `runtime-op` already resolved by prior resolution path work |
| `test/parallel/test-module-relative-lookup.js` | passing | `runtime-op` already resolved by prior resolution path work |
| `test/parallel/test-module-version.js` | passing | `runtime-op` already resolved by prior module metadata work |

No NDS2 fixture used `fork-bump` or `explicit-divergence`. Those
classifications remain part of the required taxonomy for future rows, but this
row resolved the current failures in Nimbus-owned bootstrap/runtime-contract
code.

## Focused Work

Focused reruns after the fixes:

```console
make node-compat-report FAMILY=core-semantics SLICE=assert-and-buffer-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/focused
make node-compat-report FAMILY=process-and-timing SLICE=process-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/focused
make node-compat-report FAMILY=process-and-timing SLICE=process-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/descriptor-audit
```

Observed focused results:

| Family | Slice | Node22 | Node24 |
| --- | --- | --- | --- |
| `core-semantics` | `assert-and-buffer-foundation` | `10 passed / 0 failed / 0 skipped / 0 missing` | `10 passed / 0 failed / 0 skipped / 0 missing` |
| `process-and-timing` | `process-foundation` | `10 passed / 0 failed / 0 skipped / 0 missing` | `10 passed / 0 failed / 0 skipped / 0 missing` |

The `descriptor-audit` focused rerun repeated
`process-and-timing:process-foundation` after tightening the
`process.features` property descriptor and observed the same green result:
Node22 `10 passed / 0 failed / 0 skipped / 0 missing`, Node24
`10 passed / 0 failed / 0 skipped / 0 missing`.

Files changed:

- `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`
- `crates/nimbus-runtime/src/runtime/bootstrap/source.rs`

## Broad Final Rerun

Commands:

```console
make node-compat-report FAMILY=core-semantics SLICE=assert-and-buffer-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/final-after-descriptor-audit
make node-compat-report FAMILY=process-and-timing SLICE=process-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/final-after-descriptor-audit
make node-compat-report FAMILY=streams-and-local-io SLICE=os-tty-readline-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/final-after-descriptor-audit
make node-compat-report FAMILY=networking SLICE=dns-net-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/final-after-descriptor-audit
make node-compat-report FAMILY=loader-context SLICE=module-and-async-foundation CAPTURE_LIVE=1 OUTPUT_ROOT=target/node-compat-nds2/final-after-descriptor-audit
```

Final Node22/Node24 slice results:

| Family | Slice | Node22 | Node24 |
| --- | --- | --- | --- |
| `core-semantics` | `assert-and-buffer-foundation` | `10 passed / 0 failed / 0 skipped / 0 missing` | `10 passed / 0 failed / 0 skipped / 0 missing` |
| `process-and-timing` | `process-foundation` | `10 passed / 0 failed / 0 skipped / 0 missing` | `10 passed / 0 failed / 0 skipped / 0 missing` |
| `streams-and-local-io` | `os-tty-readline-foundation` | `10 passed / 0 failed / 0 skipped / 0 missing` | `10 passed / 0 failed / 0 skipped / 0 missing` |
| `networking` | `dns-net-foundation` | `10 passed / 0 failed / 0 skipped / 0 missing` | `9 passed / 0 failed / 0 skipped / 0 missing` |
| `loader-context` | `module-and-async-foundation` | `10 passed / 0 failed / 0 skipped / 0 missing` | `10 passed / 0 failed / 0 skipped / 0 missing` |

The final broad rerun after the descriptor audit is green for all five
canonical foundation slices on Node22 and Node24. The Node24 networking slice
has nine expected fixtures in the current manifest, so
`9 passed / 0 failed / 0 skipped / 0 missing` is a green result rather than a
missing-fixture result. No final diagnostics directory was emitted under
`target/node-compat-nds2/final-after-descriptor-audit/`.

## Evidence Links

- `target/node-compat-nds2/baseline/core-semantics/assert-and-buffer-foundation/slice-observed-core-semantics-assert-and-buffer-foundation.json`
- `target/node-compat-nds2/baseline/process-and-timing/process-foundation/slice-observed-process-and-timing-process-foundation.json`
- `target/node-compat-nds2/baseline/streams-and-local-io/os-tty-readline-foundation/slice-observed-streams-and-local-io-os-tty-readline-foundation.json`
- `target/node-compat-nds2/baseline/networking/dns-net-foundation/slice-observed-networking-dns-net-foundation.json`
- `target/node-compat-nds2/baseline/loader-context/module-and-async-foundation/slice-observed-loader-context-module-and-async-foundation.json`
- `target/node-compat-nds2/focused/core-semantics/assert-and-buffer-foundation/slice-observed-core-semantics-assert-and-buffer-foundation.json`
- `target/node-compat-nds2/focused/process-and-timing/process-foundation/slice-observed-process-and-timing-process-foundation.json`
- `target/node-compat-nds2/descriptor-audit/process-and-timing/process-foundation/slice-observed-process-and-timing-process-foundation.json`
- `target/node-compat-nds2/final-after-descriptor-audit/core-semantics/assert-and-buffer-foundation/slice-observed-core-semantics-assert-and-buffer-foundation.json`
- `target/node-compat-nds2/final-after-descriptor-audit/process-and-timing/process-foundation/slice-observed-process-and-timing-process-foundation.json`
- `target/node-compat-nds2/final-after-descriptor-audit/streams-and-local-io/os-tty-readline-foundation/slice-observed-streams-and-local-io-os-tty-readline-foundation.json`
- `target/node-compat-nds2/final-after-descriptor-audit/networking/dns-net-foundation/slice-observed-networking-dns-net-foundation.json`
- `target/node-compat-nds2/final-after-descriptor-audit/loader-context/module-and-async-foundation/slice-observed-loader-context-module-and-async-foundation.json`
- `docs/plans/archive/node-compat-cron-greening-plan.md`

## Residual Risks

- The four historical NCG loader-context failure names were not preserved in a
  checked-in NCG2 proof or current local JSON report. NDS2 records that absence
  explicitly instead of guessing.
- `process.features` now models lane-specific key presence, but the values
  still truthfully reflect the underlying runtime/fork support. For example,
  adding the Node24 `quic` key is not a claim that QUIC socket behavior works in
  the V8 isolate runtime.
- NDS2 only closes the canonical foundation slices. Broader Node24 and Node22
  corpus promotion remains NDS3 scope and must keep the same wide-then-focused
  loop.
