# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6514 | [6190, 6837] | 6212 | 9.0 | 1.00× | 149.1 | 189.3 | 265.3 | 1.0 |
| 32 | 37724 | [37362, 38085] | 37623 | 1.7 | 5.79× | 784.0 | 1110.5 | 2578.1 | 31.9 |
| 256 | 52069 | [51131, 53008] | 51556 | 3.3 | 7.99× | 4485.5 | 8180.1 | 9617.2 | 251.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7442.453, 7421.349, 7232.561, 7099.962, 7291.317, 6051.941, 5986.321, 6035.150, 6025.412, 6101.757, 6297.907, 6211.780, 6229.640, 6159.590, 6122.100 |
| 32 | 38773.848, 37333.337, 36807.742, 37810.008, 36976.134, 36572.047, 37586.943, 38140.420, 38395.397, 37944.563, 37623.288, 37554.018, 38789.648, 37990.263, 37556.980 |
| 256 | 54783.846, 54393.722, 52613.005, 52333.847, 54030.618, 51549.718, 49209.799, 53295.103, 51451.058, 53547.574, 51051.474, 50205.873, 49780.489, 51555.774, 51239.879 |

**Peak:** 52069 mut/s at N=256 — 7.99× the sequential (N=1) baseline of 6514 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.6% | 0.7% | 57.8% | 35.9% | 543.871 ms |
| 32 | 14.63 | 133920/0 | 25.0% | 3.8% | 44.1% | 27.0% | 4136.952 ms |
| 256 | 115.62 | 126720/0 | 36.7% | 6.7% | 36.1% | 20.6% | 2794.177 ms |
