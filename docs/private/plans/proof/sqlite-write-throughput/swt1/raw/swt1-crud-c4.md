# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1930 | [1834, 2026] | 1974 | 9.0 | 1.00× | 500.4 | 548.2 | 717.2 | 1.0 |
| 32 | 20400 | [19931, 20869] | 20605 | 4.2 | 10.57× | 1495.0 | 1849.6 | 3511.2 | 32.0 |
| 256 | 44407 | [42805, 46010] | 45298 | 6.5 | 23.01× | 5227.4 | 8814.4 | 11004.6 | 251.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1988.446, 1957.657, 1973.796, 1972.769, 2006.988, 1933.360, 1308.214, 1961.760, 1974.852, 1975.187, 1999.319, 1984.171, 1972.747, 1991.413, 1948.209 |
| 32 | 20523.943, 17366.644, 20480.781, 20739.031, 20432.555, 20829.737, 20465.724, 20604.858, 20744.509, 20723.691, 20604.838, 20598.894, 20635.393, 20551.851, 20699.357 |
| 256 | 44472.628, 45298.427, 34468.203, 44519.716, 45553.966, 45142.553, 45882.206, 45743.453, 43036.124, 43452.804, 45502.041, 45247.477, 46058.719, 46142.691, 45588.883 |

**Peak:** 44407 mut/s at N=256 — 23.01× the sequential (N=1) baseline of 1930 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.3% | 0.2% | 52.9% | 45.6% | 2210.238 ms |
| 32 | 16.01 | 133920/0 | 13.7% | 1.6% | 47.4% | 37.3% | 7052.302 ms |
| 256 | 119.66 | 126720/0 | 30.8% | 5.5% | 39.3% | 24.4% | 3296.597 ms |
