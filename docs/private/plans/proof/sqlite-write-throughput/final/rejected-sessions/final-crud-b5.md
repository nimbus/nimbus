# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2011 | [1990, 2031] | 2025 | 1.8 | 1.00× | 486.9 | 555.3 | 694.1 | 1.0 |
| 32 | 16376 | [16240, 16512] | 16369 | 1.5 | 8.14× | 1882.6 | 2133.7 | 4012.0 | 31.9 |
| 256 | 28603 | [28331, 28876] | 28518 | 1.7 | 14.23× | 8411.3 | 13095.7 | 14801.0 | 251.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1989.931, 2025.049, 2033.130, 2049.607, 2056.217, 2026.586, 2060.110, 2015.582, 2028.391, 1962.353, 2038.671, 1967.196, 1940.231, 1989.952, 1977.473 |
| 32 | 16656.732, 16361.866, 16355.906, 16360.227, 16099.961, 15666.777, 16303.360, 16284.080, 16529.334, 16594.338, 16477.261, 16369.212, 16619.061, 16423.616, 16538.710 |
| 256 | 27837.621, 28401.360, 29205.534, 28378.030, 28468.572, 28853.371, 28975.785, 27794.506, 28154.666, 28168.920, 29374.275, 28517.920, 29143.060, 29004.739, 28769.669 |

**Peak:** 28603 mut/s at N=256 — 14.23× the sequential (N=1) baseline of 2011 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.5% | 0.2% | 53.6% | 44.7% | 2083.902 ms |
| 32 | 16.00 | 133920/0 | 11.3% | 1.3% | 55.9% | 31.4% | 8656.960 ms |
| 256 | 125.96 | 126720/0 | 21.2% | 3.7% | 57.2% | 17.8% | 4853.311 ms |
