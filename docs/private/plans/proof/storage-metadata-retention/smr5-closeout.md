# SMR5 Qualification And Closeout Proof

Date: 2026-08-26.
Implementation commit: `a2b2e5215bbf261e3efe941d021367d17c28c999`.
Pull request: #321.
Merge commit: `4fe11923de9c2ed67a14d7c22f2253132493f3f0`.

## Verdict

`SAFE` for launch with the shipped bounded profile.

Nimbus now has one checkpoint-before-delete retention contract on memory,
redb, SQLite, PostgreSQL, MySQL, and libSQL. Generated histories, restart and
fault cases, concurrent appends and reads, provider fencing, bounded PITR,
steady-state storage, the production lifecycle, operator controls, and the
repository gate all passed their owning evidence. No qualification result
requires a change to the ratified 100,000-sequence document, index, and PITR
windows, the 50,000-sequence CDC window, or the 10,000-sequence maintenance
step.

## Semantic And Fault Qualification

| Lane | Result |
| --- | --- |
| Generated retained checkpoints | `generated_retained_checkpoint_restores_every_available_target`: 1 passed. Every available target restored to the expected `MaterializedPosition`; expired targets failed closed. |
| Embedded checkpoint and restart matrix | `retention_checkpoint`: 16 passed. This covered sidecar tamper, position mismatch, all four policy windows, memory/redb/SQLite restart, before-commit and after-commit faults, concurrent append, active pins, retain-all, and a durable-but-unapplied tail. |
| PostgreSQL fixture | 84 passed, 0 failed, 1,268 skipped. All four retention cases passed. The seeded PPSC differential completed in 83.275 seconds. |
| MySQL fixture | 54 passed, 0 failed, 1,298 skipped. All four retention cases passed. The seeded PPSC differential completed in 68.637 seconds. |
| libSQL fixture | The full lane reported 58 passed, 1 failed, and 1,293 skipped. All four retention cases and the seeded PPSC differential passed; PPSC completed in 61.866 seconds. The sole failure was the existing `libsql_replica_post_visibility_ack_loss_forces_crash_and_replay` assertion at `libsql_replica_provider.rs:247`, which also failed at the same assertion on clean-main run `32952327904`, job `98145513617`. A final focused filter selected exactly the four `libsql_provider::retention` tests and exited 0 under the harness's `--no-tests fail` contract. |

An unavailable provider is not counted as a pass. PostgreSQL, MySQL, and
libSQL were all started from the repository fixture, reached readiness, ran
their retention cases, and were stopped and removed by the fixture owner.

## Measured Bounds

The qualification benchmark runs identical indexed update workloads against
retain-all and bounded redb stores. It excludes maintenance time from the
latest-path timer, measures prepare and finalize separately, restores each
PITR archive, checks the restored position, and fails if the bounded
checkpoint or retained tail exceeds its configured window.

Primary steady-state command:

```text
NIMBUS_RETENTION_BENCH_COMMITS=8192 \
NIMBUS_RETENTION_BENCH_WINDOW=512 \
NIMBUS_RETENTION_BENCH_MAINTENANCE_STEP=256 \
cargo bench -p nimbus-storage --bench metadata-retention-baseline
```

Result:

- Retain-all latest-path throughput was 220.59 writes/second; bounded was
  214.56 writes/second, a 2.81% measured overhead.
- Thirty-one maintenance cycles pruned 22,529 journal and MVCC records at
  14,035.24 records/second. Prepare totaled 1,365.327 ms with a 56.904 ms
  maximum; finalize totaled 239.848 ms with a 9.724 ms maximum.
- The materialized checkpoint was 73,757 bytes.
- Retain-all PITR was 13,567,301 bytes with 8,193 tail records, 615.023 ms
  export, and 826.788 ms restore. Bounded PITR was 931,001 bytes with 512 tail
  records, 45.889 ms export, and 55.070 ms restore.
- Retain-all redb reached 34,222,080 bytes. Bounded redb reached 4,747,264
  bytes and plateaued at 4,747,264 bytes after the first steady-state sample.
  Retain-all kept 8,192 document and index versions; bounded kept 768 of each.

A 20,000-commit scale run used a 10,000-sequence document/index/PITR window
and a 1,000-sequence maintenance step. Latest-path overhead was 1.80%; the
bounded archive held exactly 10,000 tail records and measured 16,814,350
bytes. Its 703.070 ms export and 660.131 ms restore stay inside the original
SMR0 gross projection. The checkpoint remained 73,748 bytes because it scales
with live materialized state, not retained history. This sample supports the
100,000-sequence shipped profile's approximate 168 MB journal-tail scale and
does not exceed the original conservative storage envelope.

## Repository And Review Gates

| Command or gate | Result |
| --- | --- |
| `cargo fmt --all --check` and `git diff --check` | Passed. |
| `cargo check -p nimbus-storage --bench metadata-retention-baseline` | Passed. |
| `cargo clippy -p nimbus-storage --bench metadata-retention-baseline -- -D warnings` | Passed; output contained vendored warnings only. |
| `bash scripts/check-docs.sh` | Passed: 109 pages link-clean, source map resolved, private fence intact, and titles unique. |
| `make ci` | Passed with exit 0. This covered workspace format and Clippy, dependency audit, Rust runtime and non-runtime tests, doctests, required verification harness, JavaScript builds, type checks and tests, and proof helpers. The UI lane reported 95 files and 832 tests passed. |
| Nimbus autoreview, final `pre-pr` pass | Clean with no accepted or actionable findings. The review verified the paired timing boundary, maintenance cadence, checkpoint assertion, bounded PITR tail, and restored-position assertion. |
| Hosted PR checks | GitHub Actions did not create jobs for #321 while recent main and PR runs were ending in `startup_failure` and an earlier manual CI run remained queued. The PR was `MERGEABLE` and `CLEAN`, main had no branch protection, and #321 merged without an administrative override after the local required gate and isolated review passed. |

After this proof was added, the contract verifier reported:

```text
Summary: 18 passed, 0 failed
```

## Remaining Boundary

This verdict closes metadata retention only. Blob liveness and reclamation
remain BLI-owned, distributed retention authority remains horizontal-scaling
work, and tenant KV durability remains NKV-owned. `retain-all` continues to be
an explicit unbounded operator choice and needs independent capacity alerts.
