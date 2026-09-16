# NCT2 and NCT3 proof: observed results and the reconciliation seam

Baseline: `main @ f743836c6`. Worktree: `wt-node-compat`, branch
`ci/node-compat-corpus-trust`.

## What landed

`crates/nimbus-runtime/src/runtime/tests/node/corpus_baseline.rs` owns the
recorded-expectations concept. `execute_upstream_node_compat_test_with_extra_files`
is now a thin wrapper over `..._raw` that reconciles every result. All six call
sites are unchanged, so no corpus execution can bypass the seam.

`tests/runtime/node/expectations/corpus-baseline.json` ships with four empty
lanes. The mechanism is therefore inert until NCT4 seeds it.

## Unit proof

```
cargo nextest run -p nimbus-runtime --lib \
  -E 'test(node_compat_corpus_baseline) or test(node_compat_recorded) \
      or test(node_compat_unrecorded) or test(node_compat_observed_results)'
Summary [0.032s] 8 tests run: 8 passed, 1322 skipped
```

The four policy branches each have a named test:

| recorded | observed | test | result |
| --- | --- | --- | --- |
| no | fail | `node_compat_unrecorded_failure_stays_a_regression` | lane red |
| no | pass | `node_compat_unrecorded_pass_stays_a_pass` | lane green |
| yes | fail | `node_compat_recorded_failure_keeps_the_lane_green` | lane green, `known_gap` |
| yes | pass | `node_compat_recorded_fixture_that_passes_fails_the_lane` | lane red |

## End-to-end proof on real fixtures

Real runtime execution, not a stub.

**1. An unrecorded failure stays red.** Empty baseline:

```
cargo nextest run -p nimbus-runtime --lib -E 'test(node20_readline_promises_interface_fixture)'
Summary [5.155s] 1 test run: 0 passed, 1 failed, 1329 skipped
```

Observed record:

```json
{"lane":"node20","test_relative_path":"test/parallel/test-readline-promises-interface.js",
 "test_name":"runtime::tests::node_compat::node20_readline_promises_interface_fixture",
 "outcome":"failed"}
```

**2. The same fixture, once recorded, keeps the lane green.** One node20 entry
added:

```
Summary [5.094s] 3 tests run: 3 passed, 1327 skipped
```

Observed record, same fixture, same run command:

```json
{"lane":"node20","test_relative_path":"test/parallel/test-readline-promises-interface.js",
 "test_name":"runtime::tests::node_compat::node20_readline_promises_interface_fixture",
 "outcome":"known_gap"}
```

**3. A recorded fixture that passes fails the lane.**
`test/parallel/test-crypto-padding-aes256.js` passes today. Recording it gives:

```
FAIL [4.219s] nimbus-runtime runtime::tests::node_compat::node20_loader_context_crypto_cipher_and_padding_batch_fixture
upstream node_compat fixture `test/parallel/test-crypto-padding-aes256.js` is
recorded as a known gap for lane `node20` but it passed. Remove the entry from
tests/runtime/node/expectations/corpus-baseline.json so the improvement is
recorded.
```

The message names the file to edit, so the operator does not have to find it.

Both proof entries were removed. The committed baseline has 0 entries.

## The baseline does not inflate the reported pass rate

A recorded gap keeps the Rust lane green. It does not become a pass anywhere
else:

- `run_node_compat_manifested_batch_for_lane` counts it in a separate
  `known gaps` column, never in `passed`.
- The seeded-slice mapper reports
  `NodeCompatObservedFixtureState::Fail` with detail
  `recorded corpus baseline gap`.
- The live report slice mapper increments `failed` and logs the fixture.
- `run_node_compat_watchpoint_path_batch_with_lane_extra_dirs` puts it in
  `failed_paths` for the summary artifact.

So the dashboard pass rate still measures real Node compatibility. The baseline
changes which failures stop the build, and nothing else.

## Checks

| check | result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo clippy -p nimbus-runtime --lib --tests` | no warnings |
| `cargo check -p nimbus-runtime --lib --tests` | clean |
| 8 new unit tests | pass |
| 3 end-to-end fixture scenarios | pass |

## Note on the host

The build volume was full (196 MB free of 926 GB). With the user's approval,
`target/debug/deps` (95 GB of stale Cargo artifacts) was removed from the main
tree. No source file and no modified file was touched.
