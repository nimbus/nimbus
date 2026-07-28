# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1926 | [1758, 2093] | 2019 | 15.7 | 1.00× | 490.4 | 579.5 | 815.6 | 1.0 |
| 32 | 16363 | [15793, 16934] | 16647 | 6.3 | 8.50× | 1858.4 | 2143.2 | 3866.2 | 32.1 |
| 256 | 28822 | [28508, 29136] | 28813 | 2.0 | 14.97× | 8402.3 | 12692.8 | 15187.5 | 251.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2062.914, 2053.046, 2048.027, 2056.649, 2035.461, 847.419, 1983.348, 1955.920, 1928.621, 1923.492, 1912.319, 1999.333, 2036.921, 2019.071, 2024.972 |
| 32 | 16300.536, 16058.737, 12903.193, 16681.095, 16358.120, 16856.230, 16749.403, 17178.238, 16615.325, 17000.410, 16646.620, 16895.895, 15697.210, 16588.567, 16921.059 |
| 256 | 28879.122, 29203.746, 29824.146, 28693.390, 29167.435, 27973.028, 29817.562, 28604.809, 28505.102, 28813.175, 29273.579, 28515.060, 27947.891, 28824.341, 28283.745 |

**Peak:** 28822 mut/s at N=256 — 14.97× the sequential (N=1) baseline of 1926 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.5% | 0.2% | 49.2% | 49.2% | 2294.152 ms |
| 32 | 16.01 | 133920/0 | 11.1% | 1.3% | 55.8% | 31.8% | 8694.972 ms |
| 256 | 125.84 | 126720/0 | 21.0% | 3.7% | 57.3% | 18.0% | 4831.568 ms |
