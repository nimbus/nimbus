# Node Compatibility Corpus Trust

Status: `active` | Owner: this plan | Created: 2026-09-16
Baseline: main @ `f743836c6`
Proof root: `proof/node-compat-corpus-trust/`

Next action: NCT8 - delete this plan when the pull request merges

## Current resume state

- Updated: 2026-09-16. Active task: NCT8.
- Worktree: `scratchpad/wt-node-compat`. Branch: `ci/node-compat-corpus-trust`. HEAD `361c5ee98`.
- Dirty files owned by this task: none.
- NCT0 through NCT7 are done. The baseline is seeded from run 35167962571.
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
| NCT4 | Seed the baseline from a full instrumented run | done | Run 35167962571: 12 shards, 6,930 fixtures, 1,204 failures, 1,188 recorded; 16 named refusals and no unresolved path |
| NCT5 | Close the unexpected-pass loop for ignored watchpoints | done | `corpus-baseline-reconciliation` job feeds `--observed-results` to the 150-entry catalog |
| NCT6 | Guard the baseline | done | 4 guard rejections proven; runs in the PR lane via `make node-compat-baseline-verify` |
| NCT7 | Document the contract | done | `docs/private/operating/node-compat-nightly.md`; routed from the operating README; `check-docs.sh` PASS |
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
- Result: done, with one stated exception. The baseline holds 1,188 gaps from
  run 35167962571. 15 node20 fixtures on the required surface keep the lane
  red, which is the gate working as designed. The runtime fix owns them.

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

## NCT4 platform constraint

The baseline records observed behavior, so it must be seeded on the platform
that runs the gate. The nightly runs on `ubuntu-24.04`. A baseline seeded from
a macOS developer machine would disagree with the gate on every
platform-sensitive fixture, and the first CI run would report that disagreement
as a mix of regressions and unexpected passes.

Seeding therefore needs one `workflow_dispatch` run of `Node Compatibility` on
this branch, and the `node-compat-observed-results` artifact from it.
