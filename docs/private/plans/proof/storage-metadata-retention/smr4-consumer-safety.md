# SMR4 Trimmed-History Consumer Safety Proof

Date: 2026-08-26.
Implementation commits: `240a12a49`, `edb901dd6`, `a8df4588e`, `c3699454b`.
Pull request: #320.
Merge commit: `e6b9fcb057d1253ff788558b95bded5aefd9d7c5`.

## Outcome

SMR4 makes every retained-history consumer fail closed when physical
compaction crosses its required range. Nimbus persists separate document,
index, and journal read floors on memory, redb, SQLite, PostgreSQL, MySQL, and
libSQL. It publishes the committed floors to process-local readers only after
the owning transaction succeeds.

Durable journal pages validate the authoritative journal floor before and
after each page, verify serialized sequence identity and contiguous physical
records, and keep an empty logical event distinct from a missing sequence.
Historical document and index reads, bootstrap, changefeed, PITR export, and
verification rebuild use the same typed retention contract. A concurrent
prune returns `RetentionExpired`; it cannot return a gap or partial success.

Standalone document/index version compaction now publishes the matching
durable and process-local read floors in the same transaction. Engine
consistency verification maps typed retention expiry to the existing full
scrub recovery route instead of treating trimmed history as corruption.

## Contract Evidence

- `RetentionReadFloors` owns the three resource floors and their durable
  encoding. Every backend loads and publishes the same contract.
- `retention/read_safety.rs` owns pre-page and post-page validation plus
  contiguous durable-journal checks.
- The libSQL remote-primary page shape materializes provider state and one
  bounded page from the same SQLite snapshot. The public stream keeps one
  later authoritative floor read after the fault boundary. This reduced one
  logical journal page from seven remote requests to two.
- `PauseAfterRetentionReadPage` is a one-shot armed fault. Tests arm it only
  after provider cache synchronization, so background libSQL refresh cannot
  consume the intended concurrent-prune boundary.
- The storage writer ownership scanner and effect matrix require read-floor
  publication for checkpoint finalization and standalone version compaction.
- Fully buffered readers need no lifetime pin. A future lazy streaming design
  must add pins before it can retain references across a page boundary.

## Verification

| Command or gate | Result |
| --- | --- |
| `cargo fmt --all --check` and `git diff --check` | Passed on the final commit. |
| `cargo check -p nimbus-storage --all-features` | Passed. |
| `cargo check -p nimbus-storage --no-default-features` | Passed. |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-storage --no-default-features --lib` | 388 passed, 0 failed, 3 ignored. |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 cargo test -p nimbus-storage --all-features --lib` | 552 passed, 0 failed, 3 ignored. |
| Storage ownership lanes, with and without all features | 6 passed in each feature graph. The scanner found 59 methods, 27 direct writers, 46 total writers, and 56 effect-matrix rows. |
| `cargo clippy -p nimbus-storage --all-targets --all-features -- -D warnings` | Passed after the final feature-boundary extraction. |
| Focused live libSQL qualification | 5 passed: the seeded PPSC differential plus four retention tests for concurrent prune, checkpoint rollback, provider-error rollback, and stale-lease restart. The final local PPSC run completed in 60.849 seconds. |
| `bash scripts/verify-storage-metadata-retention.sh` | Expected intermediate state: `Summary: 17 passed, 1 failed`. Only SMR5 closeout proof remains. |
| Nimbus autoreview, final `pre-pr` pass | Clean. It reviewed lock ordering, post-page checks, head widening, and the compound libSQL query. No accepted or actionable findings. |
| Hosted CI attempt 1 | 45 jobs passed, 3 scheduled/nightly jobs skipped, and the libSQL provider job failed only `libsql_replica_post_visibility_ack_loss_forces_crash_and_replay`. All four SMR4 retention tests passed; the seeded PPSC differential passed in 90.855 seconds. The same test and assertion at `libsql_replica_provider.rs:247` failed on clean main run `32952327904`, job `98145513617`. Two retries ended as GitHub `startup_failure` before job creation. PR #320 remained `MERGEABLE` and `CLEAN` and merged without an override. |

## Remaining Boundary

SMR4 proves the consumer contract but does not make the final launch-readiness
claim. SMR5 owns generated-history and crash/restart qualification, all
configured provider lanes, measured latest-path and retention budgets,
operator and architecture documentation, `make ci`, the fully green verifier,
and the final `SAFE` or `NOT SAFE` verdict.
