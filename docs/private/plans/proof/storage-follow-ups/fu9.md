# FU9 — remaining `mutation_journal` flake family

Status: fixed. Test-only changes; no product code touched.

Four tests were flaking, not the two assigned. Both assigned flakes were
root-caused, and the reproduction harness surfaced two more in
`frontier_contract` that share flake #1's mechanism. All four are fixed, and
the fifth signal found along the way is recorded below rather than guessed at.

The two causes are distinct:

- **Cause A — the restarted trigger-candidate worker's cursor record.** The
  FU5 class. Three tests (#1, #3, #4).
- **Cause B — a fatal `expect` on a documented transient refusal during the
  crash-and-replay window.** One test (#2), plus one latent sibling.

## Flake #1 — `publisher_observers::projection_provider_schema_refresh_waits_for_journal_frontier`

Assigned symptom, at `publisher_observers.rs:196`:

```
assertion `left == right` failed: schema publication provenance must cover the durable schema record
  left: SequenceNumber(2)
 right: SequenceNumber(1)
```

### Offending state, captured from a natural failure

Dumping the journal and the sampled token on assertion failure:

```
token:     ProjectionToken { tenant_incarnation: 1, lease_epoch: 0,
                             durable_sequence: SequenceNumber(2) }
record #1: seq 1  SchemaChange { SetTable { table: "tasks" } }   writes: []
record #2: seq 2  TriggerDelivery { cursor: TriggerDeliveryCursor {
                    materialized_through: SequenceNumber(1) } }  writes: []
```

### Mechanism

`catch_up_loaded_provider_tenant_async` reconciles the journal, refreshes
schema, then samples **one** `ProjectionToken` for every observer notification
(`provider_hints.rs:275`). On the embedded store that token's
`durable_sequence` is `runtime.applied_head()`
(`committed_mutations.rs:1031-1040`), so any record landing concurrently raises
it.

The reconciliation itself is what lands that record: it dispatches the
reconciled commits with `emit_trigger_candidates = true`
(`provider_hints.rs:252-257`), and dispatch calls
`runtime.ensure_trigger_candidate_worker_started()`
(`commit_processing.rs:83-93`). The test's `shutdown_trigger_candidates_for_testing`
is a **lifecycle** shutdown, and `BackgroundWorker` restarts by design — so the
worker comes back and commits its zero-write cursor advance at seq 2, racing
the token sample.

This is legitimate engine behavior. The test-side fix is FU5's pattern applied
directly: `disable_trigger_candidates_for_testing`, which permanently suppresses
both `start_worker` and `enqueue_commits` so no restart is possible. The
assertion at :196 is unchanged.

## Flakes #3 and #4 — `frontier_contract`, same cause

Not assigned; found by running the whole module in-process under load (see
Method). Both are cause A, and both were confirmed against captured panics:

| Test | Assertion | Observed |
| --- | --- | --- |
| `frontier_diagnostics_remain_ordered_under_concurrent_sampling` | `frontier_contract.rs:6`, `active_assigned_head == durable_head` | `64/63`, `62/61`, `3/2` |
| `publisher_stall_diagnostics_distinguish_assignment_apply_and_publication_lag` | `frontier_contract.rs:234`, `publication_lag == 1` | `2` vs `1` (also `:6`, `3/2`) |

An assigned-but-not-yet-durable cursor record is exactly a `+1` on
`active_assigned_head` and a `+1` on `publication_lag`.

Both tests shut the worker down to express "no unrelated records", then commit
documents that restart it — 32 commits in the first, the seed commit in the
second. The file's own `settled_frontier` helper already documents this hazard
and drains it, but neither of these two sampling points goes through that
helper. Fixed by making the suppression permanent at both sites, matching the
stated intent.

`publisher_stall_diagnostics...` was identified by reading the code before its
failures were tallied; the tally then showed it was the *most* frequent failure
in the family.

## Flake #2 — `durable_outcomes::trigger_cursor_unreadable_progress_evicts_and_replays`

The weak signal, and a different cause. My pre-reproduction hypothesis — that
the `OutcomeCase::Unreadable` path's surviving assertions are polling budgets,
so a failure would be a timeout — was **wrong**. The captured panic
(`durable_outcomes.rs:536`) is not a timeout:

```
trigger-cursor replacement schema should load: Storage { kind: Unavailable,
  message: "tenant trigger-cursor-unreadable runtime is restarting after durable recovery" }
```

### Decision: test-side, and the product is right

The refusal is deliberate contract, on three independent pieces of evidence:

1. `TenantLifecycle::operation_rejection_if_deleted` (`tenant/lifecycle.rs:53-67`)
   returns it with the comment "Durable-recovery eviction is transient... Sync/
   admission races must not turn that window into a false 404."
2. `Error::retryability` (`nimbus-core/src/error.rs:415-424`) classifies
   `Storage{Unavailable}` as `RetryableAfterBackoff` — callers are *required*
   to retry it.
3. `publisher_recovery.rs:1050` asserts this exact error as expected behavior.

So the defect is the test calling `.expect()` on a refusal it is contractually
obliged to retry. Fixed with `support::load_schema_across_runtime_restart`,
which retries **only** that refusal — every other error still fails
immediately, and exhausting the budget still fails.

### A second refusal in the same window

The load battery then produced a different panic one line lower, at the
identity read (`durable_outcomes.rs:544`, in
`trigger_cursor_advanced_head_evicts_replays_and_does_not_reuse_sequence`):

```
trigger-cursor replacement runtime identity should load: InvalidInput(
  "embedded-only blocking tenant lifecycle helpers are unavailable for
   non-embedded persistence providers; use the async engine surfaces")
```

Same window, different surface: the blocking `tenant_runtime_identity_for_testing`
hook falls through to `Engine::require_embedded_provider_kind` (`engine/mod.rs:781`)
precisely when the tenant is absent from the registry — the gap between
deregistering the failed runtime and registering its successor
(`engine/tenants.rs:457-471`).

`support::wait_for_replacement_runtime_identity` now covers both reads. Passing
still requires a successful schema load **and** an identity different from the
one before the crash — strictly what the test asserted before — with the last
refusal attached to the timeout message for diagnosability. The same helper
replaces the identical single-shot pattern in the schema-outcome exerciser
(`durable_outcomes.rs:155`), which was latently exposed to the same window.

## Method

Single-test loops do not reproduce cause B. Flake #2 survived **460** such runs:

| Shape | Result |
| --- | --- |
| Isolated, serial | 0 / 60 |
| 8 concurrent copies | 0 / 200 |
| 8 concurrent copies + parallel `cargo build` | 0 / 200 |
| Whole module in-process, 6 concurrent | reproduced |

It needs its ~136 in-process siblings, so the reproduction unit had to become
the module run, not the test run.

**Attribution used concurrent A/B, never sequential batches.** A sequential
tally on the fixed binary read 28 failures / 60 module runs against a 6/60
baseline, which looks like a severe regression and is not one: it is ambient
machine load, the same confound that nearly caused a misattribution in FU1.
Running both binaries' workers *simultaneously* removes it, because neither arm
can occupy a different machine state than the other:

| A/B run | Arm A (pre-fix) | Arm B (fixed) |
| --- | --- | --- |
| 3 workers/arm x 8 | 4 / 24, incl. 1 FU9 target | 3 / 24, **0** FU9 targets |
| 2 workers/arm x 12 | 1 / 24, a FU9 target | **0 / 24** |

### The `queued::*` timeouts are my load, not a defect

Both arms show a residual family (`concurrent_mutations_do_not_strand_the_journal_worker`
and neighbors) failing at `queued.rs:237` with `Elapsed(())` — a **45-second
wall-clock budget** on a concurrent workload. It appears at equal rates in the
fixed and unfixed arms and disappears entirely at lower concurrency, so it is
an artifact of running six copies of a 137-test module at once, not something
this change caused or should mask. Recorded, not fixed: no CI lane runs that
shape.

## Changes

All in `crates/nimbus-engine/src/tests/mutation_journal/`:

| File | Change |
| --- | --- |
| `publisher_observers.rs` | flake #1: lifecycle shutdown -> permanent disable |
| `frontier_contract.rs` | flakes #3/#4: same, at the concurrent-sampling and publication-stall tenants |
| `durable_outcomes.rs` | flake #2: three fatal `expect`s on in-window refusals -> the two helpers |
| `support.rs` | new `load_schema_across_runtime_restart` and `wait_for_replacement_runtime_identity` |

No assertion was weakened. Cause A's fixes change *setup* (which suppression
hook), not expectations. Cause B's fixes change a fatal read into a bounded
retry whose success condition is unchanged.

## Verification

Run in `/Users/jack/src/github.com/nimbus/nimbus-fu9-journal` on branch
`codex/fu9-journal-flakes`, base `8c4093eaa`, with
`NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1` for the suite runs.

| Gate | Result |
| --- | --- |
| 100x isolated, `projection_provider_schema_refresh_waits_for_journal_frontier` | 0 failures / 100 |
| 100x isolated, `frontier_diagnostics_remain_ordered_under_concurrent_sampling` | 0 failures / 100 |
| 100x isolated, `publisher_stall_diagnostics_distinguish_assignment_apply_and_publication_lag` | 0 failures / 100 |
| 100x isolated, `trigger_cursor_unreadable_progress_evicts_and_replays` | 0 failures / 100 |
| 100x isolated, `trigger_cursor_advanced_head_evicts_replays_and_does_not_reuse_sequence` | 0 failures / 100 |
| Full engine suite, run 1 | 663 tests run: 663 passed (2 slow), 5 skipped — 93.5s |
| Full engine suite, run 2 | 663 tests run: 663 passed (2 slow), 5 skipped — 108.2s |
| Full engine suite, run 3 | 663 tests run: 663 passed (2 slow), 5 skipped — 109.9s |
| `cargo clippy -p nimbus-engine --all-targets --all-features -- -D warnings` | exit 0 (21 warning lines, all vendored `brotli`) |
| `cargo fmt --all --check` | exit 0 |

Fail-before evidence for the four fixed tests is the pre-fix arm of the two A/B
tables above, plus the 6 failures / 60 module runs measured on the binary
carrying only flake #1's fix (3x `publisher_stall_diagnostics`,
2x `frontier_diagnostics_remain_ordered`, 1x `trigger_cursor_unreadable`).
