# SQLite Write Overhead And Opportunities — 2026-07

Status: `accepted research; implementation control plane is
../sqlite-write-throughput-optimization-plan.md`

Baseline commit: `e47b64eacc3d54dc5bfe7d51727306a81cfacb28`

Research date: 2026-07-27

## Executive Verdict

Nimbus is not "half as fast as SQLite's 48–50k writes/s." That comparison
mixes unlike units.

The fresh production-topology result is **21,433 durable logical CRUD
mutations/s** at N=256. At the measured average batch of 142.22, that rate
represents approximately:

- 64,299 core row changes/s: one `commit_log` row, one
  `document_versions` row, and one live `documents` change per logical
  mutation;
- another approximately 301 metadata row changes/s;
- approximately 186k application-issued SQL statements/s under the current
  source-counted statement shape;
- 150.7 append/apply batches/s;
- 301.4 SQLite transactions/s and sync-bearing commits/s, because the queued
  path deliberately commits the journal before committing materialization.

SQLite's FAQ number is an old statement-rate example whose point is that many
statements in one transaction amortize durable commit cost. The FAQ now says
modern SQLite does far more than 50k inserts/s and continues to warn that
`synchronous=OFF` trades away power-loss safety. It is not a ceiling for
application transactions or a comparable unit for Nimbus logical mutations.

There is nevertheless material, measured Nimbus overhead worth removing.
With the same build, payloads, durability, six-batch distribution, and
hardware, a clean layered planning run measured:

| Layer | Logical mut/s | 95% CI | CV | Retained from preceding layer |
| --- | ---: | ---: | ---: | ---: |
| Raw one-row SQLite CRUD | 305,895 | 291,708–320,082 | 7.3% | — |
| Current per-record SQL loop, resident connection | 50,358 | 49,688–51,029 | 2.1% | 16.5% of raw; work is much heavier |
| Replay-guarded, prepared, batch-hoisted SQL | 151,485 | 149,141–153,830 | 2.4% | 300.8% of current-loop SQL |
| Nimbus-shaped SQL lower bound | 171,088 | 168,132–174,044 | 2.7% | 112.9% of guarded SQL |
| Production `nimbus-storage` append + apply | 38,810 | 38,319–39,301 | 2.0% | 22.7% of the SQL lower bound |
| Complete Engine, N=256 | 21,433 | 20,753–22,112 | 5.7% | 55.2% of production storage |

The strongest low-risk opportunity is therefore the combination of
**statement reuse and batch-invariant validation hoisting**. The controlled
SQL ablation preserves preimage reads and resource-binding cleanup while
reducing the exact fixture's application-issued statement count from 6,449 to
3,401 and elapsed time from 15.257 ms to 5.073 ms. It is a lower-layer
experiment, not a promise that full Engine throughput will triple.

Second is an **actor-owned resident writer connection**. Current connection
open plus production-equivalent initialization averaged 494.1 microseconds.
The queued six-batch fixture opens twelve writer connections, placing an
approximately 5.93 ms / 30% upper bound on connection churn inside the
19.796 ms production-storage sample. The independent resident-current SQL
lane is 29.8% faster than production storage, although that comparison also
excludes some Rust-side record work.

CPU-only record/document serialization took 1.147 ms per 768-mutation
fixture—about 5.8% of current production-storage elapsed time—so serialization
reuse is real but third in priority. Removing forward-apply preimage guards
and delete-side resource-binding cleanup reduced guarded lower-layer elapsed
by approximately **7–11.5% across accepted planning and independent-audit
runs**. That is worthwhile, but the run-sensitive combined ablation does not
attribute the two mechanisms and omits production's Rust
deserialization/comparison cost. It therefore earns a dedicated higher-proof
experiment after the low-risk work, not an immediate production shortcut.

This evidence supports a conservative end-to-end target whose **mean
same-session paired `F_ref`/`B_ref` ratio is at least 1.40** at N=256, with a
positive lower 95% confidence bound on the paired percentage delta. SWT0
freezes the baseline source commit and protocol as `B_ref`; SWT5 freezes the
exact final source commit as `F_ref`. The final session reruns both immutable
binaries rather than reusing SWT0's numeric result. Six predeclared balanced
block pairs prevent post-hoc pairing or adaptive stopping. The existing 30,000
mean and 28,000 lower-CI values remain absolute `F_ref` floors. This dual gate
is robust to identical production source measuring 21,433 in the historical
run and 25,862 in the independent quiet-host audit. Every N=1, N=32,
contention, latency, memory, WAL, database-size, checkpoint, cold-start, and
correctness gate still applies.
The audit and disposition are recorded in
`../proof/sqlite-write-throughput/independent-audit-remediation.md`.

## Ownership And Provenance

No current plan owns benchmark-driven SQLite write-path optimization.

- `archive/parallel-prepare-serial-commit-plan.md` is the completed predecessor
  that established the three Engine-owned client document mutation routes,
  batching, publication ordering, and crash/replay evidence.
- `architecture-review-2026-07-plan.md` owns findings from its July 6 review;
  it does not contain or own this later SQLite measurement campaign.
- `layered-admission-control-plan.md` owns future admission work, not storage
  efficiency.

The sole new owner is
`../sqlite-write-throughput-optimization-plan.md`. The planning work was
created from clean `origin/main` in:

- branch: `codex/sqlite-write-throughput-plan`;
- worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-sqlite-write-throughput-plan`;
- base: `e47b64eacc3d54dc5bfe7d51727306a81cfacb28`.

The dirty primary checkout was inspected only to preserve its pre-existing
work; it was not modified.

## What Was Measured

### Metric vocabulary

| Metric | Meaning |
| --- | --- |
| Logical mutation/s | One acknowledged Nimbus insert, update, or delete |
| SQL statement/s | Application-issued SQL statements, including transaction control and production connection initialization where stated |
| Row changes/s | Rows actually inserted, updated, or deleted; a zero-row guard `DELETE` is not a row change |
| Transaction/s | SQLite `BEGIN`/`COMMIT` pairs |
| Sync-bearing commit/s | Commits under WAL + `synchronous=FULL`; not inferred to be one physical device flush each |
| Effective batch | Logical mutations included in one queued journal append/apply pair |
| WAL bytes/frames | Fieldwise maximum pre-probe WAL observation across every measured repetition and round in the final harness |

The phrase "writes per second" is not used without one of these qualifiers.

### Fixed environment

| Item | Value |
| --- | --- |
| Hardware | Apple M2 Max, 32 GiB |
| OS | macOS 15.7.2 (24G325), arm64 |
| Rust | rustc/cargo 1.96.1 |
| Workspace SQLite binding | `rusqlite 0.40.1` |
| SQLite sys crate | `libsqlite3-sys 0.38.1` |
| Build | workspace `bundled-sqlcipher-vendored-openssl`, release/bench profile |
| Durability | WAL, `synchronous=FULL`, foreign keys on |
| Page/checkpoint policy | 4 KiB pages, default `wal_autocheckpoint=1000` pages |
| Workload | 256 disjoint documents, phased insert/update/delete, 768 logical mutations |
| Captured batches | `[5, 251, 90, 256, 20, 146]`, mean 128 |
| Layered statistics | 12 measured samples, 60 fresh-database repetitions/sample; clean planning reference only |
| Full Engine statistics | 3 warmups + 15 measured rounds per N |

The batch distribution came from a real saturated N=256 Engine round. The
layered harness replays it exactly. Setup, schema creation, and fixture
construction are outside timed write intervals except that production
`SqliteTenantStore` writer connections intentionally perform their real
per-open initialization inside the measured append/apply calls.

The landed harness fixes its valid 26-character table id across processes so
base and candidate payload bytes and database keys are identical. It also
performs an untimed live-state check after every batch and, after every timed
production repetition, audits exact journal payloads, version contents,
catalog/metadata rows, and empty final live/index/resource state. The clean
planning reference predates those integrity additions and remains
non-acceptance evidence until SWT0 replaces it.

The final harness also retains fieldwise maxima for database bytes, WAL bytes,
page size, WAL frames, passive-checkpointed frames, and the autocheckpoint
threshold across every repetition and round. Resource gates therefore cannot
silently inherit one arbitrary last sample.

### Layer definitions

1. **Raw row mutation** uses the real workspace rusqlite/SQLCipher build and
   identical record blobs, but changes one row per logical CRUD operation in
   six FULL transactions.
2. **Current-loop SQL, resident connection** reproduces the current
   per-record metadata/schema/table/preimage query shape, version/live/journal
   data shape, twelve FULL transactions, and non-cached execution on one
   already initialized connection. This isolates current SQL-loop cost from
   connection churn and higher-level Rust work.
3. **Guarded prepared/hoisted SQL** preserves the preimage and
   resource-binding guards, caches recurring statements, and performs
   format/schema/table-identity checks once per batch.
4. **Nimbus-shaped SQL lower bound** uses the same durable row and transaction
   shape with cached statements, while omitting replay-preimage and
   resource-binding guard queries. It is a diagnostic floor, not an authorized
   production design.
5. **Production storage** calls
   `SqliteTenantStore::append_durable_records_batch` followed by
   `apply_durable_records_batch` for every captured batch.
6. **Complete Engine** uses the existing concurrent write-throughput harness,
   including preparation, conflict handling, journal assignment, ordered
   publication, cache invalidation, and fan-out.

Within each layered run, lanes execute in the fixed order
raw → resident-current → guarded → lower-bound → production storage. The CV
gate rejects visibly unstable complete runs, but fixed ordering means host
drift can still map onto lanes and is one reason same-session candidate/base
alternation remains mandatory for acceptance.

All 256 fixture updates change `rank`; none is a no-op. The statement and
row-change census depends on that property. The layered SQL append loops also
receive precomputed MessagePack records, so their timed regions omit record
encoding; the separate CPU serialization lane prices that work.

## Baselines And Planning References

### Complete Engine

Command and raw output are recorded in
`../proof/sqlite-write-throughput/full-engine-baseline.md`.

| N | Mean logical mut/s | 95% CI | Median | CV | p50/p95/p99 µs | Avg batch |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1,711 | 1,682–1,740 | 1,727 | 3.1% | 567.2 / 705.6 / 867.2 | 1.00 |
| 32 | 13,510 | 13,273–13,748 | 13,652 | 3.2% | 2,289.5 / 3,334.5 / 4,929.2 | 16.34 |
| 256 | 21,433 | 20,753–22,112 | 21,920 | 5.7% | 11,085.2 / 19,150.1 / 23,993.2 | 142.22 |

N=256 measured commit-phase shares:

| Phase | Share |
| --- | ---: |
| prepare / plan CPU | 26.0% |
| conflict and assignment checks | 4.6% |
| storage apply plus Engine publication | 51.8% |
| first durable journal append | 17.6% |

The last label is easy to misread. `append_durable_records_batch` commits one
FULL transaction, then `apply_durable_records_batch` commits a second FULL
transaction. The second sync is inside the reported `apply` time. Therefore
17.6% is not the total durability-sync share.

### Exact layered planning reference

The complete clean report and samples are in
`../proof/sqlite-write-throughput/layered-planning-reference.md`. The report's
exact producing executable was overwritten before its hash was captured, so
these values rank opportunities but are not an acceptance or A/B baseline.
Five post-review reruns from a superseded review-pass executable and one run
from the final hardened executable were rejected whole for host noise;
`../proof/sqlite-write-throughput/layered-review-reruns.md` records their
complete reports and hashes. SWT0 must capture a cryptographically bound
same-session replacement before an implementation candidate can be accepted.

| Lane | Logical mut/s | SQL stmt/s | Row changes/s | Tx/s | Sync commits/s | Mean elapsed |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Raw row mutation | 305,895 | 310,675 | 305,895 | 2,389.8 | 2,389.8 | 2.523 ms |
| Current-loop SQL, resident connection | 50,358 | 422,866 | 151,862 | 786.8 | 786.8 | 15.257 ms |
| Guarded prepared/hoisted SQL | 151,485 | 670,836 | 456,823 | 2,367.0 | 2,367.0 | 5.073 ms |
| Nimbus-shaped SQL lower bound | 171,088 | 523,957 | 515,937 | 2,673.3 | 2,673.3 | 4.492 ms |
| Production storage append+apply | 38,810 | 337,567 | 117,138 | 606.4 | 606.4 | 19.796 ms |

Higher statement/s in the guarded lane is not more work: it finishes a fixed
3,401-statement fixture three times faster than the current-loop lane's
6,449-statement fixture.

Every per-second column is derived from the same mean logical-fixture
throughput. The planning report's transaction-rate cells were recomputed from
that estimator after review; the measured logical rates and raw samples did
not change.

Source-counted production statements for the fixed 768/6 fixture:

| Component | Statements |
| --- | ---: |
| Journal append work, including control | 793 |
| Apply work, including repeated reads/checks and control | 5,659 |
| Twelve fresh connections × 3 PRAGMAs + 16 idempotent DDL statements | 228 |
| **Total** | **6,680** |

`busy_timeout` is a C API call and is not counted. SQLite-internal statements
are not counted. The production fixture changes 2,318 rows: 768 journal,
768 version, 768 live-document, twelve next/applied metadata, one
document-version format, and one table-catalog row.

### WAL and bytes

| Lane | DB bytes before probe | WAL bytes before probe | Frames | Passive-checkpointed frames | Autocheckpoint |
| --- | ---: | ---: | ---: | ---: | ---: |
| Raw | 4,096 | 700,432 | 170 | 170 | 1,000 pages |
| Current-loop resident | 4,096 | 1,404,952 | 341 | 341 | 1,000 pages |
| Guarded prepared/hoisted | 4,096 | 1,404,952 | 341 | 341 | 1,000 pages |
| SQL lower bound | 4,096 | 1,404,952 | 341 | 341 | 1,000 pages |
| Production storage | 4,096 | 1,396,712 | 339 | 339 | 1,000 pages |

All Nimbus-shaped lanes produce essentially the same durable byte shape.
None reaches the 1,000-page automatic-checkpoint threshold. The passive
checkpoint was issued only after size/frame capture. There is no evidence in
this **768-mutation layered fixture** that checkpoint policy is the current
bottleneck. That result does not settle Engine-scale behavior: an N=256 Engine
round writes enough WAL to cross the fixture's scale and may include automatic
checkpoint work. SWT0 therefore captures Engine-scale WAL high-water frames,
checkpoint counts, and checkpoint time before the campaign accepts or rejects
checkpoint tuning globally.

### Connection and CPU-only preparation

| Operation | Mean |
| --- | ---: |
| `Connection::open` only | 45.2 µs |
| Open + production-equivalent PRAGMA/DDL initialization | 494.1 µs |
| `SqliteTenantStore::open` + initial schema load | 494.2 µs |
| Record MessagePack + current document JSON/typed-field encoding | 1.147 ms per 768 mutations |
| Same CPU serialization as a rate | 669,480 logical mutations/s |

The serialization lane performs no SQLite I/O and is not a durable throughput
number.

## Complete Client Document Mutation Paths

Client document mutations remain Engine-owned. There are exactly three routes.
This scoped invariant does not cover every storage writer:

- schema, scheduler, trigger, and point-in-time-restore operations can run as
  internal committer jobs;
- object manifests write documents and commit-log entries through
  `TenantPointWrite` on the read executor, outside client-mutation committer
  routing and write-log staging;
- libSQL replica refresh reconciles durable records into its local cache
  through a storage-owned write transaction.

Those surfaces are not fourth client document mutation routes, but they are
real concurrent-writer constraints for any connection-residency design.

### Queued journal path

1. Async insert/update/delete enters
   `apply_mutation_with_mode_async_cancellable` and submits to the bounded
   per-tenant mutation journal.
2. Parallel preparation resolves validation, authorization, schema, reads,
   conflict dependencies, and candidate writes outside the serial assign step.
3. The journal worker adaptively drains pending requests. Serial assignment
   reparses/reprepares stale work when conflict-mediated checks require it,
   assigns a dense sequence and commit timestamp, builds `TenantEventRecord`s,
   and stages the pending write log. Schema validation belongs to initial
   preparation and any such reprepare; it is not an unconditional serial-step
   check.
4. The embedded ordered publisher calls
   `append_durable_records_batch`. SQLite opens and fully initializes a writer
   connection, begins IMMEDIATE, reads the next sequence, MessagePack-serializes
   and inserts every journal row, updates `next_sequence`, and commits.
5. Only after durable append, it calls `apply_durable_records_batch`. SQLite
   opens and fully initializes another writer connection, begins IMMEDIATE,
   reads `applied_sequence`, validates and applies every record, writes
   versions/index versions/live state, updates `applied_sequence`, and commits.
6. The Engine publishes the staged write log, invalidates caches, advances
   the applied head, and only then releases subscription/fan-out work.
7. Ambiguous apply outcomes recover from the durable journal. The serial
   kill-switch follows the same append-before-apply ordering.

The queued path is intentionally two SQLite transactions per batch. This
research does not authorize collapsing them.

### Direct path

Synchronous `apply_mutation_with_mode*` validates schema while preparing the
mutation, passes authorization/capability/rate admission, and enters the
per-tenant direct committer. `run_prepared_direct_mutation` performs
conflict-mediated revalidation/re-detection, assigns the dense
sequence/timestamp, stages publication, and calls
`persist_prepared_write_batch`. It does not move the full schema-validation
operation uniformly into the serial section.

For embedded SQLite, `apply_prepared_write_batch` uses one
`execute_write` transaction. In that transaction it applies document,
document-version, index-version, and schedule effects, sets the prepared
record, appends the commit log, and updates both next/applied watermarks before
COMMIT. It is one FULL SQLite transaction per logical direct mutation, not the
queued path's two-transaction batch topology.

### `MutationExecutionUnit` path

`begin_mutation_execution_unit` pins the read snapshot, applied sequence, and
schema. Runtime host operations stage reads and writes. `commit` resolves the
write set, durable event record, schedule operations, and read dependencies
outside the serial closure, then validates conflicts/schema and assigns the
dense sequence/timestamp inside the committer.

Document-writing units call the same embedded
`persist_prepared_write_batch`/`apply_prepared_write_batch` seam as the direct
route and commit as one atomic SQLite transaction for the whole function
invocation. Schedule-only units use their schedule seam. Publication,
cache/applied-head updates, and fan-out occur only after storage success.

### Per-record SQLite apply work

For the schemaless one-write records in the benchmark, current queued apply
performs:

- a document-version format metadata read for every record;
- a document-version insert;
- a table-schema read for index-version planning, even when no schema/index
  exists;
- hidden and active table-identity catalog reads for every record;
- a live-document preimage read for every record;
- current-document JSON and typed-field serialization for version storage,
  then again for live storage;
- the live insert/update/delete;
- an unconditional resource-binding delete for delete records;
- batch-level applied-watermark update and commit.

This census explains why the apply phase dominates despite group-committed
fsyncs.

## External Context

### SQLite

The official SQLite FAQ says the historical 50k figure is INSERT statements
and explains that transaction rate, not statement execution, was the limiting
factor in its old disk example. Its 2024 update says current SQLite does far
more, while preserving the batching lesson:
<https://www.sqlite.org/faq.html#q19>.

SQLite's WAL documentation states that there is still only one writer at a
time, that FULL synchronous mode syncs the WAL on each transaction commit,
and that the default automatic checkpoint occurs at 1,000 pages:
<https://www.sqlite.org/wal.html>.

A Turso benchmark reports about 150k inserted rows/s at FULL with 100
inserts/transaction and higher figures with larger batches. That is useful
context for statement/row throughput, not a target for Nimbus logical
mutations because the data model and semantics differ:
<https://turso.tech/blog/beyond-the-single-writer-limitation-with-tursos-concurrent-writes>.

### Self-hosted Convex

Local source checkout:
`/Users/jack/src/github.com/get-convex/convex-backend` at `21219db1`.

The local source confirms that self-hosted Convex:

- owns one persistent SQLite connection behind `Arc<Mutex<Inner>>`;
- uses `prepare_cached` for repeated document/index writes;
- commits revision/index effects in one SQLite transaction;
- performs tracking/serialization/persistence work outside the central
  committer while awaiting ordered futures for publication.

Convex's representation and transaction topology are not Nimbus's:
self-hosted Convex does not maintain Nimbus's separate journal +
document-version + live-document trio, and Nimbus group-commits multiple user
mutations.

SpacetimeDB's published `keynote-2` comparison reports 3,121 Node/Drizzle/
SQLite transfers/s and 1,140 self-hosted Convex transfers/s on its stated
host. That is 36.5% retained / 2.74× slower, but each transfer reads and
patches two accounts, and the hardware, workload, transaction shape, and
publication semantics differ. It is context only, not a Nimbus-vs-Convex
product benchmark:
<https://github.com/clockworklabs/SpacetimeDB/tree/master/templates/keynote-2#results-summary>.

## Ranked Opportunity Ledger

| Rank | Candidate | Measurement | Expected production mechanism | Implementation risk | Correctness risk | Effort | Verdict |
| ---: | --- | --- | --- | --- | --- | --- | --- |
| 1 | Cache/prepare batch statements and hoist format/schema/table invariants | Guarded lane: 50,358 → 151,485 mut/s; 6,449 → 3,401 statements; 15.257 → 5.073 ms | Avoid repeated parse/finalize and 3,048 redundant fixture statements while retaining replay guards | low–medium | low if invalidation boundaries are explicit | medium | **Execute first** |
| 2 | Actor-owned resident writer connection; retain both queued transaction boundaries | 494.1 µs initialized open; twelve opens ≈5.93 ms; resident-current lane is 29.8% above production storage | Remove repeated open, PRAGMA, 16-statement idempotent schema init, and encrypted key verification; retain statement cache | medium | low–medium: poison/reopen/error recovery and concurrency ownership | medium | **Execute second** |
| 3 | Serialize current document/record once and reuse encoded forms | 1.147 ms / fixture, 5.8% of production storage elapsed ceiling | Move deterministic encoding off ordered apply and avoid duplicate JSON/typed-field work | medium | low if encoded payload is internal and integrity hash semantics stay unchanged | medium | **Measure after 1–2; execute only if ≥3%** |
| 4 | Forward-apply conditional write without full preimage deserialization | Guarded → lower-bound saves approximately 7–11.5% of guarded lower-layer elapsed across accepted planning/audit runs; combined ablation also omits delete binding cleanup and does not include full Rust deserialization | First isolate preimage vs binding cost, then use affected-row/hash/sequence guard on forward apply while retaining full recovery validation | high | high: corruption/idempotent replay detection | high | **Worthwhile; execute as a dedicated evidence-gated phase after low-risk work** |
| 5 | Checkpoint tuning | Layered fixture: 337–341 frames, below 1,000-page autocheckpoint; Engine N=256 crosses that WAL scale | Capture Engine-scale checkpoint counts/time before deciding whether foreground scheduling matters | medium | medium operational risk | medium | **Defer pending SWT0 Engine-scale evidence** |
| 6 | More aggressive queued batching | Current N=256 average batch 142.22; first append only 17.6% while apply is 51.8% | Further amortize two commits/connection setup | medium | medium latency/fairness risk | medium | **Defer until lower-risk apply work lands** |
| 7 | Collapse append and apply transaction | Would halve queued transaction count | Remove one commit per batch | high | **unacceptable without redesigning crash/replay contract** | high | **Reject** |
| 8 | Remove journal, versions, indexes, validation, publication, or weaken FULL/WAL | Produces misleading benchmark wins | Deletes product semantics or durability | low code effort | **unacceptable** | irrelevant | **Reject** |

### Candidate 1 boundaries

Batch hoisting must key format validation to the storage format and
`(table, table_id)` identity actually present in the batch. Schema/index
planning may be deduplicated per table only while schema mutation ordering
within the batch remains explicit. Prepared statements must not outlive a
connection or silently ignore `SQLITE_SCHEMA` invalidation.

The first implementation PR should retain:

- every journal/version/live/index row;
- every preimage and integrity comparison;
- resource-binding cleanup;
- append-before-apply ordering;
- FULL/WAL;
- all three client document mutation routes.

### Candidate 2 boundaries

Writer residency is connection reuse, not a concurrency redesign. A single
per-tenant writer owner may serve queued, direct, and execution-unit commits
under the existing Engine commit serialization. It is not SQLite's sole
writer: object-manifest writes and libSQL replica-cache reconciliation can
open storage-owned write transactions outside those client routes. The owner
must preserve busy/locked classification, bounded progress, and recovery while
coexisting with them. The queued publisher still executes:

1. append transaction COMMIT;
2. apply transaction COMMIT;
3. Engine publication.

Connection initialization must split one-time file/schema setup from
per-connection safety setup without weakening encrypted open/key verification.
Fatal/poisoned connection behavior must be explicit and tested.

### Candidate 3 boundaries

Encoded document fields may be carried as an internal prepared persistence
payload, but `TenantEventRecord` integrity and storage encoding remain
canonical. No storage-format change is justified by the current measurement.
Do not keep parallel JSON representations beyond the commit lifetime.

## Rejected Benchmark Cheats

The following cannot satisfy any performance gate:

- `synchronous=NORMAL` or `OFF`;
- disabling WAL or durable acknowledgements;
- omitting the journal, MVCC document versions, live state, maintained indexes,
  authorization, validation, integrity checks, publication, or cache/fan-out
  semantics;
- a benchmark-only production fast path;
- treating one raw row change as one Nimbus logical mutation;
- comparing numbers from different bindings, payloads, batches, builds, or
  hosts as an A/B result;
- accepting a noisy sample set with CV above 10%;
- improving the mean while confidence intervals, latency, memory, WAL,
  database size, cold start, contention, or correctness regress materially.

## Target Derivation

Historical N=256 observation: 21,433 logical mut/s.

Independent quiet-host audit of identical production source: 25,862 logical
mut/s. Its raw scratch report was not transferred into this branch, so this is
diagnostic host-drift evidence rather than an acceptance baseline.

SWT0 first merges every checkpoint/resource observation seam needed by final
acceptance, then freezes the resulting source commit and canonical protocol as
`B_ref` and records one diagnostic reference run. The final session rebuilds
and reruns `B_ref` alongside an exact post-optimization commit `F_ref`. Both
commits and binary hashes are recorded before sampling and remain immutable
for the whole session.

Required relative gain: mean of six predeclared, balanced, same-session paired
`F_ref`/`B_ref` N=256 throughput ratios ≥1.40. The paired Student-t interval is
computed over those six adjacent block deltas, never over a post-hoc pairing
of per-round samples.

Required `F_ref` mean: at least 30,000 logical mut/s.

Required uncertainty gates: the lower 95% confidence bound of the paired
percentage delta is positive, and the `F_ref` absolute lower 95% confidence
bound is at least 28,000 logical mut/s.

Why this is ambitious but credible:

- the guarded statement/invariant ablation removes 66.8% of the current-loop
  SQL elapsed time without removing replay guards;
- writer initialization is independently measured at up to 30% of production
  storage elapsed time;
- production storage is 1.81× faster than the complete Engine, leaving a
  known translation tax but still substantial headroom;
- the 30k absolute floor is 19.8% of the guarded lane's 151,485 logical
  mut/s; expressed in matching physical units, its approximately 90k core row
  changes/s is also about 19.7% of that lane's 456,823 row changes/s;
- no durability, transaction, journal, MVCC, index, or publication semantic
  change is required by the target.

The contemporaneous paired 40% requirement preserves the intended
implementation improvement across host-state drift; the 30k/28k floors prevent
a uniformly depressed session from weakening it. This target is not a capacity
promise across machines. It is the local campaign acceptance gate. Publishable
capacity decisions still require pinned server-class hardware and an open-loop
latency companion.

## Reproduction

Compile the two release benchmarks:

```bash
timeout 900 env \
  CARGO_TARGET_DIR=/Users/jack/src/github.com/nimbus/nimbus/target \
  cargo bench -p nimbus-engine --bench concurrent-write-throughput --no-run

timeout 900 env \
  CARGO_TARGET_DIR=/Users/jack/src/github.com/nimbus/nimbus/target \
  cargo bench -p nimbus-engine --bench sqlite-write-overhead --no-run
```

Full Engine:

```bash
timeout 600 env \
  NIMBUS_CWB_WORKLOAD=crud \
  NIMBUS_CWB_LADDER=1,32,256 \
  NIMBUS_CWB_OPS_PER_WORKER=100 \
  NIMBUS_CWB_MAX_MUTATIONS_PER_ROUND=9000 \
  NIMBUS_CWB_MEASURE_ROUNDS=15 \
  NIMBUS_CWB_WARMUP_ROUNDS=3 \
  NIMBUS_CWB_SPLIT_PHASES=1 \
  NIMBUS_CWB_OUT=/tmp/sqlite-write-overhead-cwb.md \
  <compiled-concurrent-write-throughput-binary>
```

Layered:

```bash
timeout 600 env \
  NIMBUS_SWO_ROUNDS=12 \
  NIMBUS_SWO_REPETITIONS_PER_SAMPLE=60 \
  NIMBUS_SWO_OUT=/tmp/sqlite-write-overhead-layered.md \
  <compiled-sqlite-write-overhead-binary>
```

Record the exact binary hash, commit, dirty state, OS/hardware, raw samples,
and output hash with each run. A candidate is not accepted from a historical
absolute comparison; benchmark base and candidate in the same quiet session,
alternating order where practical.

## Limitations

- The M2 Max local protocol is excellent for architectural A/B attribution,
  not a cross-machine capacity guarantee.
- The layered SQL lanes reproduce statement/data/transaction shape but do not
  execute all Engine validation, authorization, conflict, publication, or
  Rust-side preimage comparison work. Only the production storage and complete
  Engine lanes measure those owners.
- Their fixture is schemaless, has no maintained secondary index, and carries
  one document write per durable record. Indexed, multi-write, and
  schema-bearing behavior is protected by the campaign's Engine correctness
  and regression gates rather than this lower-layer throughput fixture.
- MessagePack records are precomputed outside the layered SQL timed append
  loops; the CPU serialization lane measures that omitted work separately.
- Layered checkpoint evidence is fixture-scale. Full-Engine retention ratios
  can include automatic checkpoint I/O and store aging until SWT0 records
  Engine-scale counters.
- The raw lane intentionally has one physical row change and one transaction
  per captured batch; it is a device/library control, not an application peer.
- The statement count is source-derived for the fixed fixture and excludes
  SQLite internals. Future SQL changes must update or trace-verify it.
- The full Engine batch distribution varies naturally; the layered replay
  freezes one observed distribution so candidate comparisons are controlled.
- Closed-loop saturated latency has coordinated omission and is not an SLA.
  The plan requires a below-saturation open-loop companion before publishing a
  service-latency claim.
