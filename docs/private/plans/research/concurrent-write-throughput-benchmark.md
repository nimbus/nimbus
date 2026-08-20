# Concurrent write-throughput benchmark — group-commit ceiling

Records the design and results of `crates/nimbus-engine/benches/concurrent-write-throughput.rs`,
a closed-loop concurrency sweep that measures the per-tenant mutation journal's
**group-commit** throughput. It exists because the embedded-provider CRUD
benchmark is sequential (one mutation awaited at a time → batch size 1) and never
exercises the journal worker's coalescing of concurrently-queued mutations into
a single fsync (base `MUTATION_JOURNAL_BATCH_SIZE` = 32, growing adaptively
under backlog up to `NIMBUS_MUTATION_JOURNAL_BATCH_MAX`, default 256).

## Methodology

Canonical closed-loop concurrency sweep (see the bench module doc comment for the
full rationale and references — Little's Law, Gunther's USL, Gil Tene on
coordinated omission, Criterion/JMH conventions):

- **Closed-loop, single tenant.** A fixed pool of N worker tasks, each in a tight
  `{mutation → await durable ack → repeat}` loop, all driving ONE tenant (group
  commit coalesces per tenant, so this loads one journal worker).
- **Geometric ladder** `N ∈ {1,2,4,8,16,24,32,48,64,96,128,192,256}` — swept to
  locate the knee and plateau, not a single guessed concurrency.
- **N=1 is the sequential anchor.** The N=1 rung is by definition the
  one-op-at-a-time path (batch size 1); every higher rung is reported as a
  speedup `S(N) = X(N)/X(1)`, so the group-commit payoff is a workload-independent
  multiple. The default CRUD workload replays the sequential CRUD baseline's shape
  (schemaless `tasks` table, phased insert→update→delete over 300 docs, same
  fields, no pre-seed), so N=1 lands near the published ~2,661 mut/s figure as a
  cross-check (not bit-identical — a separate harness).
- **Little's Law check per rung.** `N ≈ throughput × mean-latency` must reproduce
  N; a rung where it fails flags a measurement bug.
- **Statistics** match the embedded-provider harness house style: warmup rounds
  discarded, then measured rounds; mean + median throughput, a two-sided Student-t
  95% CI on the round means, coefficient of variation (CV).
- **Latency caveat.** Reported p50/p95/p99 are closed-loop (queue) latencies —
  under coordinated omission at saturated rungs they are not SLA service times.

## Results (SQLite, single-host macOS M2-class laptop, 15 rounds, phased CRUD)

| N | throughput mut/s (mean) | 95% CI | speedup | p50 µs | CV% | N≈X·R |
|---|---|---|---|---|---|---|
| 1 | 1,604 | [1,545, 1,662] | 1.00× | 580 | 6.6 | 1.0 |
| 2 | 2,519 | [2,463, 2,574] | 1.57× | 659 | 4.0 | 2.0 |
| 4 | 3,467 | [3,256, 3,678] | 2.16× | 1,121 | 11.0 | 4.0 |
| 8 | 6,929 | [6,563, 7,295] | 4.32× | 926 | 9.5 | 8.0 |
| 16 | 12,029 | [11,786, 12,272] | 7.50× | 1,165 | 3.6 | 16.0 |
| 24 | 14,227 | [13,884, 14,570] | 8.87× | 1,488 | 4.4 | 24.0 |
| 32 | 15,874 | [15,654, 16,094] | 9.90× | 1,811 | 2.5 | 31.9 |
| 48 | 15,160 | [14,447, 15,872] | 9.45× | 2,993 | 8.5 | 48.2 |
| 64 | 16,484 | [16,042, 16,926] | 10.28× | 3,575 | 4.8 | 64.1 |
| 96 | 16,668 | [16,474, 16,862] | 10.39× | 5,399 | 2.1 | 95.9 |
| **128** | **16,668** | [16,420, 16,917] | **10.39×** | 7,255 | 2.7 | 127.8 |
| 192 | 16,248 | [15,861, 16,636] | 10.13× | 11,125 | 4.3 | 191.7 |
| 256 | 16,584 | [16,309, 16,860] | 10.34× | 14,710 | 3.0 | 254.5 |

**Peak: 16,668 mut/s at N=128 — 10.39× the sequential N=1 baseline.**

## Findings

- **Group commit delivers ~10× the sequential write throughput.** Throughput
  climbs steeply to a **knee at the batch cap (N≈16–32)**, then flat-lines at a
  **saturation plateau of ~16,600 mut/s** from N=64 through N=256 (no retrograde
  in range — the single-writer committer is pinned at C_max, and latency grows
  linearly with N, the textbook saturation signature).
- **~10×, not 32×.** Group commit amortizes only the *fsync*; the per-mutation CPU
  work (validation, index maintenance, version snapshot, serialization) is not
  batched, so the speedup is bounded well below the batch cap.
- **Little's Law closes at every rung** (`N ≈ X·R` reproduces the set N to within
  a fraction of a percent), validating that the harness has no clock / accounting
  / coordinated-omission measurement bug.
- **N=1 = 1,604 mut/s** on this run — the same order as the ~2,661 mut/s
  embedded-provider CRUD figure; the difference is harness + machine-state, not a
  regression. The speedup is anchored on this run's own N=1.

## The deadlock this benchmark found

Building this sweep surfaced a real, intermittent **lost-wakeup deadlock** in the
mutation journal worker's release path (`release_mutation_worker` read
`has_pending()` before clearing `worker_running`, so a mutation enqueued in that
window could be stranded with no drainer). It reproduced ~1 run in 4 before the
fix and 0 in 8 after. Reachable on the ordinary write path, invisible to the
sequential tests. Root-caused, fixed, and guarded with a deterministic regression
test in **PR #184** (merged). This benchmark paid for itself on day one.

## Caveats

- Single-host macOS (Apple M2-class) laptop; **not** a multi-node or
  Linux/server-class number. Treat as relative / architectural evidence, not a
  capacity guarantee. A pinned minicloud/KVM run with a fixed CPU governor is the
  follow-up for publishable capacity figures.
- Latency percentiles at saturated rungs are queue latency, not service latency
  (coordinated omission); a faithful SLA-latency number needs an open-loop
  companion run below C_max.
- Effective batch size (ops/append) is measured directly: the journal worker
  exports `journal_batch_size_sum` / `journal_batch_count` counters and the
  bench reports their ratio as the `avg batch` column.

## How to run

```
cargo bench -p nimbus-engine --bench concurrent-write-throughput --no-run
BIN=$(ls -t target/release/deps/concurrent_write_throughput-* | grep -v '\.d$' | head -1)
NIMBUS_CWB_WORKLOAD=crud NIMBUS_CWB_MEASURE_ROUNDS=15 NIMBUS_CWB_WARMUP_ROUNDS=3 \
  NIMBUS_CWB_OUT=/tmp/cwb.md "$BIN"
```

Env knobs (all optional): `NIMBUS_CWB_WORKLOAD=crud|insert`, `NIMBUS_CWB_LADDER`,
`NIMBUS_CWB_OPS_PER_WORKER`, `NIMBUS_CWB_MAX_MUTATIONS_PER_ROUND`,
`NIMBUS_CWB_MEASURE_ROUNDS`, `NIMBUS_CWB_WARMUP_ROUNDS`, `NIMBUS_CWB_SEED_DOCS`,
`NIMBUS_CWB_BACKEND=sqlite|redb`, `NIMBUS_CWB_OUT`.

## PPSC0 post-instrumentation baseline (2026-07-15, main @c1595e948)

Re-run after PPSC0 landed (PreparedCommit unification, phase timers, sampled
shadow-conflict observation, bench phase split — PRs #188/#191/#192). Same
harness, SQLite, 15 rounds, phased CRUD, split mode on.

**Peak: 18,022 mut/s at N=64 — 10.01× the N=1 anchor (1,801 mut/s); plateau
16.6–18k through N=256, no retrograde.** Slightly above the pre-PPSC0 peak
(16,668): batch-merging the shadow observation removed per-request overhead.

Under-gate phase split at the plateau (the PPSC baseline number):
**plan-CPU ≈ 21–22% · conflict-check (sampled shadow) ≈ 0.4–2% · apply
(storage apply + publish) ≈ 52–54% · fsync/append ≈ 23–24%.** The split is
apply-dominated, per the parallel-prepare plan's Decision Record: the
embedded prepare-pool lift is Amdahl-bounded (~1.3×); provider-arm RTT
pipelining and the architecture/contract outcomes carry the plan.

Provider baseline (PPSC0 B5, live local Postgres): mean gate-hold/commit
1.19 ms loopback vs 215 ms at 5 ms/direction injected RTT (~25% gate-hold
share) — the enqueue-to-durable network round-trip dominates exactly as the
provider-arm analysis predicts.

Regression note: the first baseline attempt measured a collapse to ~569
mut/s at N=256 — the unbounded, per-request shadow-conflict scan under the
gate (#191) feeding back into queue depth. Fixed in #192 (window clamp 64 +
batch-merge + 1-in-16 deterministic sampling + separate conflict-check
split column). The instrumentation caught its own regression: working as
designed.

## PPSC2-C adaptive batching (2026-07-16, PR #196, main @24eb0a473)

Adaptive journal batching landed: base drain cap stays 32; under observed
backlog (journal + admission queue depth at drain time) the cap grows to
`NIMBUS_MUTATION_JOURNAL_BATCH_MAX` (default 256), with an off-by-default
Tokio-time coalescing window (`NIMBUS_MUTATION_JOURNAL_COALESCE_MICROS`).
One durable append per drained batch is preserved; effective batch size is
recorded in the phase metrics (`journal_batch_size_sum / journal_batch_count`)
and reported by the bench split as "avg effective batch."

Short ladder (SQLite, release, `NIMBUS_CWB_LADDER=1,32,256`, 5 rounds):

| N | fixed cap (mean mut/s) | adaptive (mean mut/s) | Δ | avg batch | p50/p95/p99 µs (adaptive) |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 2,013 | 2,002 | −0.5% (CI overlap) | 1.00 | 489 / 557 / 686 |
| 32 | 18,880 | 18,292 | −3.1% (CI overlap) | 31.55 | 1,697 / 2,092 / 4,664 |
| 256 | 18,974 | **25,593** | **+34.9%** | 246.97 | 9,953 / 14,934 / 18,309 |

The fsync-ratio gate: a fixed 32 cap needs ≥8 durable appends per 256-write
burst; the adaptive committer averaged 246.97 writes per append ≈ **1.04
appends per 256 writes**. Tail latency at N=256 *improved* — the fixed-cap
arm's p50 was 13.5 ms (from the same sweep's raw report; the table above
tabulates only the adaptive arm's percentiles) versus the adaptive arm's
9,953 µs ≈ 10.0 ms — because requests spend less time queued behind fsyncs.

Provider lane (PR #197): the deterministic postgres test
`postgres_adaptive_batch_commits_a_burst_in_one_durable_round_trip` proves a
paused 96-insert concurrent burst commits in **one durable network round
trip** (>32 ops/RTT, past the fixed cap) while an idle arrival keeps the
1-op/1-round-trip baseline — the ops-per-RTT lever the provider-arm analysis
identified, live-Postgres-verified (PostgreSQL 17.9).

## PPSC5-A embedded ordered publisher (2026-07-17, `ppsc5-publisher`)

Slice A keeps provider stores on the actor-owned serial arm and moves the
embedded arm to a bounded ordered publisher. The serial kill-switch is the
pre-publisher implementation retained in the same binary, so the comparison
below isolates topology while holding the tree, release build, machine, SQLite
store, workload, and five measured/two warm-up rounds constant.

| CRUD N | serial mut/s | publisher mut/s | delta | serial avg batch | publisher avg batch | serial/publisher fsync share |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1,923 | 2,042 | +6.2% | 1.00 | 1.00 | 44.2% / 44.4% |
| 32 | 12,430 | 15,684 | +26.2% | 16.19 | 16.03 | 30.9% / 30.0% |
| 256 | 20,710 | 23,432 | +13.1% | 146.42 | **152.42** | 17.2% / 16.6% |

The burst fsync-ratio gate therefore holds: the publisher improves rather than
reduces average effective batch at N=256, while N=1 remains one append per
mutation. The publisher's 750 µs Tokio-time accumulator window activates only
when receiver or assignment backlog proves more work exists; the current
batch's own response guards no longer self-classify a large idle burst as
pressure. Publisher accumulation uses its own
`NIMBUS_COMMITTER_PUBLISHER_BATCH_MAX` (default 256) and
`NIMBUS_COMMITTER_PUBLISHER_COALESCE_MICROS` (default 750) keys, independently
of the actor's `NIMBUS_MUTATION_JOURNAL_*` policy.

Hot-key parity re-check (N=32, same release/split protocol): serial **3,046
mut/s**, publisher **3,124 mut/s** (**+2.6%**). The N=1 hot-key samples are
fsync-dominated and noisy (351 vs 312 mut/s); the requested contention parity
rung does not regress.

Review-fix rerun after removing the current batch's own response guards from
the pressure signal (10 measured/two warm-up rounds, release/split protocol):

| CRUD N | recorded publisher mut/s | review-fix mut/s | review-fix p50 µs | review-fix avg batch |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 2,042 | 1,321 | 742.8 | 1.00 |
| 32 | 15,684 | 12,517 | 2,417.1 | 16.11 |
| 256 | 23,432 | **23,609** | **10,210.8** | 129.32 |

The machine's singleton anchor was 35% below the recorded run, so the N=1 and
N=32 absolute latency/throughput rows are not accepted as a clean historical
comparison. Normalized scaling improved from 7.68× to 9.47× at N=32 and from
11.47× to 17.87× at N=256. The saturated throughput gate did not regress:
N=256 is +0.8%, with p50 improving 4.1%.

Correctness evidence for this slice:

| Gate | Result |
| --- | --- |
| mutation-journal/per-path group | 72/72 passed |
| `fanout_never_precedes_applied_head` | passed |
| `publisher_preserves_sequence_order_across_transient_retry` | passed |
| `publisher_torn_tail_recovery_replays_exactly_one_contiguous_prefix` | passed |
| `kill_switch_mid_load_produces_identical_state` | passed; documents and durable journal bytes match pipeline/serial/mid-load flip |
| Hermitage + unchanged window differential | 11/11 + 1/1 passed |
| actor→publisher loom handoff lane | 13/13 passed |
| core + storage + engine | 978 passed, 4 skipped |
| server | 559 passed, 23 skipped |
| live PostgreSQL storage/engine | 21/21 + 12/12 passed |
| live libSQL storage/engine | 18/18 + 6/6 passed |

In pipeline mode the embedded actor's serial boundary ends at
`assign_queued_mutation_batch` → `PublisherHandoff::send`; durable append,
storage apply, watermark publication, and fan-out execute only in
`persist_assigned_batch_once` on the publisher. Opaque embedded commit jobs use
the same ordered publisher behind an assignment fence. Provider stores and the
explicit `serial`/`off` kill-switch intentionally retain the pre-slice actor
path until PPSC5 slice C.

## PPSC5-D/U8 performance closeout (2026-07-23)

U8 measures the production ordered-publisher topology after the provider arm
flip. The measured candidate merged as `42c4c5198` (PR #235). Its pre-merge
worktree base and branch head are not reachable here, because the branch
squash-merged and the worktree is gone.

The runs wrote reports under `target/ppsc-bench/`. Those files are build
output and are not retained, so the tables below are the record rather than a
pointer to one. The hashes name the exact files the numbers were transcribed
from:

| Report | SHA-256 |
| --- | --- |
| `embedded-sqlite.md` | `c58971fd1b3af35a03733b2d91f1dd5f6c468332ae9974e5554fa76c5d1ca2bd` |
| `postgres-mixed-load.md` | `93fc2d19ff9682e329375ef008085ca17bc6affd7e04a955eaa0c7a8f7ff4919` |
| `mysql-mixed-load.md` | `605cf352f7c1bc83b3f74e6d135d00545e7a0bac2a4bb5d2153533dd1ca2ad5e` |

### Embedded SQLite fsync/batching gate

Command:

```bash
NIMBUS_CWB_WORKLOAD=crud NIMBUS_CWB_LADDER=1,32,256 \
NIMBUS_CWB_MEASURE_ROUNDS=5 NIMBUS_CWB_WARMUP_ROUNDS=2 \
NIMBUS_CWB_BACKEND=sqlite NIMBUS_CWB_SPLIT_PHASES=1 \
NIMBUS_CWB_OUT=target/ppsc-bench/embedded-sqlite.md \
cargo bench -p nimbus-engine --bench concurrent-write-throughput
```

| N | Mean mut/s | 95% CI | Median | CV | Speedup | Avg effective batch | Fsync/append share |
| ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: |
| 1 | 1,629 | [1,585, 1,672] | 1,630 | 2.2% | 1.00× | 1.00 | 44.9% |
| 32 | 12,690 | [12,418, 12,962] | 12,566 | 1.7% | 7.79× | 16.42 | 29.0% |
| 256 | 20,132 | [19,734, 20,531] | 20,174 | 1.6% | 12.36× | 143.25 | 16.3% |

Raw measured-round mut/s:

- N=1: `1600.495, 1663.828, 1662.955, 1587.432, 1629.586`
- N=32: `12863.783, 12483.350, 12554.561, 12565.920, 12982.045`
- N=256: `19603.976, 20124.548, 20445.661, 20313.205, 20174.071`

The saturated rung retains a large effective batch and reduces fsync share as
load grows. It therefore does not regress the embedded group-commit/fsync
behavior established earlier in the campaign.

### PostgreSQL 16 loopback and injected RTT

Command:

```bash
NIMBUS_BENCH_POSTGRES_URL='host=127.0.0.1 port=25432 user=postgres password=<fixture> dbname=postgres' \
NIMBUS_BENCH_RTT_DELAY_MS=5 \
make bench-postgres-provider WORKLOAD=mixed-load \
  REPORT=target/ppsc-bench/postgres-mixed-load.md
```

The pinned fixture was `postgres:16`, image digest
`sha256:33f923b05f64ca54ac4401c01126a6b92afe839a0aa0a52bc5aeb5cc958e5f20`.
The run used 10 steady, 8 cold, and 4 RTT measured rounds after 2/1/1 warmups.

| Lane/backend | Median per op | Mean 95% CI | Median ops/s |
| --- | ---: | --- | ---: |
| Steady SQLite | 403.39 µs | [389.67, 414.91] µs | 2,478.97 |
| Steady PostgreSQL loopback | 5.19 ms | [5.15, 5.22] ms | 192.65 |
| Cold SQLite | 440.44 µs | [424.67, 446.23] µs | 2,270.44 |
| Cold PostgreSQL loopback | 6.48 ms | [6.05, 6.75] ms | 154.31 |
| RTT PostgreSQL loopback | 20.12 ms | [18.92, 21.95] ms | 49.71 |
| RTT PostgreSQL +5 ms/direction | 643.84 ms | [640.75, 647.71] ms | 1.55 |

Raw round durations:

- steady SQLite: `186.31, 189.16, 202.43, 188.99, 177.34, 196.55, 194.72, 192.54, 195.32, 207.64 ms`
- steady PostgreSQL: `2.47, 2.46, 2.50, 2.54, 2.47, 2.49, 2.49, 2.47, 2.50, 2.50 s`
- cold SQLite: `212.67, 211.93, 214.31, 201.12, 216.72, 201.49, 210.90, 202.99 ms`
- cold PostgreSQL: `2.77, 2.88, 2.91, 3.06, 3.16, 3.21, 3.29, 3.31 s`
- RTT loopback: `163.55, 157.91, 158.31, 174.28 ms`
- RTT injected: `5.14, 5.14, 5.18, 5.16 s`

For the four mutation commits in each RTT round, mean durable append was
16.83 ms/commit loopback and 658.21 ms/commit injected, representing 92.14%
and 95.19% of measured commit phase time. This provider I/O occurs after
serial assignment releases the assignment/recovery gate, so ordinary mutation
`durable_append` network-wait share under serial assignment is 0% by
construction. Initial lease acquisition remains intentionally before first
assignment; lease renewal is a separate background lifecycle.

### MySQL 8.4 loopback and injected RTT

Command:

```bash
NIMBUS_MYSQL_URL='mysql://root:<fixture>@127.0.0.1:23306/test' \
NIMBUS_BENCH_RTT_DELAY_MS=5 \
make bench-mysql-provider WORKLOAD=mixed-load \
  REPORT=target/ppsc-bench/mysql-mixed-load.md
```

The pinned fixture was `mysql:8.4`, image digest
`sha256:c592c15aaf4a1961e15d82eb31ea5987dda862d1c4b1e93424438c0e91dc1f8d`.
The run used the same 10/8/4 measured and 2/1/1 warmup protocol.

| Lane/backend | Median per op | Mean 95% CI | Median ops/s |
| --- | ---: | --- | ---: |
| Steady SQLite | 415.24 µs | [383.91, 490.60] µs | 2,408.23 |
| Steady MySQL loopback | 4.53 ms | [4.46, 4.61] ms | 220.85 |
| Cold SQLite | 435.60 µs | [418.04, 459.56] µs | 2,295.69 |
| Cold MySQL loopback | 6.49 ms | [6.01, 6.82] ms | 154.19 |
| RTT MySQL loopback | 20.10 ms | [17.87, 24.23] ms | 49.75 |
| RTT MySQL +5 ms/direction | 553.52 ms | [521.74, 577.79] ms | 1.81 |

Raw round durations:

- steady SQLite: `302.92, 234.62, 181.25, 187.32, 189.18, 204.17, 196.88, 205.75, 194.98, 201.76 ms`
- steady MySQL: `2.10, 2.12, 2.15, 2.19, 2.26, 2.17, 2.16, 2.20, 2.18, 2.25 s`
- cold SQLite: `225.22, 201.20, 213.75, 205.79, 229.39, 201.48, 212.39, 195.78 ms`
- cold MySQL: `2.77, 2.82, 2.88, 3.03, 3.20, 3.24, 3.32, 3.36 s`
- RTT loopback: `192.38, 159.65, 161.63, 159.98 ms`
- RTT injected: `4.21, 4.47, 4.53, 4.38 s`

Mean durable append was 18.57 ms/commit loopback and 584.78 ms/commit
injected, representing 89.44% and 90.78% of measured commit phase time. The
same post-assignment ownership rule makes ordinary mutation network-wait share
under serial assignment 0%.

### In-flight-depth proof and interpretation boundary

The RTT workload intentionally runs one tenant and awaits each read, query,
insert, or update before issuing the next operation. Its observed effective
mutation rate of 0.01 operations per nominal RTT is therefore a latency/protocol
sensitivity result, not evidence for or against same-tenant batching. It is not
used to claim publisher pipelining throughput.

Actual provider statement in-flight depth is established through the
production Engine publisher interface and its provider diagnostics:

```bash
NIMBUS_PROVIDER_FIXTURE_PROJECT=nimbus-external-provider-tests-u8 \
NIMBUS_PROVIDER_FIXTURE_POSTGRES_PORT=25432 \
make test-external-provider PROVIDER=postgres REUSE=1 KEEP=1 \
  TEST_FILTER='test(postgres_provider_publisher_contract)'

NIMBUS_PROVIDER_FIXTURE_PROJECT=nimbus-external-provider-tests-u8 \
NIMBUS_PROVIDER_FIXTURE_MYSQL_PORT=23306 \
make test-external-provider PROVIDER=mysql REUSE=1 KEEP=1 \
  TEST_FILTER='test(mysql_provider_publisher_contract)'
```

Both commands executed exactly one test and passed. PostgreSQL asserted
configured and observed in-flight depth **2**; MySQL asserted configured and
observed depth **1**, the deliberate driver/protocol divergence. Both also
asserted one journal statement per provider batch and exercised the canonical
async tenant lifecycle plus the production ordered-publisher path. The complete
U8 live-provider lanes independently passed PostgreSQL 72/72, MySQL 45/45, and
libSQL 49/49 with required fixtures and no provider skips.
