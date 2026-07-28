# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1917 | [1889, 1944] | 1933 | 2.6 | 1.00× | 506.4 | 562.5 | 744.5 | 1.0 |
| 32 | 18949 | [16555, 21344] | 20114 | 22.8 | 9.89× | 1521.8 | 1722.2 | 3477.1 | 40.2 |
| 256 | 45361 | [44892, 45830] | 45513 | 1.9 | 23.67× | 5204.8 | 8756.3 | 10396.9 | 249.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1892.224, 1785.790, 1888.877, 1841.774, 1954.271, 1965.605, 1957.844, 1949.663, 1899.404, 1950.439, 1934.824, 1922.736, 1933.093, 1945.300, 1925.927 |
| 32 | 19537.616, 19547.584, 20081.216, 20173.376, 19832.254, 19921.212, 20184.334, 20460.104, 20211.502, 20389.346, 20114.037, 20251.497, 20133.097, 20051.677, 3351.485 |
| 256 | 45768.225, 46207.691, 45928.030, 45471.835, 44968.900, 45513.267, 44818.779, 44198.307, 45853.069, 45674.411, 45032.003, 46494.221, 43832.091, 44116.025, 46535.135 |

**Peak:** 45361 mut/s at N=256 — 23.67× the sequential (N=1) baseline of 1917 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.4% | 0.2% | 54.1% | 44.3% | 2194.706 ms |
| 32 | 16.00 | 133920/0 | 10.3% | 1.1% | 60.1% | 28.4% | 9351.179 ms |
| 256 | 120.34 | 126720/0 | 31.3% | 5.9% | 38.1% | 24.7% | 3220.747 ms |
