# NCT0 - Fail-before evidence

Date: 2026-09-16
Measured run: `35095026629`, head `f743836c6`, workflow `Node Compatibility`.

## Result

All seven jobs failed. Two independent causes.

## Cause A - the live release-train probe destroys the evidence pipeline

The `node-compat-evidence` job failed 76 seconds after start, in step 4 of 11:

```text
2026-09-16T12:19:26.1687444Z verified checked-in Node official fixture identities against upstream: 4 lanes
2026-09-16T12:19:26.4122893Z error: tag_drift: node24: dist index latest v24.21.0 != registry v24.20.0
2026-09-16T12:19:26.4123953Z error: tag_drift: node26: dist index latest v26.8.2 != registry v26.8.1
2026-09-16T12:19:26.4262745Z ##[error]Process completed with exit code 1.
```

`python3 scripts/runtime/node/release_train.py probe-live` fetches the live Node
dist index and compares it with the checked-in lane registry. It exits 1 on any
difference. Node publishes patch releases continuously, so this check goes red
on upstream's schedule, not on ours.

The probe runs before every measurement step. These steps therefore never ran:
seeded slice reports, application and tooling canaries, four oracle samples,
`node-compat-status`, `node-compat-dashboard`, `node-compat-trends`,
`node-compat-publish-evidence`, and the artifact upload. A routine upstream
patch release discards the whole night's compatibility evidence.

## Cause B - the Rust corpus lane gates on an aspirational corpus

Six `rust-corpus` partitions each exited 100, which nextest uses for test
failures.

Aggregate across the six partitions:

```text
531 tests run, 234 passed, 259 failed, 38 timed out
about 1,230 skipped per partition
294 distinct failing test names
```

Distinct failing tests by lane: node22 126, node24 119, node26 29, node20 19,
plus `node_compat_fixture_node_options_exposes_preserve_symlinks_flags`.

Parsed from the batch panic bodies: 126 batch panic headers declaring 784
failing fixtures, which resolve to 694 distinct `(lane, fixture)` pairs.

### The failures are genuine and reproduce locally

```text
cargo nextest run -p nimbus-runtime --lib \
  -E 'test(=runtime::tests::node_compat::node20_readline_promises_interface_fixture) or ...'

thread 'runtime::tests::node_compat::node20_readline_promises_interface_fixture'
panicked at crates/nimbus-runtime/src/runtime/tests/node/mod.rs:3667:35:
upstream node_compat fixture `test/parallel/test-readline-promises-interface.js`
should execute: runtime JavaScript error: AssertionError [ERR_ASSERTION]:
Mismatched function calls: Expected noop to be called exactly 1, actual 0.
```

Failure-signature counts across the run: js-assertion 1466, rust-panic 966,
js-typeerror 396, not-a-function 178, timeout 176, not-defined 104,
js-referenceerror 100, fs-authority-denied 84, other PermissionDenied 84,
err-invalid 74, module-not-found 46, cannot-find-module 20,
unknown-builtin-module 8, not-implemented 8.

These are Node API and behavior gaps, not one harness defect.

### The corpus is aspirational by design

The archived dashboard records full-corpus pass rates of node20 69.0%,
node22 21.1%, node24 19.3%, and node26 0.0%. `tests/runtime/node/CODEX-HANDOFF.md`
records that the merge gate was the `v8_isolate_required` surface, which reached
`gaps == 0` and `pass_rate_percent == 100` for node22 and node24 in PR #10.

The nightly does not use that distinction. It runs the whole
`runtime::tests::node_compat::` namespace and requires every fixture to pass.

### No expectation data reaches the Rust lane

- `tests/runtime/node/classifications/*.json` holds 45 entries in total
  (node20 20, node22 21, node24 4, node26 0). All are
  `rust_watchpoint_expected_failure`.
- `tests/runtime/node/expectations/rust-watchpoints.json` mirrors the 152
  `#[ignore]` attributes under `node/cases/`.
- Neither file is read by any Rust source. `NodeCompatExecutionClass::ExpectedFailure`
  is used only for report labels and topology assertions, never to gate execution.
- `scripts/runtime/node/watchpoints.py` implements `detect_unexpected_passes`
  behind `--observed-results`, but no producer of that file exists, and the
  nightly calls the validator without it.

The lane therefore carries no baseline. A real regression cannot be told apart
from the permanent background failure.

## History

Zero successful runs across all retained history. The archived
`node-compat-cron-greening-plan.md` records the cron as red daily since
2026-05-14 and was archived without execution.
