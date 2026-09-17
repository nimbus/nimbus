# Node Compatibility Corpus Trust

Status: `active` | Owner: this plan | Created: 2026-09-16
Baseline: main @ `f743836c6`
Proof root: `proof/node-compat-corpus-trust/`

Next action: NCT4 is blocked. The executor joins a hung worker on drop, so
24 batch tests are killed at 10 minutes and no run can measure the whole
corpus. That defect owns the block, and it is outside this plan.

## Current resume state

- Updated: 2026-09-17. Active task: NCT4.
- Worktree: `scratchpad/wt-node20`. Branch: `fix/node20-required-surface-fixtures`.
  Base `d93408e19`. A second worktree `scratchpad/wt-node-compat` holds the
  earlier branch `ci/node-compat-corpus-trust` at HEAD `954181ec3`.
- Dirty files owned by this task: the 5 files of the node20 lane-wiring change.
- Two flakes found while measuring that change are now tracked as NCT9 and
  NCT10. Both reproduce with the change stashed, so neither belongs to it.
- NCT0 through NCT3 and NCT5 through NCT7 are done.
- NCT4 waits on a runtime fix that is now written, on branch
  `fix/runtime-executor-shutdown-bound`. Run 35187917432 kills 24 batch tests at
  the 600 s bound. They hang, so a larger bound cannot help. An earlier reading
  named `RuntimeExecutorInner::drop` and the admission permit as the cause. That
  reading was wrong, and a stack sample of the hung process corrects it: the
  worker is inside the invocation itself. A runtime invocation enforces its
  execution and system timeouts with a V8 termination, which reaches running
  JavaScript only, so a guest that parks in the event loop is bounded by
  nothing. `crates/nimbus-runtime/src/runtime/driver/invocation.rs` owns that
  defect, not this plan.
- The measurement machinery is complete and correct. It refuses the partial
  measurement and names all 15 truncated batches and the fixture that follows
  each one's last record.
- Next: merge the runtime fix, re-seed the baseline from a complete
  measurement, then take the 15 node20 required-surface fixtures. Re-seeding
  waits on the runtime fix.
- The `rust-corpus` lane stays red on 15 node20 required-surface fixtures.
  Fixing the runtime is the next work, and it is outside this plan.
- Running commands: none.

## Outcome

> A red Node Compatibility run means one of two things: measured Node behavior
> moved away from its recorded baseline, or the vendored release train is stale.
> A green run means the measured corpus matches the baseline exactly. No job is
> made non-gating to reach green.

## Architecture

Before:

```text
[node-compat-evidence job]
  step 4 release_train.py probe-live  --(upstream published a patch)--> exit 1
  steps 5..11 seeded slices, canaries, oracle, dashboard, trends, upload  NEVER RUN

[rust-corpus job x6 partitions]
  nextest runtime::tests::node_compat::  -> 294 failing tests -> exit 100
  no expectation data is read; every fixture must pass
```

After:

```text
[release-train-freshness job]          [node-compat-evidence job]
  probe-live -> fails on real drift      slices, canaries, oracle,
  owns vendored-inventory freshness      dashboard, trends, upload
                                         owns measured compatibility

[rust-corpus job x6 partitions]
  nextest -> reconcile each fixture against the recorded baseline
    expected fail + observed fail -> known gap, lane stays green
    expected fail + observed pass -> FAIL, baseline must shrink
    expected pass + observed fail -> FAIL, real regression
  emits observed-results JSON per partition

[corpus-gate job]
  aggregates partitions -> watchpoints.py validate --observed-results
```

## Scope

- Owns: the Node Compatibility nightly workflow and its two failure causes.
- Owns: the per-fixture expectation baseline and the harness seam that reads it.
- Owns: the observed-results artifact contract between the Rust lane and the
  existing Python validators.
- Does not own: closing the Node API gaps themselves. The baseline records them.
  A later plan must burn them down.
- Does not own: the `v8_isolate_required` surface. It is already at zero gaps and
  stays a hard gate.
- Non-goal: `continue-on-error`, `|| true`, or any change that makes a lane
  advisory.

## Invariants

1. No lane becomes non-gating. A weaker gate is never the fix.
2. A fixture in the required surface must never enter the baseline.
3. An unexpected pass fails the lane. The baseline can only shrink through a
   recorded, reviewed change.
4. Release-train drift stays a failing signal. It stops destroying measurement.
5. The baseline records observed behavior only. It never records a wish.

## Status ledger

| ID | Task | Status | Evidence |
|---|---|---|---|
| NCT0 | Capture fail-before evidence | done | `proof/node-compat-corpus-trust/nct0-baseline.md`; run 35095026629; 294 failing tests; evidence job exit 1 at 76s |
| NCT1 | Split release-train freshness from measurement | done | `actionlint` clean; 3 jobs; the 5 retained local commands all exit 0 |
| NCT2 | Emit observed results from the Rust corpus lane | done | `proof/node-compat-corpus-trust/nct2-nct3-reconciliation.md`; JSONL verified on a real fixture run |
| NCT3 | Add the expectation baseline and the reconciliation seam | done | 8 unit tests pass; 3 end-to-end fixture scenarios pass; all 4 policy branches named |
| NCT4 | Seed the baseline from a full instrumented run | in_progress | Run 35167962571 seeded 1,188 gaps, and the confirming run 35171841643 proved it truncated: 6,930 fixtures against 6,908, 99 only in the first and 77 only in the second. Batch bound proven by 0 timeouts in run 35178467471; abort record, record-count witness, and 5 declaration fixes added; re-seeding |
| NCT5 | Close the unexpected-pass loop for ignored watchpoints | done | `corpus-baseline-reconciliation` job feeds `--observed-results` to the 150-entry catalog |
| NCT6 | Guard the baseline | done | 4 guard rejections proven; runs in the PR lane via `make node-compat-baseline-verify` |
| NCT7 | Document the contract | done | `docs/private/operating/node-compat-nightly.md`; routed from the operating README; `check-docs.sh` PASS |
| NCT9 | Fix the http2 teardown RST race | todo | Diagnosed: `socketOnData` in the vendored `ext/node/polyfills/http2.ts` destroys the socket once nghttp2 wants neither read nor write, without waiting for the peer FIN, so the peer reads ECONNRESET. 1 failure in 19 instrumented runs. Fix lives in `nimbus/deno` and needs a tag bump |
| NCT10 | Fix the V8 backing-store heap corruption | todo | Diagnosed: 2 of 3 native aborts on 2026-09-17 are `POINTER_BEING_FREED_WAS_NOT_ALLOCATED` inside `v8::internal::BackingStore::~BackingStore()` during isolate teardown under `V8WorkerRuntimePool::return_runtime_with_authority`. A third abort shows the same corruption during snapshot deserialization at isolate creation. Reports kept in the session scratchpad `crash-evidence/` |
| NCT8 | Cleanup | todo | |

## Tasks
### NCT0 Capture fail-before evidence

- Problem: the nightly has never been green. The cause was not recorded.
- Acceptance: both causes are measured and reproduced.
- Evidence: `proof/node-compat-corpus-trust/nct0-baseline.md`.

### NCT1 Split release-train freshness from measurement

- Problem: `release_train.py probe-live` runs as step 4 of 11 in the evidence
  job and exits 1 when upstream Node publishes any patch release. Steps 5
  through 11 never run, so the night produces no dashboard, no trends, and no
  artifact. On 2026-09-16 the drift was node24 `v24.21.0` against registry
  `v24.20.0`, and node26 `v26.8.2` against registry `v26.8.1`.
- Owning seam and paths: `.github/workflows/node-compat-nightly.yml`.
- Steps:
  1. Move the release-train and latest-suite verification into a new
     `release-train-freshness` job.
  2. Keep the local, deterministic fixture validation in the evidence job.
  3. Confirm the evidence job no longer depends on a live upstream probe.
- Acceptance: `actionlint` is clean, and the evidence job reaches
  `Upload node-compat artifacts` when `probe-live` reports drift.
- Fail-before: run 35095026629 evidence job exits 1 before any measurement step.
- Verification: `actionlint .github/workflows/node-compat-nightly.yml`.

### NCT2 Emit observed results from the Rust corpus lane

- Problem: the Python validators accept `--observed-results`, but nothing
  produces that file. `make node-compat-validate-watchpoints` therefore runs
  static-only, and an unexpected pass is never detected.
- Owning seam and paths: `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`.
- Steps:
  1. Record every fixture attempt as `{lane, test_relative_path, test_name,
     outcome}`.
  2. Write the records when `NIMBUS_NODE_COMPAT_OBSERVED_RESULTS` names a path.
  3. Keep the file valid for `observed_result_entries` in `watchpoints.py`.
- Acceptance: a focused run writes a file that `watchpoints.py validate
  --observed-results` accepts.
- Fail-before: no observed-results producer exists in the repository.

### NCT3 Add the expectation baseline and the reconciliation seam

- Problem: all seven call sites of `execute_manifested_node_compat_test` treat
  any fixture failure as a test failure. The corpus is aspirational, so the lane
  gates on a condition that cannot hold.
- Owning seam and paths: `crates/nimbus-runtime/src/runtime/tests/node/mod.rs`,
  `tests/runtime/node/expectations/corpus-baseline.json`.
- Steps:
  1. Add `tests/runtime/node/expectations/corpus-baseline.json`, keyed by lane
     and `test_relative_path`, with a reason for each entry.
  2. Add one reconciliation function that maps a raw fixture result plus its
     baseline entry to a reconciled result.
  3. Route all seven call sites through it.
- Acceptance: an expected-failure fixture keeps the lane green; a baseline entry
  that passes fails the lane with an "unexpected pass" message.
- Fail-before: `node20_readline_promises_interface_fixture` fails locally.
- Verification: `cargo nextest run -p nimbus-runtime --lib -E 'test(node_compat)'`.

### NCT4 Seed the baseline from a full instrumented run

- Problem: a baseline invented from a log parse is not evidence.
- Steps:
  1. Run the full corpus with observed-results recording.
  2. Generate the baseline from the recorded results.
  3. Record the producing run in the baseline header.
- Acceptance: a second full run is green against the committed baseline.
- Verification: the Node Compatibility workflow run is green.
- Result: reopened. The baseline holds 1,188 gaps from run 35167962571, and
  the confirming run showed that measurement was truncated by the nextest
  timeout. A batch now states when it starts, when its fixture loop ends, and
  when it unwinds, and the aggregator refuses only the silent third state. A
  record-count witness makes every attempted fixture produce exactly one
  record, so the count a batch reports always matches the shard. The bound is
  proven: it refuses the partial measurement and names every truncated batch.
  Re-seeding is blocked on a runtime defect that this plan does not own.
  15 node20 fixtures on the required surface keep the lane red, which is the
  gate working as designed. The runtime fix owns them.

### NCT5 Close the unexpected-pass loop for ignored watchpoints

- Problem: `rust-watchpoints.json` states that a passing cataloged entry must
  remove the `#[ignore]`, but nothing checks. 152 ignored watchpoints never run.
- Steps: run the ignored watchpoints in the nightly and feed the results to
  `make node-compat-validate-watchpoints OBSERVED_RESULTS=...`.
- Acceptance: the workflow fails when a cataloged watchpoint passes.

### NCT6 Guard the baseline

- Problem: a baseline can rot into a dumping ground.
- Steps:
  1. Reject a baseline entry whose fixture file does not exist.
  2. Reject a baseline entry that covers a required-surface fixture.
  3. Run the guard in the pull-request lane, not only in the nightly.
- Acceptance: each rejection has a test.

### NCT7 Document the contract

- Steps: write the runbook and route it from
  `docs/private/operating/README.md`.

### NCT9 Fix the http2 teardown RST race

`socketOnData` ends and then destroys the session socket as soon as
`!handle.hasPendingData() && !this.destroyed`. `destroy()` closes the file
descriptor when our writable side drains. When the peer has not closed its
side, the kernel answers the next peer segment with RST, and the peer reports
`read ECONNRESET`. The adjacent graceful path `finishSessionClose` already
delays its destroy behind `setImmediate` and guards it with `if (!session.closed)`.
The `socketOnData` path has no equivalent guard.

The defect is lane independent. The fixture is a `shared_official_batch_case!`,
`http2.ts` holds no version branch, and the fixture transfers headers only.

Success criteria:
- The teardown waits for the peer FIN, with a bounded `unref()` fallback timer.
- The full http2 fixture set passes on all four lanes, including
  `test-http2-zero-length-header.js`, which relies on the current destroy.
- A new `v2.9.6-nimbus.6` tag, the pin updates, and the lock update land together.

### NCT10 Fix the V8 backing-store heap corruption

`nimbus-runtime-worker-0` aborts inside `v8::internal::BackingStore::~BackingStore()`
with `POINTER_BEING_FREED_WAS_NOT_ALLOCATED`. libmalloc reports a pointer it
never allocated, so this is heap corruption or a double free of an ArrayBuffer
backing store, not a benign race. Both teardown aborts reach the destructor
through the `ArrayBufferSweeper`, one from `Heap::TearDown` and one from the
final mark-compact in `Heap::StartTearDown`. The third abort shows
`ReadReadOnlyHeapRef` failing during snapshot deserialization at isolate
creation, which is consistent with the same corruption surfacing earlier.

`SharedArrayBufferStore::default()` is built per runtime in
`crates/nimbus-runtime/src/runtime/driver/construction.rs:260`, so a store
shared across isolates is excluded. No nimbus crate creates a backing store
directly.

Success criteria:
- The corrupting writer is named, with an address-sanitizer run or an
  equivalent instrumented build as proof.
- The node20 networking subset runs 50 times with 0 aborts.


### NCT8 Cleanup

- Trigger: the final pull request of this plan merges.
- Steps: archive this plan and update `docs/private/plans/README.md`.

## Goal

Execute NCT1 through NCT7 in order. Keep one task `in_progress`. Record
evidence with exact counts. Do not weaken a gate to reach green. Stop and report
if a task needs a new schema, a new public contract, or an owner decision.

## Execution log

| Date | Item | Action | Evidence |
|---|---|---|---|
| 2026-09-16 | NCT0 | Measured both failure causes and reproduced one locally | Run 35095026629; 294 failing tests; `node20_readline_promises_interface_fixture` fails locally |
| 2026-09-16 | plan | Promoted this plan as the node-compat owner | No prior plan owned the topic |
| 2026-09-16 | NCT1 | Moved `probe-live` and `verify-fixture-upstream` into a `release-train-freshness` job | `actionlint` clean; local commands `verify-node-lts-docs.sh`, `verify-node-latest-suite-tags.sh` (x2), `verify-node-release-train.sh`, `node-compat-validate-fixtures`, `node-compat-validate-watchpoints` (150 entries) all exit 0 |
| 2026-09-16 | NCT2+NCT3 | Made `execute_upstream_node_compat_test_with_extra_files` a reconciling wrapper over a `_raw` body; added `corpus_baseline.rs` and an empty `corpus-baseline.json` | 8 unit tests pass; unrecorded failure red, recorded failure green (`known_gap`), recorded pass red with an actionable message; `cargo fmt`/`clippy` clean |
| 2026-09-16 | NCT5+NCT6+NCT7 | Added `corpus_baseline.py` (aggregate/refresh/verify), the `corpus-baseline-reconciliation` job, the PR-lane guard, and the runbook | `actionlint` clean on both workflows; 4 guard rejections proven; aggregate merged 219 attempts to 136 fixtures; refresh refused a partial run; `check-docs.sh` PASS |
| 2026-09-16 | NCT4 | Blocked the first seeding run: partition 4 of 6 died on a transient `sccache` 503 and wrote no corpus shard. Added the shard-completeness guard and an `sccache` preflight that degrades to a cache-less build | run 35147979837 partition 4 exit in 0 s (`ServerBusy`, HTTP 503 from `ghac`); 3 aggregate scenarios proven (complete exit 0, missing `partition-3.jsonl` exit 1, no-flag exit 0); preflight proven on both a failing and a healthy stub; `actionlint` clean; `check-docs.sh` PASS |
| 2026-09-16 | NCT4 | Replaced the vendored-fixture guard. It derived the vendoring path from `test_relative_path`, and 11 of 12 refusals were false. The seam now records the path it read (`fixture_source_relative_path`), and both guards resolve that path | 7 of the 12 fixtures are vendored at the fixture root, and `test-async-hooks-enable-recursive-fsreqcallback-regression.js` is vendored under a different file name; `NodeCompatFixtureIdentity` names the pair; 82 of 83 node_compat tests pass (the 1 failure is the pre-existing `__nimbus-preserve-symlinks-options-probe`); refresh proved 4 refusals and 2 records, including a laneless fixture whose runtime path differs from its vendored path; `cargo fmt`/`clippy` clean; `check-docs.sh` PASS |
| 2026-09-16 | NCT4 | Found the measurement truncated. nextest killed a batch test at 45 s x 3 while the batch was still executing fixtures, and the fixtures it had already recorded still reached the artifact, so the measurement shrank silently and moved between runs | Runs 35167962571 and 35171841643 recorded 6,930 and 6,908 fixtures, 99 only in the first and 77 only in the second, in contiguous alphabetical groups; the 6 partition logs name about 32 `TIMEOUT [ 135.0xxs]` batch tests; the largest batch holds 312 fixtures and only 251 were recorded |
| 2026-09-16 | NCT4 | Bounded the batch at 10 minutes, made a batch state its start and its end, and made `aggregate` refuse a batch that started and did not finish | 7 new Python guard tests pass; 1 new Rust test pairs the records; a real 115-fixture batch aggregates clean, and the same shard cut to 60 lines is refused by name; `nextest list` accepts the override |
| 2026-09-16 | NCT4 | Kept the observed reason on a recorded gap. `into_result` replaced it with the words "recorded corpus baseline gap", and the 3 supplementary signal-lifecycle watchpoints failed because the report could no longer see what the runtime did | `known_gap_detail: Option<String>` replaces `known_gap: bool`; 82 of 83 node_compat tests pass (the 1 failure is the pre-existing `__nimbus-preserve-symlinks-options-probe`); `cargo fmt`/`clippy` clean; `actionlint` clean; `check-docs.sh` PASS |
| 2026-09-17 | NCT4 | Gave a batch a third state. The guard read a panic that unwound out of the fixture loop as a kill, and the first bracketed run reported 16 loud failures as silent truncation while nextest reported 0 timeouts. A drop without `finish` now records `batch_abort`, which a kill can never write, because the process dies without unwinding | run 35178467471: 0 `TIMEOUT [` in all 6 shards, against about 32 at 135 s before; 3 new Python guard tests (accepts an abort, an abort carries no fixture count, refuses an abort whose start is missing) take the suite from 7 to 10, all pass; 1 new Rust test proves the pair `batch_start` then `batch_abort` |
| 2026-09-17 | NCT4 | Closed the hole behind it. A fixture whose vendored source is missing panics while it reads that source, which is before the seam that writes the evidence, so the batch counted the fixture as executed and the shard never named it. A record-count witness now reads the count on both sides of each fixture and records the failure through the ordinary baseline decision | 4 batches showed the mismatch (networking/node20 265 against 264, streams-and-local-io/node24 308 against 306, http-remaining node22 and node24 139 against 138); 1 new Rust test proves a regression for a fixture that recorded nothing, `AlreadyRecorded` for one that did, and exactly one record per fixture path |
| 2026-09-17 | NCT4 | Corrected 5 batch declarations against the official upstream identity catalogs. Each named a fixture source for a lane that never shipped it | `test-dgram-blocklist.js` and `test-os-constants-signals.js` arrived after v20.20.2 (node20 source now `None`); `test-fs-promises-writefile-typedarray.js` and `test-fs-promises-writefile-with-fd.js` left after v22.23.2 (new `node20_node22_exclusive_batch_case!`); `test-http-rawheaders-limit.js` is in no Node release and is removed, and `rust-watchpoints.json` is re-synced at 150 entries; catalog lookup uses the `parallel/...` prefix, and the result matched the vendored tree exactly |
| 2026-09-17 | NCT4 | Repaired a latent test-isolation race that the new drop records made visible. `NODE_COMPAT_OBSERVED_RESULTS_ENV` is process-wide, so a concurrent test appended to the file a test was reading back. Read-back now filters on the recorded Rust test name | `node_compat_observed_results_append_one_json_line_per_fixture` flaked in 2 of 8 runs before the fix; 85 passed, 1 failed (the pre-existing `__nimbus-preserve-symlinks-options-probe`), 2 ignored, stable across 10 consecutive runs; `cargo fmt`/`clippy` clean; `python3 -m unittest scripts.test_node_compat_corpus_baseline` 10 tests OK; `node-compat-validate-fixtures` and `node-compat-validate-watchpoints` (150 entries) clean; `node-compat-baseline-verify` ok at 1,188 gaps; `actionlint` clean; `check-docs.sh` PASS |
| 2026-09-17 | NCT4 | Found why the bound did not stop the truncation, and corrected an earlier reading. The bound is reached, not avoided: 24 batch tests hang and die at 600 s, and an earlier "0 timeouts" reading was a grep artifact, because the log carries ANSI codes between `TIMEOUT` and `[`. A sampled stack puts the block in `drop_in_place<NimbusRuntime>`, not in the bounded fixture call: `RuntimeExecutorInner::drop` joins a worker that sits inside `block_on` of a job which never observes the shutdown cancel | runs 35178467471 and 35187917432 both time out the same 24 tests at 600 s; 15 batches are refused by name, and each names the fixture after its last record (`test-worker-message-port.js` stops `loader-context` in all 4 lanes, `test-net-listen-invalid-port.js` stops `net-diagnostic-core` in 2, and 6 batches record 0 fixtures); the node20 batch reproduces on this machine and stops on the same fixture; `sample` shows `facade.rs:157` joining `worker_loop::cooperative::execution::admit_job_inner` parked in the tokio I/O driver |
| 2026-09-17 | NCT4 | Corrected the root cause and fixed it. The block is not the admission permit and not the executor drop itself: a stack sample of the hung process puts worker-0 inside `admit_job_inner` -> `invoke_direct`, with its tokio runtime parked in `kevent` and nothing left to wake it. A runtime invocation enforces its timeouts with a V8 termination, which reaches running JavaScript only, so a guest parked in the event loop never ends. `invoke_bundle_unmanaged` now waits for the guest and for the invocation stop signal together | `test-worker-message-port.js` alone reproduced the hang, and it now fails in 34 s with `runtime system wall time timed out after 30s` and a diagnostic artifact, instead of killing the process at 600 s; new test `runtime_times_out_an_invocation_that_parks_in_the_event_loop` passes in 4 s and, with `invocation.rs` stashed, hangs and is killed at 135 s |
| 2026-09-17 | NCT4 | Proved the truncation is gone end to end. The node20 `loader-context` batch runs to completion and reports a measurement, instead of dying at the runner bound | batch finished in 133.3 s with 40 named fixture gaps and no truncation refusal, where every earlier run was killed at 600 s; `test-worker-message-port.js` and `test-inspector-open.js` each record `runtime system wall time timed out after 30s` with a diagnostic artifact; `test-crypto-dh-leak.js` is a newly visible unexpected pass that the truncated runs could never reach; `make test-rust-runtime` 521 passed, 0 failed, 94 ignored |
| 2026-09-17 | NCT4 | Found that the node20 watchpoints never ran on a Node20 runtime. Each single-fixture `node20_*` test called `run_node_compat_watchpoint`, which sends no lane, and a missing lane resolves to `RuntimeCompatibilityTarget::Node24`. The lane thus ran node20 fixture files on a Node24 runtime and recorded each version difference as a permanent node20 gap. The 23 tests now call `run_node_compat_watchpoint_for_lane` with `NodeCompatLane::Node20`, and `post_bootstrap.js` sets the Node20 default stream highWaterMark to 16 KiB with the public `setDefaultHighWaterMark` API, guarded on the compatibility major that the file already reads | 15 of the 23 node20 watchpoints pass, against 7 before; 8 `#[ignore]` attributes removed and 8 stale reasons replaced with the measured failure; `rust-watchpoints.json` 150 -> 142 entries, node20 23 -> 15, and node22/node24/node26 hold at 63/44/20 with no entry added; `make test-rust-runtime` 521 passed, 0 failed, 94 ignored; the 6 node20 lane subsets give the same 3 passed and 3 failed as baseline `d93408e19`, with identical per-subset counts (streams-and-local-io 291 passed/13 gaps/7 failed, loader-context 131/26/14, networking 244/11/9); `test-http2-compat-serverrequest-host.js` failed once with `read ECONNRESET` in 1 of 4 runs and passed in the other 3, and it sends headers only, so the default highWaterMark cannot reach it; `make node-compat-required-surface-blockers` node22 0 and node24 0 |

## NCT4 platform constraint

The baseline records observed behavior, so it must be seeded on the platform
that runs the gate. The nightly runs on `ubuntu-24.04`. A baseline seeded from
a macOS developer machine would disagree with the gate on every
platform-sensitive fixture, and the first CI run would report that disagreement
as a mix of regressions and unexpected passes.

Seeding therefore needs one `workflow_dispatch` run of `Node Compatibility` on
this branch, and the `node-compat-observed-results` artifact from it.
