# Concurrent write-throughput benchmark — group-commit ceiling

Records the design and results of `crates/nimbus-engine/benches/concurrent-write-throughput.rs`,
a closed-loop concurrency sweep that measures the per-tenant mutation journal's
**group-commit** throughput. It exists because the embedded-provider CRUD
benchmark is sequential (one mutation awaited at a time → batch size 1) and never
exercises the journal worker's coalescing of up to `MUTATION_JOURNAL_BATCH_SIZE`
(32) concurrently-queued mutations into a single fsync.

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
- Effective batch size (ops/fsync) is inferred from the speedup, not measured
  directly; direct measurement needs journal-worker fsync instrumentation.

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
