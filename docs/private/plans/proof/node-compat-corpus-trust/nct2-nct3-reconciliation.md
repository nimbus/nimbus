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

---

# NCT5 proof: the unexpected-pass loop was decorative, and now it is not

## The key format made detection impossible

`rust-watchpoints.json` keys every entry on the bare Rust function name:

```
"test_name": "node22_process_env_delete_application_preset_watchpoint"
```

`std::thread::current().name()` returns the full module path:

```
runtime::tests::node_compat::node22_process_env_delete_application_preset_watchpoint
```

`detect_unexpected_passes` matches on equality, so the first version of the
observed-results writer would have matched nothing and reported success on
every run. The writer now emits the bare name as `test_name` and keeps the full
path as `rust_test_path`.

`node_compat_observed_results_append_one_json_line_per_fixture` asserts that
`test_name` holds no `::`, so this cannot regress silently.

## The cataloged watchpoints never ran

The 150 cataloged watchpoints carry `#[ignore]`, so the corpus command never
executed them. Even with correct keys, the observed results could not contain
one. The corpus job now measures them with `--run-ignored only` in the same
partition.

That step treats a test result as data and an unexpected exit code as a
failure: nextest returns 0 when every test passed and 100 when at least one
failed, and both are valid measurements. Any other status stops the job. The
gate is `watchpoints.py validate --observed-results` in the next job.

## First real measurement found a stale catalog entry

```
cargo nextest run -p nimbus-runtime --lib \
  -E 'test(node22_process_env_delete_application_preset_watchpoint)' \
  --run-ignored only
Summary [3.904s] 1 test run: 1 passed, 1329 skipped
```

The catalog records that fixture as `expected_failure`. It passes. Feeding the
real measurement through the pipeline reports it:

```
make node-compat-validate-watchpoints OBSERVED_RESULTS=.../observed-wp.json
error: {"action": "remove_ignore_and_promote_or_reclassify_expectation",
        "classification": "watchpoint", "expectation": "expected_failure",
        "kind": "unexpected_pass", "outcome": "passed",
        "test_name": "node22_process_env_delete_application_preset_watchpoint"}
```

This is the first unexpected pass the repository has ever detected. The check
existed and reported success for as long as it has been in the workflow,
because nothing gave it data.

Expect the first instrumented run to report more of these. Each one is an
`#[ignore]` to remove or an expectation to reclassify, and NCT4 owns the list.

## Checks

| check | result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo clippy -p nimbus-runtime --lib --tests` | no warnings |
| 8 unit tests | pass |
| `actionlint` on both workflows | clean |
| `make node-compat-baseline-verify` | ok, 0 recorded gaps |
| `bash scripts/check-docs.sh` | PASS, 110 pages |
| guard rejects a required-surface entry | proven |
| guard rejects a non-vendored fixture | proven |
| guard rejects a duplicate, an unordered entry, an empty reason | proven |
| guard rejects a missing lane | proven |
| `refresh` refuses a partial run | proven |
| `aggregate` merged 219 attempts into 136 fixtures | proven |
