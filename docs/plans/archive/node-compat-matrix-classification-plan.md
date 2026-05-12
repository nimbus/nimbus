# Node Compatibility Matrix Classification Plan

Status: `done`

Owner: Node-compatible runtime / test infrastructure

This plan owns the post-evidence-hardening wave after the completed Node LTS
compatibility closeout. It starts from checkpoint commit `17a6bf48`, where the
suite-wide status/dashboard/trend/publish workflows are already in place.

## Objective

Drive the Node22 lane from `994 / 1283` documented-green vendored test files
toward full classified coverage by moving long-tail fixture inventory out of
historical Rust-only tables where practical, then classifying every remaining
Node22 vendored `test-*` file as one of:

- manifest-owned green coverage
- expected failure with an owner-backed runtime/platform reason
- expected skip with an owner-backed non-contract reason
- precise gap assigned to a future roadmap owner

## Guardrails

- Do not weaken tests, delete assertions, or convert failures into green claims.
- Do not treat prose counts as sufficient when a machine-owned file inventory is
  practical.
- Keep public support claims tied to generated status, dashboard, trend, and
  published evidence artifacts.
- Preserve the completed NLC baselines; new Node-compat roadmap work belongs in
  this plan until it is archived.
- Keep `node_compat.rs` as the execution engine and fixture-harness owner, not
  the long-term inventory database.

## Starting Baseline

- Node20 validation lane: `913 / 1308` documented green, `395`
  unmanifested/unclassified.
- Node22 primary lane: `994 / 1283` documented green, `289`
  unmanifested/unclassified.
- Node24 preview lane: `925 / 1495` documented green, `570`
  unmanifested/unclassified.
- The first inventory audit shows the current Node22 dashboard numerator is
  evidence-backed by family docs, but the exact green file list is not yet fully
  reconstructable from manifest-owned data. Closing that source-of-truth gap is
  the first batch before adding more coverage.

## Checkpoints

| ID | Status | Exit criteria |
| --- | --- | --- |
| NCM1 Inventory truth source | `done` | `make node-compat-inventory LANE=node22` emits JSON/Markdown showing vendored denominator, documented green count, Rust-referenced fixture paths, unreferenced candidates, and any reconstructability warnings. |
| NCM2 Manifest-owned matrix seam | `done` | Long-tail fixture batches that are practical to data-own are moved from inline historical Rust tables into manifest/generated inventory, with tests proving the generated inventory matches the execution set. |
| NCM3 First Node22 classification batch | `done` | A small owner-coherent Node22 unclassified group is classified as green/expected-failure/skip/gap, with status/dashboard/trend deltas updated and focused runtime evidence recorded. |
| NCM4 Iterative coverage batches | `done` | Repeat NCM3 in family-sized batches until the Node22 unclassified count is zero or every remaining item has a precise owner-backed classification. |
| NCM5 Closeout audit | `done` | Final review confirms docs, manifests, generated inventories, dashboards, and runtime tests agree; archive this plan only after the evidence is reproducible from make targets. |

## Verification Gates

- `make node-compat-inventory LANE=node22`
- `make node-compat-status`
- `make node-compat-dashboard`
- `make node-compat-trends`
- `make node-compat-publish-evidence`
- focused `cargo test -p nimbus-runtime ... -- --nocapture --test-threads=1`
  for each promoted batch
- `cargo fmt --all --check`
- `git diff --check`

## Progress Log

- `2026-05-12`: Started from checkpoint commit `17a6bf48` after completing the
  Node compatibility evidence-hardening baseline. First focus is making the
  Node22 unclassified denominator and Rust-vs-manifest source-of-truth gap
  machine-readable.
- `2026-05-12`: Completed NCM1. Added `make node-compat-inventory`, a
  schema-validated Node22 fixture inventory report, dashboard integration, and
  refresh orchestration. The first generated report preserves the public
  `994 / 1283` documented-green status while exposing a `93`-file
  documented-green reconstructability gap and `382` Rust-unreferenced candidate
  paths, which makes NCM2 the correct next root-cause slice.
- `2026-05-12`: Completed NCM3 first batch. Added a lane classification catalog
  and classified four zero-byte, non-official Node22 vendored placeholders as
  `expected_skip` without increasing the green numerator. Node22 now reports
  `994` documented green, `4` classified non-green, and `285`
  unmanifested/unclassified vendored files.
- `2026-05-12`: Continued NCM4 with the first host-harness classification
  slice. Classified eleven official pseudo-TTY fixtures as
  `requires_pseudo_tty_host_harness` / `expected_gap`, owned by the
  process/TTY host seam. Node22 now reports `994` documented green, `15`
  classified non-green, and `274` unmanifested/unclassified vendored files.
- `2026-05-12`: Continued NCM4 with denominator-cleanliness and host-boundary
  classifications. Classified three `test/fixtures/**` support files as
  non-runnable support fixtures, `test-process-abort-exitcode.js` as a
  process-abort host gap, and the non-Node-context addon timerify fixture as a
  native-addon host gap. Node22 now reports `994` documented green, `20`
  classified non-green, and `269` unmanifested/unclassified vendored files.
- `2026-05-12`: Continued NCM4 with the `test/async-hooks` directory slice.
  Classified six provider-accounting fixtures for FSEVENTWRAP, FSREQCALLBACK,
  timers, and TTYWRAP as async-hooks native resource-accounting gaps. Node22
  now reports `994` documented green, `26` classified non-green, and `263`
  unmanifested/unclassified vendored files.
- `2026-05-12`: Continued NCM4 with the remaining small non-parallel
  directory tail: upstream `known_issues`, WPT fixtures, pummel stress tests,
  and sequential host-state tests. These are now explicit expected gaps/skips
  with owners for WPT harness, pummel stress, sequential host state, known
  issues, and platform boundaries. Node22 now reports `994` documented green,
  `54` classified non-green, and `235` unmanifested/unclassified vendored
  files.
- `2026-05-12`: Continued NCM4 with the remaining diagnostics-channel
  instrumentation family. Classified loader, HTTP/net/UDP/process, worker,
  leak, async-context, and TracingChannel follow-ons as precise
  diagnostics-channel completion gaps. Node22 now reports `994` documented
  green, `80` classified non-green, and `209` unmanifested/unclassified
  vendored files.
- `2026-05-12`: Advanced NCM2/NCM4 by adding grouped lane-classification
  entries so large owner-coherent batches can live as manifest data instead of
  repeated one-row JSON. Used that grouped shape for the 80-file process
  long-tail host-surface batch covering process state, identity, environment,
  exit/abort, execve/dlopen, active-resource accounting, warnings, and signal
  behavior. Node22 now reports `994` documented green, `160` classified
  non-green, and `129` unmanifested/unclassified vendored files.
- `2026-05-12`: Completed NCM4 status classification closeout. Classified the
  fs long-tail host-I/O batch, timer scheduler tail, and final small
  host/standards/runtime tail. The Node22 suite status now reports `994`
  documented green, `289` classified non-green, and `0`
  unmanifested/unclassified vendored files. The separate inventory report still
  exposes the `93`-file documented-green reconstructability gap, so NCM2 remains
  active.
- `2026-05-12`: Hardened NCM2 evidence after the status closeout. The inventory
  now separates denominator status from path reconstructability: Node22 reports
  `1283 / 1283` documented-or-classified status coverage, while the path audit
  still exposes `286` Rust-unreferenced classified non-green files, `96`
  Rust-unreferenced unclassified files, `3` classified non-green files that are
  Rust-referenced, and the `93` documented-green reconstructability gap.
- `2026-05-12`: Completed the NCM2 source-of-truth correction. The Node22 green
  numerator now uses reconstructable lane-local path evidence instead of the
  older prose family sum when those disagree: `898` path-owned green files plus
  `385` explicit non-green classifications equals `1283 / 1283` classified
  vendored files. The generated inventory now includes the path-owned green
  test list, reports `0` Rust-unreferenced unclassified files, and has `0`
  reconstructability warnings. Public docs were corrected from the earlier
  `994` prose claim to the path-owned `898` support claim.
- `2026-05-12`: Completed NCM5 closeout. Re-ran the status, inventory,
  dashboard, trend, publish, schema-validation, `cargo fmt --all --check`, and
  focused runtime manifest-topology verification gates. The checked-in evidence
  snapshot reports zero suite warnings and keeps the support claim tied to
  path-owned Node22 green evidence plus explicit non-green classifications.
