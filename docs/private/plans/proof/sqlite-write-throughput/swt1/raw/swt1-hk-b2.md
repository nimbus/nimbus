# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 469 | [453, 485] | 462 | 6.2 | 1.00× | 2040.1 | 2868.2 | 3312.3 | 1.0 |
| 32 | 3142 | [3122, 3162] | 3146 | 1.2 | 6.70× | 10115.4 | 10770.0 | 11076.9 | 31.8 |
| 256 | 2645 | [2635, 2656] | 2647 | 0.7 | 5.64× | 59875.9 | 233257.6 | 363755.7 | 238.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 502.935, 420.816, 437.523, 461.782, 510.078, 455.144, 452.094, 453.006, 483.248, 507.790, 467.708, 447.706, 439.145, 491.246, 505.747 |
| 32 | 3220.205, 3165.852, 3118.874, 3155.201, 3109.584, 3139.332, 3145.881, 3098.356, 3136.874, 3152.973, 3145.403, 3177.758, 3154.385, 3061.896, 3149.225 |
| 256 | 2660.346, 2691.912, 2662.458, 2641.449, 2646.605, 2657.581, 2635.078, 2616.252, 2637.743, 2650.604, 2618.111, 2626.146, 2646.761, 2654.229, 2636.306 |

**Peak:** 3142 mut/s at N=32 — 6.70× the sequential (N=1) baseline of 469 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.6% | 0.1% | 0.5% | 95.7% | 1198.718 ms |
| 32 | 0.00 | 48000/0 | 4.0% | 4.3% | 0.3% | 91.4% | 14546.350 ms |
| 256 | 0.00 | 134503/0 | 4.4% | 4.9% | 1.0% | 89.8% | 47937.557 ms |
