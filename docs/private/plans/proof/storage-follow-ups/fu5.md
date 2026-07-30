# FU5 — `opaque_internal_job_cannot_overtake_ordered_publisher` flake

Status: fixed. Test-only change; no product code touched.

## Symptom

`crates/nimbus-engine/src/tests/mutation_journal/arm_selection.rs`:

```
assertion `left == right` failed
  left: 3
 right: 2
```

at `assert_eq!(journal.len(), 2)`. Reproduced at multiple unrelated base
commits across three campaigns, both under full-suite load and in isolation,
so it is pre-existing and not load-only.

## Identity of the extra record

Captured by dumping the full durable journal on assertion failure. Record #3
in a natural (unforced) failure:

```
sequence: SequenceNumber(3)
events: [ TriggerDelivery { cursor: TriggerDeliveryCursor {
            materialized_through: SequenceNumber(1) } } ]
writes: []
is_provably_inert_trigger_delivery_only: true
```

Records #1 and #2 are the expected `DocumentWrite` (seq 1) and `SchemaChange`
(seq 2). The extra record is the trigger-candidate worker's zero-write
delivery-cursor advance, acknowledging the document commit at seq 1.

## Mechanism

The test calls `shutdown_trigger_candidates_for_testing`, which is a
**lifecycle** shutdown (`TriggerCandidateFeed::shutdown` →
`BackgroundWorker::shutdown`). It is not a suppression: `BackgroundWorker`
supports restart by design, and every commit batch restarts the feed —
`Engine::dispatch_or_enqueue_trigger_candidates`
(`crates/nimbus-engine/src/engine/mutations/commit_processing.rs:91`) calls
`runtime.ensure_trigger_candidate_worker_started()` before enqueuing.

So the sequence is:

1. Test shuts the trigger-candidate worker down.
2. The test's own document insert commits at seq 1 and **restarts** the worker.
3. The restarted worker builds trigger candidates for that commit. The tenant's
   trigger registry is ready (populated at runtime construction,
   `engine/mod.rs:701`) with zero registrations, so it materializes zero
   invocation records but still commits the cursor advance through seq 1 via
   `materialize_trigger_invocations_and_sync`.
4. That cursor commit lands as a zero-write `TriggerDelivery` record at seq 3,
   racing the test's `read_durable_journal_async`. Whichever wins decides
   whether the test sees 2 or 3 records.

This is legitimate engine behavior, not a product race. Production callers of
`shutdown_trigger_candidates` are tenant teardown and durable-recovery eviction
(`tenant.rs:665`, `engine/tenants.rs:357`, `engine/mutations/publisher.rs:770`),
where resuming on the next commit is correct. No product change is warranted.

The same file already documented this behavior in a sibling test
("A document write restarts the trigger-candidate worker"); the flaky test
simply never accounted for it.

## Fix

Use `disable_trigger_candidates_for_testing` instead. Unlike the lifecycle
shutdown, it sets a permanent flag that makes both `start_worker` and
`enqueue_commits` no-ops, so no later commit can restart the feed. The hook
already existed for exactly this purpose.

The assertion is unchanged — the extra record is excluded at the source rather
than tolerated, so the test still proves the journal holds exactly the two
records the two mutation paths produced, in order.

Three sibling tests in the same file had the identical latent defect and are
fixed the same way. Two of them compare journal **bytes** across committer
arms, where a nondeterministic cursor record breaks the comparison, and their
`expect` strings already stated the intent the lifecycle shutdown failed to
deliver ("background cursor commits should be disabled for byte comparison",
"trigger cursor should not add nondeterministic records"):

- `run_static_arm_workload` (backs `construction_time_committer_arms_produce_identical_state`)
- `run_seeded_history` (backs `provider_publisher_pipeline_matches_serial_reference_for_seeded_history`)
- `ordered_publisher_serializes_queued_direct_and_execution_unit_paths` — its
  seed-time shutdown was undone by the later queued write; an interleaved
  cursor commit would break its contiguous-sequence assertion.

`journal_progress_sync_cannot_overtake_publisher` was left on the lifecycle
shutdown: it asserts only while the publisher is paused at
`DURABLE_BEFORE_PUBLISH`, before any dispatch can restart the worker.

## Evidence

Fail-before / fail-after, deterministic. A temporary probe ran the exact test
body and then polled the journal:

- With `shutdown_trigger_candidates_for_testing`: the third record appears
  within the poll window, every run. Captured identity above.
- With `disable_trigger_candidates_for_testing`: `max(journal.len()) == 2`
  over 10s of continuous polling — the record never appears at all.

Rate, single test in isolation, same machine, back to back:

| tree | failures |
| --- | --- |
| pre-fix | 8 / 100 |
| post-fix | 0 / 100 |

The 8/100 pre-fix rate matches the ~1-in-8 rate reported from earlier campaigns.

Full engine suite, three consecutive runs, all green:

```
NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo nextest run -p nimbus-engine --no-fail-fast
run 1: 663 tests run: 663 passed (2 slow), 5 skipped
run 2: 663 tests run: 663 passed (2 slow), 5 skipped
run 3: 663 tests run: 663 passed (2 slow), 5 skipped
```

`cargo fmt --all --check` clean. `cargo clippy -p nimbus-engine --all-targets
--all-features -- -D warnings` clean.

## Note on two discarded measurements

An intermediate suite run showed one failure in
`tests::mutation_journal::triggers::trigger_candidate_worker_retries_transient_materialization_failure`
(a 1s `eventual` polling budget timing out), and a follow-up loop measured it
at 15/100 and then 98/100. Those numbers are invalid: the machine's data volume
had filled to 259Mi free and the harness was reporting write errors. After
space was reclaimed, the same test measured 0/40 on the pre-fix tree and 0/40
on the post-fix tree, interleaved under identical conditions. It is unrelated
to this change, which touches only `arm_selection.rs`.

Without the `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1` guard that
`make` sets, a bare `cargo nextest run -p nimbus-engine` fails 70 tests
demanding live postgres/mysql fixtures. That is environmental, not a
regression.
