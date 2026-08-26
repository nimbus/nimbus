# SMR3 Production Lifecycle Proof

Date: 2026-08-26.
Implementation commits: `f7fcead24`, `ec85d69b0`.
Pull request: #319.
Merge commit: `820549575ecbfde94731276e106ad681c31511c7`.

## Outcome

SMR3 makes metadata retention part of the production Engine tenant lifecycle.
Every Engine construction path now uses an explicit `MetadataRetentionProfile`.
The shipped profile retains 100,000 document, index, and PITR sequences, 50,000
CDC sequences, and runs maintenance in 10,000-sequence steps. Operators can
select explicit retain-all behavior.

Each loaded tenant owns one single-flight controller. It inspects and prepares
retained history outside the mutation route, then submits finalization through
the ordered internal committer route. Finalization publishes the checkpoint,
prunes the durable journal, and prunes eligible MVCC history under one storage
transaction. Tenant eviction, explicit deletion, and Engine quiesce cancel the
controller and wait for accepted work to drain.

The controller reports desired, confirmed, and physical floors; floor lag;
run, success, failure, and pruned-record counts; duration; last failure; retry
delay; and next eligibility. A bearer-protected local-admin route can request
one manual run and returns the typed result. The CLI, environment, and file
configuration surfaces preserve their documented precedence.

## Contract Evidence

- `MetadataRetentionProfile` owns the four durable windows and maintenance
  step. Validation rejects zero windows and a zero step.
- `MetadataRetentionController` owns tenant-local single flight, deterministic
  retry, progress wakeup, periodic recheck, cancellation, manual requests, and
  bounded diagnostics.
- A successful retry that finds no eligible work clears its expired retry
  deadline. The regression proves that the controller returns to its periodic
  wait instead of entering a zero-delay storage-read loop.
- `prepare_retained_history` remains read-only. Finalization uses the existing
  embedded process fence or current provider committer lease.
- `RetentionFinalizationGuard` closes the late-pin race between preparation and
  finalization on memory, redb, and SQLite. A pin created after preparation
  makes finalization fail closed.
- The storage writer ownership matrix declares checkpoint publication,
  journal-prefix deletion, MVCC pruning, fencing, and materialized-root
  neutrality for the new Engine finalizer.
- `POST /debug/tenants/{tenant_id}/engine/retention` is installed only on the
  local-admin router. The server test proves unauthenticated refusal and
  authenticated success.

## Verification

| Command or gate | Result |
| --- | --- |
| `cargo test -p nimbus-engine engine::metadata_retention::tests:: --all-features -- --nocapture` | 10 passed. Covered automatic advancement, resource-specific windows, single flight, retry, recovered ineligible retry, off-route preparation, no below-threshold hot-path wait, shutdown drainage, retain-all diagnostics, and late manual refusal. |
| Focused storage retention and ownership suites | 28 passed: 24 retention tests and 4 ownership-effect tests. |
| Focused CLI configuration and local-admin server tests | Passed. Covered CLI/environment/file precedence, route authorization, and manual result serialization. |
| `cargo fmt --all --check` and `git diff --check` | Passed. |
| Storage and Engine Clippy lanes | Passed. Vendored dependency warnings only. |
| `make ci` | Rust format, Clippy, deny, 517 runtime tests, 8 locker tests, 7,632 non-runtime tests, doc tests, and the required harness passed. The aggregate JavaScript build stopped after an idle Vite/Rolldown worker stalled; its isolated target then passed, and `make build-js typecheck-js test-js proof-helpers` passed, including 95 UI files and 832 UI tests. |
| `bash scripts/verify-storage-metadata-retention.sh` | Expected intermediate state: `Summary: 16 passed, 2 failed`. SMR4 owns post-page consumer validation. SMR5 owns closeout evidence. |
| Nimbus autoreview, first `pre-pr` pass | Found one blocking retry busy loop after a failure followed by a successful ineligible inspection. Commit `ec85d69b0` fixed it and added a behavioral regression. |
| Nimbus autoreview, final `pre-pr` pass | Clean. No accepted or actionable findings. |
| Pull request #319 | Merged as `820549575`. The required hosted merge gate passed; other informational jobs were still pending at proof capture. |

## Remaining Boundary

SMR3 can now delete retained prefixes while Engine consumers are active. SMR4
must make every durable-journal, changefeed, bootstrap, historical-read, and
PITR page validate the authoritative floor after its read. A concurrent prune
must return typed `RetentionExpired`, never a gap or partial success. SMR5 owns
the full provider and benchmark qualification plus the final launch-readiness
verdict.
