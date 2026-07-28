# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 643 | [617, 669] | 620 | 7.3 | 1.00× | 1560.3 | 1790.6 | 2485.5 | 1.0 |
| 32 | 5981 | [5848, 6114] | 5995 | 4.0 | 9.30× | 5291.3 | 6129.4 | 7137.2 | 31.9 |
| 256 | 5105 | [5015, 5194] | 5147 | 3.2 | 7.94× | 38458.3 | 103569.8 | 153813.9 | 242.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 638.001, 710.322, 718.384, 707.063, 719.009, 655.371, 600.948, 632.180, 619.585, 601.474, 616.534, 616.732, 615.218, 593.998, 600.102 |
| 32 | 6144.719, 5463.487, 5876.265, 5838.366, 6023.661, 5888.413, 6105.411, 5777.022, 6557.101, 6166.931, 5995.242, 6039.838, 5861.551, 6112.702, 5862.299 |
| 256 | 5147.469, 5253.800, 5059.989, 5230.874, 4705.410, 5204.718, 5083.286, 4925.940, 5257.954, 5189.086, 5256.058, 5190.601, 5074.333, 5131.171, 4856.969 |

**Peak:** 5981 mut/s at N=32 — 9.30× the sequential (N=1) baseline of 643 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 12.2% | 0.5% | 1.8% | 85.6% | 340.614 ms |
| 32 | 0.00 | 48000/0 | 12.1% | 12.0% | 1.5% | 74.5% | 6647.424 ms |
| 256 | 0.00 | 134400/0 | 13.9% | 13.2% | 2.2% | 70.7% | 22376.543 ms |
