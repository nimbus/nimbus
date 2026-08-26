# SRR2 Atomic PITR Import Proof

Date: 2026-08-26.
Implementation commit: `1a553ac87`.

## Outcome

Memory, redb, and SQLite now publish an embedded point-in-time restore as one
state change. The transaction contains the base snapshot, base-sequence MVCC
anchors, the retained journal tail, the final applied position, and the
retention checkpoint and floors.

The importer validates the archive and replays it into a disposable store
before it opens the destination write transaction. A target-position mismatch
therefore fails before the destination changes.

Redb and SQLite seed one document version for each live checkpoint document and
one open index interval for each maintained checkpoint tuple. Tail replay then
closes or replaces those anchors through the normal version-writing seams.

## Fail-Before Evidence

Command:

```text
cargo test -p nimbus-storage embedded_pitr_import_ -- --nocapture
```

Baseline result: two tests failed.

- `embedded_pitr_import_fault_rolls_back_and_same_archive_retries` observed a
  redb destination at sequence 3 after the injected tail-append fault. The
  expected sequence was 0.
- `embedded_pitr_import_seeds_base_history_and_survives_restart` read `None`
  for a live checkpoint document at the imported base sequence.

## Contract Evidence

- The importer stages all work, then runs the tail-append fault before the
  visibility boundary. Memory, redb, and SQLite remain empty. Each store retains
  floor 0 and accepts the same archive on the next call.
- At the base and next sequence, redb and SQLite return the checkpoint document.
  The first retained-tail update is at sequence 5.
- At the base and next sequence, redb and SQLite return the checkpoint rank.
  Both stores return the updated rank at sequence 5.
- Reopening the imported redb and SQLite files preserves the same historical
  document and index results.
- The failed redb and SQLite imports contain zero document-version and
  index-version rows.

## Verification

| Command or gate | Result |
| --- | --- |
| Focused fail-before regressions | 2 passed after repair. |
| Retention checkpoint tests | 15 passed. |
| Journal snapshot tests | 17 passed. |
| SQLite snapshot tests | 3 passed. |
| Generated retained-checkpoint model | 1 passed. |
| Cross-provider canonical history model | 1 passed. |
| `cargo nextest run -p nimbus-storage` | 397 passed; 4 planned tests skipped. |
| `cargo clippy -p nimbus-storage --all-targets --all-features -- -D warnings` | Passed. Warnings came only from vendored dependencies. |
| `cargo fmt --all --check` | Passed. |
| `git diff --check` | Passed. |

## Remaining Uncertainty

SRR5 owns the complete workspace and hosted provider gates.
External SQL providers still accept only sequence-0 journal-replay imports and
are outside this embedded nonzero-base repair.
