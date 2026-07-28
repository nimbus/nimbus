# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2027 | [2003, 2050] | 2043 | 2.1 | 1.00× | 482.5 | 554.0 | 698.5 | 1.0 |
| 32 | 16003 | [15386, 16620] | 16243 | 7.0 | 7.90× | 1898.7 | 2206.2 | 3913.6 | 32.1 |
| 256 | 28734 | [28469, 28999] | 28809 | 1.7 | 14.18× | 8414.9 | 12851.3 | 15393.2 | 251.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2055.438, 2024.437, 1976.560, 1904.970, 2024.450, 2046.560, 2059.517, 2057.512, 2042.802, 2035.739, 2011.606, 2044.731, 2046.487, 2076.530, 1991.286 |
| 32 | 16428.917, 16243.141, 15960.770, 15880.470, 16070.545, 16003.273, 15979.529, 16155.039, 12109.769, 16895.914, 16532.032, 16541.153, 16552.665, 16435.712, 16256.845 |
| 256 | 29380.281, 28504.332, 29539.288, 28737.048, 28896.273, 27809.897, 29237.584, 28809.076, 28247.060, 28582.948, 28916.644, 27987.805, 28544.048, 28924.660, 28890.154 |

**Peak:** 28734 mut/s at N=256 — 14.18× the sequential (N=1) baseline of 2027 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.4% | 0.2% | 53.7% | 44.7% | 2070.801 ms |
| 32 | 16.01 | 133920/0 | 11.1% | 1.4% | 55.2% | 32.4% | 8898.345 ms |
| 256 | 125.59 | 126720/0 | 21.0% | 3.8% | 57.0% | 18.2% | 4860.279 ms |
