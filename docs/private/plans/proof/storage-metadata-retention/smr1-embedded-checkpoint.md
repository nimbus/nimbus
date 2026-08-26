# SMR1 Embedded Checkpoint Proof

Date: 2026-08-26.
Implementation commits: `c1a9dab72`, `659f67412`.
Pull request: #314.
Merge commit: `0d4b9a1125bfe6febaa6b585f84a4818e54c024d`.

## Outcome

SMR1 establishes the provider-neutral retained-history contract and implements
it on memory, redb, and SQLite. A versioned materialized checkpoint binds the
retained replay base, desired, confirmed, and physical floors stay distinct,
and each embedded backend publishes the checkpoint, deletes the journal
prefix, and prunes eligible MVCC history in one transaction or write lock.

PITR archives now support a validated nonzero base. The materialized snapshot
also carries resource-path bindings and the trigger-delivery cursor, so
checkpoint replay does not lose those journaled sidecars. A target before the
retained base fails closed, and a gapped or tampered retained tail is rejected.

The embedded cursor floor uses the published physical floor. Journal deletion
through sequence `N` is intentional because the journal and changefeed cursor
contract reads records strictly after `N`. Document and index compaction stays
strictly before its floor because a historical snapshot at `N` still needs its
MVCC anchor or interval.

## Contract Evidence

- `RetentionGcConfig` has separate document-version, index-version, CDC, and
  PITR windows.
- `MaterializedRetentionCheckpoint` is versioned and self-digesting. Its base
  digest binds materialized state, resource-path bindings, and the
  trigger-delivery cursor.
- Candidate construction replays a contiguous journal tail from the prior
  checkpoint and refuses a cut above the applied head.
- Memory, redb, and SQLite compare the expected prior checkpoint before they
  publish a new one. A concurrent append can add only a higher sequence and
  cannot create a retained gap.
- Redb and SQLite commit checkpoint metadata, physical floor, journal prefix
  deletion, and MVCC pruning atomically. Memory performs the same transition
  under one write lock.
- Faults before commit expose the old checkpoint and full history. Faults
  after commit expose the new checkpoint and pruned floor together.
- PITR export and import accept a validated nonzero base, validate the target
  sequence and timestamp, and reinstall the imported checkpoint and floor.
- Redb and SQLite idempotent replay still applies resource-path-binding
  effects when the document row is already current or absent.

## Verification

| Command or gate | Result |
| --- | --- |
| Focused embedded retention tests | 12 passed. Covered restart, nonzero-base restore, both checkpoint fault points, retain-all monotonicity, four windows, tamper rejection, sidecar restore, concurrent append, and durable-but-unapplied protection. |
| Generated retained-history model | Passed. Every target from the retained checkpoint through the head restored to the model; a target before the checkpoint expired. |
| PITR journal snapshot tests | 12 passed. Included every snapshot field, target tamper, and retained-tail validation. |
| `cargo test -p nimbus-storage --lib --no-fail-fast` | 379 passed, 3 ignored. |
| `cargo test -p nimbus-storage --no-default-features --lib --no-fail-fast` | 379 passed, 3 ignored. |
| `cargo check -p nimbus-storage --all-features --tests` | Passed. |
| `cargo check -p nimbus-engine --tests` | Passed. |
| `cargo check -p nimbus-object-storage --tests` | Passed. |
| `cargo fmt --all --check` | Passed. |
| `cargo clippy -p nimbus-storage --all-targets --all-features -- -D warnings` | Passed. Vendored dependency warnings only. |
| `bash scripts/verify-storage-metadata-retention.sh` | Expected intermediate state: `Summary: 12 passed, 6 failed`. The six failures are owned by SMR2 through SMR5. |
| Nimbus autoreview, `pre-pr` | First pass clean. One subthreshold reliability observation led to bounded condition-variable waits in `659f67412`; the required rerun was clean with no accepted or actionable findings. |
| Pull request #314 hosted merge gates | Passed. PR merged as `0d4b9a112`. |

`git diff --check` also passed before publication.

## Routed Follow-Up Evidence

Two pre-existing boundaries were found and kept out of SMR1:

1. The PITR snapshot contract still omits scheduler jobs, scheduler results,
   cron state, and trigger-invocation records that are not materialized from
   the durable journal. SMR4 must decide and test the public PITR contract for
   those consumers before closeout.
2. `MaterializedPosition` verification does not bind resource-path bindings or
   the trigger-delivery cursor. The independent retention-checkpoint digest
   protects the retained base now. Expanding the IMV verifier or manifest is
   an IMV-owner proposal, not an SMR1 change.

SMR1 has no provider lease-fencing or production lifecycle caller. SMR2 and
SMR3 own those remaining contracts.
