# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1940 | [1929, 1952] | 1950 | 1.1 | 1.00× | 502.4 | 581.3 | 732.0 | 1.0 |
| 32 | 15629 | [15377, 15882] | 15667 | 2.9 | 8.05× | 1965.5 | 2457.2 | 4138.1 | 32.0 |
| 256 | 27843 | [27505, 28181] | 27650 | 2.2 | 14.35× | 8677.2 | 13280.2 | 15896.7 | 251.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1938.031, 1957.364, 1949.859, 1965.236, 1939.977, 1896.183, 1962.080, 1953.328, 1901.849, 1925.547, 1952.715, 1954.660, 1952.992, 1923.673, 1931.922 |
| 32 | 15209.857, 15643.523, 15165.384, 15577.379, 15829.985, 15372.405, 15646.351, 15975.935, 16159.315, 15875.343, 16159.118, 15667.394, 14404.581, 16020.118, 15732.335 |
| 256 | 27612.366, 28409.594, 26995.734, 28837.390, 28821.649, 27568.862, 27649.954, 28603.695, 27454.384, 27841.302, 27636.055, 26849.434, 27415.568, 27760.066, 28187.655 |

**Peak:** 27843 mut/s at N=256 — 14.35× the sequential (N=1) baseline of 1940 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.6% | 0.2% | 53.4% | 44.8% | 15074.765 ms |
| 32 | 16.01 | 133920/0 | 11.2% | 1.4% | 56.1% | 31.3% | 9036.350 ms |
| 256 | 125.09 | 126720/0 | 21.9% | 3.8% | 56.0% | 18.3% | 5006.818 ms |
