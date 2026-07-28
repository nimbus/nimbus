# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1990 | [1973, 2007] | 1990 | 1.5 | 1.00× | 493.7 | 549.8 | 640.3 | 1.0 |
| 32 | 17013 | [16894, 17131] | 17037 | 1.3 | 8.55× | 1818.0 | 2022.8 | 3756.8 | 31.9 |
| 256 | 29310 | [29046, 29575] | 29122 | 1.6 | 14.73× | 8275.4 | 12393.0 | 14125.6 | 250.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1996.217, 1986.867, 2038.665, 2000.959, 2000.311, 2003.860, 1973.896, 1908.649, 1990.007, 1976.000, 1984.715, 1975.247, 1968.988, 2027.946, 2018.003 |
| 32 | 17140.818, 16961.687, 16928.781, 16938.403, 17145.603, 17036.549, 16910.252, 17127.521, 17287.812, 16359.020, 17080.595, 17081.043, 17233.709, 16960.112, 16996.014 |
| 256 | 29198.888, 30310.963, 29122.273, 29655.849, 28979.202, 29914.701, 29817.242, 29009.495, 28888.154, 28804.407, 28735.260, 29806.319, 29092.600, 28929.641, 29390.503 |

**Peak:** 29310 mut/s at N=256 — 14.73× the sequential (N=1) baseline of 1990 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.5% | 0.2% | 53.2% | 45.1% | 2102.220 ms |
| 32 | 16.01 | 133920/0 | 11.5% | 1.3% | 56.5% | 30.8% | 8349.336 ms |
| 256 | 122.79 | 126720/0 | 21.3% | 3.8% | 57.0% | 17.8% | 4792.299 ms |
