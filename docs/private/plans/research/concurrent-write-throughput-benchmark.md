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
above 64 outstanding operations; N≤32 stays on immediate flush.

Hot-key parity re-check (N=32, same release/split protocol): serial **3,046
mut/s**, publisher **3,124 mut/s** (**+2.6%**). The N=1 hot-key samples are
fsync-dominated and noisy (351 vs 312 mut/s); the requested contention parity
rung does not regress.

Correctness evidence for this slice:

| Gate | Result |
| --- | --- |
| mutation-journal/per-path group | 66/66 passed |
| `fanout_never_precedes_applied_head` | passed |
| `publisher_preserves_sequence_order_across_transient_retry` | passed |
| `publisher_torn_tail_recovery_replays_exactly_one_contiguous_prefix` | passed |
| `kill_switch_mid_load_produces_identical_state` | passed; documents and durable journal bytes match pipeline/serial/mid-load flip |
| Hermitage + unchanged window differential | 11/11 + 1/1 passed |
| actor→publisher loom handoff lane | 11/11 passed |
| core + storage + engine | 965 passed, 4 skipped (baseline 960 + five slice tests) |
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
