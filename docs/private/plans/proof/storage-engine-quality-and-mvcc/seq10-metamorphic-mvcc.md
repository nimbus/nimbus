# SEQ10 Metamorphic MVCC

status: done

## Scope

SEQ10 expands the generated-history harness into an MVCC conformance lane. The
new coverage keeps the existing pure `GeneratedTaskHistoryModel` and adds a
small datadriven history DSL so failures can be reproduced from a readable
script as well as from seeded histories.

The required embedded lane now verifies three views of the same generated
history:

1. Latest materialized state after each generated prefix.
2. PITR-restored historical prefixes at selected committed sequences.
3. CDC/changefeed document-write sequences from the initial snapshot cut through
   the log tail.

This phase intentionally keeps live external-provider execution under the
existing SEQ3/SEQ4 evidence gate. The generated MVCC oracle is backend-neutral
and currently runs as the required embedded proof; provider-aware seed
execution remains a SEQ14 closeout requirement once MySQL/libSQL live fixtures
are available.

## Read-Before-Edit Checklist

- `docs/plans/storage-engine-quality-and-mvcc-plan.md`
- `crates/nimbus-storage/src/simulation/generated.rs`
- `crates/nimbus-storage/src/simulation/verification.rs`
- `crates/nimbus-storage/src/tests/generated_history.rs`
- `crates/nimbus-storage/src/store/journal_snapshot.rs`
- `crates/nimbus-storage/src/changefeed.rs`

## Implementation Evidence

| Area | Evidence |
| --- | --- |
| Datadriven DSL | `GeneratedTaskHistory::datadriven(...)` parses `insert <slot> <status> <rank> <title>`, `update <slot> <status> <rank> <title>`, and `delete <slot>` scripts; it rejects duplicate inserts, missing updates/deletes, malformed ranks, and malformed slots with line-numbered errors. |
| Pure model reuse | The existing `GeneratedTaskHistoryModel` remains the oracle for final and prefix document state. `GeneratedTaskHistory::model_through(...)` is reused for every latest and PITR prefix assertion. |
| MVCC/PITR oracle | `assert_generated_task_mvcc_history_matches_model(...)` replays a history once, records every committed prefix sequence, exports PITR archives for first/middle/final checkpoints, imports each archive into a fresh tenant, and compares restored documents with the pure model prefix. |
| CDC oracle | The same helper exports a changefeed bootstrap before replay, streams pages from the typed cursor after replay, filters authoritative `TenantEventKind::DocumentWrite` events, and asserts the streamed sequences exactly match the committed prefix sequence list. |
| Reproducible required lanes | `datadriven_generated_task_history_drives_mvcc_pitr_and_cdc_conformance` covers script-driven histories. `generated_mvcc_history_required_seed_corpus_matches_pitr_and_cdc_models` covers a deterministic seeded generated history with the same oracle. |

## Verification Evidence

| Command | Result |
| --- | --- |
| `cargo test -p nimbus-storage generated_mvcc -- --nocapture` | Passed: `1 passed, 0 failed`, `298 filtered out`. Covers deterministic seeded generated MVCC/PITR/CDC conformance. |
| `cargo test -p nimbus-storage datadriven_generated_task_history -- --nocapture` | Passed: `1 passed, 0 failed`, `298 filtered out`. Covers the datadriven script parser plus MVCC/PITR/CDC conformance. |
| `cargo test -p nimbus-storage generated_history -- --nocapture` | Passed: `8 passed, 0 failed`, `2 ignored`, `289 filtered out`. Confirms the existing generated-history, recovery, diagnostics, shadow materializer, datadriven, and generated MVCC lanes remain green. |
| `cargo check -p nimbus-storage` | Passed. |

## SEQ10 Closeout

SEQ10 is complete for the required embedded generated MVCC conformance lane.
Nimbus now has a readable datadriven history DSL, deterministic generated MVCC
seed coverage, latest-prefix checks, PITR historical-prefix checks, CDC no-miss
sequence checks, and reproducible failure context that points back to either a
script line or generated seed.
