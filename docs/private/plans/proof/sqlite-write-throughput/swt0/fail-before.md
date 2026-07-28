# SWT0.1 Deterministic Fail-Before And Checkpoint Evidence

Date: 2026-07-28

Scope: SWT0.1 observability only. Production SQL, transaction boundaries,
journal ordering, durability, concurrency, and storage formats are unchanged.
The local implementation commits are:

- `1c9858984` — test-only SQLite write counters plus the resettable
  WAL/checkpoint seam;
- `5ca8a3988` — opt-in Engine benchmark reporting;
- `dff400fad` — complete indexed CRUD concept coverage and independently
  incrementable prepare/execute counters;
- `ab8a87ed3` — concept-owned observability test module, keeping its parent
  below the repository modularity threshold.

No SWT0.2 baseline was frozen or run.

## Deterministic queued-batch evidence

Test:
`tests::sqlite_foundation::journal::observability::sqlite_queued_batch_fail_before_observes_repeated_write_work`

The test resets a path-scoped, mutex-protected counter set immediately before
the current queued append/apply pair. A one-record schemaless insert records:

| Counter | Observed |
| --- | ---: |
| writer opens | 2 |
| document-version format checks | 1 |
| index-schema checks | 1 |
| table-identity checks | 1 |
| current-document encodes | 2 |

The two writer opens are the durable journal append writer and the separate
materialized apply writer. The two current-document encodes are the
document-version projection and the live-document projection.

The same test then uses an indexed insert/update/delete batch to prove the
counter coverage beside both version owners:

| Counter | Observed |
| --- | ---: |
| writer opens | 2 |
| document-version format checks | 3 |
| index-schema checks | 3 |
| table-identity checks | 3 |
| current-document encodes | 4 |

The four encodes are two projections each for the insert and update. Delete
has no current document.

For the indexed three-record batch, prepare and execute counters both record
the following exact successful-path counts:

| Statement concept | Prepares | Executes |
| --- | ---: | ---: |
| journal next-sequence read | 1 | 1 |
| journal insert | 3 | 3 |
| next-sequence metadata write | 1 | 1 |
| applied-sequence read | 1 | 1 |
| applied-sequence metadata write | 1 | 1 |
| durable-record reread | 0 | 0 |
| document-version format read | 3 | 3 |
| document-version format write | 1 | 1 |
| document-version insert/tombstone | 3 | 3 |
| index-schema read | 3 | 3 |
| index-version format read | 3 | 3 |
| index-version format write | 1 | 1 |
| index-version close | 2 | 2 |
| index-version open | 2 | 2 |
| table-identity check | 3 | 3 |
| document preimage read | 3 | 3 |
| live-document insert | 1 | 1 |
| live-document update | 1 | 1 |
| live-document delete | 1 | 1 |
| resource-binding upsert | 0 | 0 |
| resource-binding delete | 1 | 1 |

Both the schemaless and indexed observations are reset to an all-zero snapshot
inside the test. The counter target is one exact SQLite path, so unrelated
parallel SQLite tests do not contribute.

These are fail-before values for later optimization tasks: SWT1 must preserve
execute semantics while reducing recurring prepares/checks, and SWT2 must
remove recurring writer opens without combining the queued append and apply
transactions.

## WAL/checkpoint seam

The resettable seam is compiled only under `cfg(test)` or the existing
`test-hooks` feature. It observes each successful foreground COMMIT with
SQLite's read-only `PRAGMA wal_checkpoint(NOOP)`, records WAL/checkpointed
high-water frames, and classifies a foreground automatic-checkpoint event when
the post-COMMIT frame count reaches that connection's `wal_autocheckpoint`
threshold. SQLite does not expose checkpoint-only COMMIT time, so the recorded
time for such a commit is explicitly an upper bound.

`probe_sqlite_passive_checkpoint` is a separate, explicit post-run
`PRAGMA wal_checkpoint(PASSIVE)` operation with separate count, timing, busy,
WAL-frame, and checkpointed-frame fields. The focused unit test proves that
running it does not increment foreground or automatic-checkpoint counters.
Observation errors after COMMIT are counted and swallowed so diagnostics cannot
turn a durable success into an ambiguous operation result.

## Engine-scale N=256 CRUD diagnostic

This was an observation-only release run, not an acceptance throughput sample:
one measured round, no warmup, four CRUD units per worker. `ladder()` forces the
N=1 anchor, so the run contains N=1 and N=256. The command was:

```bash
timeout 600 env \
  NIMBUS_CWB_WORKLOAD=crud \
  NIMBUS_CWB_LADDER=256 \
  NIMBUS_CWB_OPS_PER_WORKER=4 \
  NIMBUS_CWB_MAX_MUTATIONS_PER_ROUND=3072 \
  NIMBUS_CWB_MEASURE_ROUNDS=1 \
  NIMBUS_CWB_WARMUP_ROUNDS=0 \
  NIMBUS_CWB_WAL_CHECKPOINT_OBSERVATION=1 \
  NIMBUS_CWB_OUT=/tmp/swt0-engine-wal.md \
  cargo bench -p nimbus-engine --bench concurrent-write-throughput
```

Report SHA-256:
`1c333a7b5bd327eae0629faeef0e9e1b389311901dd97459ac7324357f826d10`

Benchmark binary SHA-256:
`3ebe026f77db4b64d124dd9ac072aab9d02deb564c33396a751f1fdd4bb0fb87`

The diagnostic binary was built from `5ca8a3988`. The later `dff400fad`
follow-up changes only `cfg(test)` counter APIs/assertions and is not compiled
into the benchmark.

| N | Foreground commits | Automatic checkpoints | Automatic COMMIT upper bound | WAL high water | Checkpointed high water | NOOP probes / time | Probe share of measured wall | Post-run PASSIVE busy/log/checkpointed / time |
| ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: | --- |
| 1 | 24 | 0 | 0 ms | 145 frames | 0 frames | 24 / 0.108 ms | 0.293% | 0/145/145 / 0.605 ms |
| 256 | 48 | 1 | 11.275 ms | 1,024 frames | 1,024 frames | 48 / 0.366 ms | 0.157% | 0/516/516 / 3.255 ms |

This proves the Engine CRUD path crosses the 1,000-page automatic-checkpoint
scale and that foreground work is distinguishable from the post-run passive
probe. It does not authorize checkpoint tuning or replace SWT0.2's quiet-host
baseline protocol.

Checkpoint observation is disabled by default. Canonical timed benchmark runs
do not execute the NOOP or PASSIVE probes, and test-only statement tracing is
not compiled into the benchmark. Normal release builds do not enable
`nimbus-storage/test-hooks`, so the WAL observer and its post-COMMIT call sites
are absent entirely.

## Verification

The literal task-card nextest command first ran 73 tests: 72 passed and the
libSQL fixture precondition failed because this shell had neither
`NIMBUS_LIBSQL_URL` nor `NIMBUS_LIBSQL_ADMIN_URL`. No test assertion failed.
Following the repository's documented ordinary-lane fixture contract, the
same filter was rerun with
`NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1`.

| Command | Result |
| --- | --- |
| `NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1 timeout 300 cargo nextest run -p nimbus-storage -E 'test(sqlite)'` | 73 passed, 0 failed, 354 skipped |
| `cargo fmt --all --check` | passed |
| `timeout 600 cargo clippy -p nimbus-storage --all-targets -- -D warnings` | passed |
| `timeout 300 cargo check --release -p nimbus-storage --lib` | passed without `test-hooks` |
| `timeout 600 cargo check -p nimbus-engine --bench concurrent-write-throughput` | passed |
| `timeout 600 cargo clippy -p nimbus-engine --bench concurrent-write-throughput -- -D warnings` | passed; dependency warnings remained capped and the target emitted no denied warning |

## Invariant audit

- Queued journal append COMMIT and materialized apply COMMIT remain separate.
- Direct and `MutationExecutionUnit` writes still use the existing
  `SqliteWriteTransaction` COMMIT path.
- Document write, document/index version effects, live row effects, resource
  binding effects, and commit-log append remain in their existing storage
  transactions.
- Preimage validation, table identity, schema validation, serialization,
  storage-format validation, and resource-binding cleanup are observed but not
  weakened or bypassed.
- No SQL text used by production writes changed. The only new pragmas are the
  opt-in post-COMMIT NOOP diagnostic and the explicit post-run passive probe.
- Counter state is thread-safe, resettable, path-scoped, and test-only.
- No production dependency, migration, storage-format, concurrency, or
  durability change was introduced.

## Orchestrator review addendum

An independent orchestrator review initially suspected the foreground
`PRAGMA wal_checkpoint(NOOP)` probe of degrading to a real PASSIVE
checkpoint, because SQLite builds that predate the NOOP keyword parse unknown
modes as PASSIVE (verified empirically on the older system sqlite3). The
workspace's bundled SQLite 3.51.3 parses `noop` to `SQLITE_CHECKPOINT_NOOP`
("do no work at all"; sqlite3.c pragma parser and sqlite3.h constant), so the
probe is status-only on the pinned runtime and the seam description above is
accurate.

That version-skew hazard is now pinned by a regression test instead of an
assumption: `sqlite_wal_observation_probe_does_not_checkpoint` drives a
sub-threshold observed workload and asserts the main database file is
untouched with the whole backlog still checkpointable afterward. Any future
runtime whose NOOP silently checkpoints fails the SQLite lane loudly.

With that guard added, the task-card SQLite filter passes
`74 passed, 0 failed, 354 skipped` under
`NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1`, and
`cargo clippy -p nimbus-storage --all-targets -- -D warnings` remains clean.

## Structured review findings and fixes

The orchestrator's structured autoreview (Codex `gpt-5.6-sol`, high reasoning;
the Claude engine was unavailable on this host because the isolated reviewer
subprocess lacks the local egress proxy's CA trust) accepted three findings,
all fixed on this branch:

1. **Failed foreground probes were invisible in the benchmark report.** The
   WAL observation table now has a `probe errors` column and the section text
   declares a nonzero count to invalidate that rung's diagnostic.
2. **Process-global observer state raced under plain `cargo test`.** All three
   observability tests now share `#[serial_test::serial(sqlite_write_observation)]`;
   nextest's process-per-test model was already safe.
3. **The NOOP guard's file-length assertion was not a sound discriminator**
   (a degraded PASSIVE probe can rewrite already-allocated pages without
   growing the file). The guard now also asserts zero probe errors and
   `checkpointed_high_water_frames == 0` across the observed run, which a
   PASSIVE-degraded probe cannot satisfy on a fresh sub-threshold store.

After the fixes: `cargo fmt --all --check` passes; the task-card SQLite filter
passes `74 passed, 0 failed` under
`NIMBUS_DISABLE_IMPLICIT_EXTERNAL_PROVIDER_FIXTURES=1`;
`cargo clippy -p nimbus-storage --all-targets -- -D warnings` is clean; and the
Engine benchmark compiles in the bench profile.

A second review pass accepted one further finding: post-COMMIT sampling runs
after SQLite releases the writer lock, so per-commit attribution of the
`automatic checkpoints` columns is not provable at the storage layer. The
canonical benchmark workloads keep attribution sound in practice because the
per-tenant committer serializes writers end-to-end and SQLite runs automatic
checkpoints inside the committing connection's own COMMIT; concurrent
non-committer writers (object manifests, replica reconciliation) could shift
a sample onto an adjacent commit. The snapshot fields and the benchmark
section now define these columns as sampled aggregate WAL state under that
stated serialization assumption rather than exact per-commit attribution.
