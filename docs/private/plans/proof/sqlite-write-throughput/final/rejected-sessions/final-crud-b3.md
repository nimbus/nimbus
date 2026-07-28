# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1976 | [1954, 1998] | 1986 | 2.0 | 1.00× | 496.0 | 568.8 | 719.2 | 1.0 |
| 32 | 16203 | [16104, 16302] | 16243 | 1.1 | 8.20× | 1909.1 | 2176.6 | 3909.1 | 31.9 |
| 256 | 28425 | [28063, 28786] | 28518 | 2.3 | 14.39× | 8440.0 | 13289.8 | 15765.6 | 251.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1920.486, 1993.790, 1944.302, 1986.274, 2028.150, 2012.034, 1987.134, 1918.113, 1977.338, 1915.648, 1987.781, 2021.110, 2029.144, 1976.518, 1938.283 |
| 32 | 16050.651, 16308.493, 15984.423, 16423.409, 15888.638, 16267.350, 16075.529, 16483.497, 16379.674, 15963.706, 16364.880, 16169.301, 16243.899, 16198.279, 16243.029 |
| 256 | 28517.555, 28990.012, 29194.364, 27493.284, 27141.624, 28552.297, 27908.060, 28292.926, 27992.426, 28670.948, 29193.061, 28712.338, 27889.545, 29431.536, 28387.530 |

**Peak:** 28425 mut/s at N=256 — 14.39× the sequential (N=1) baseline of 1976 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.5% | 0.2% | 53.4% | 44.8% | 2119.227 ms |
| 32 | 16.01 | 133920/0 | 11.4% | 1.4% | 55.8% | 31.4% | 8752.465 ms |
| 256 | 126.09 | 126720/0 | 20.9% | 3.7% | 57.1% | 18.2% | 4864.491 ms |
