# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7136 | [6735, 7536] | 7335 | 10.1 | 1.00× | 130.8 | 164.9 | 244.1 | 1.0 |
| 32 | 38731 | [36154, 41308] | 39919 | 12.0 | 5.43× | 738.5 | 1127.9 | 2482.5 | 32.5 |
| 256 | 52115 | [50909, 53322] | 51771 | 4.2 | 7.30× | 4545.2 | 8057.3 | 9703.4 | 251.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7316.385, 7406.435, 7398.277, 7271.893, 7340.694, 7523.905, 7335.702, 7306.790, 7335.477, 4545.512, 7150.651, 7345.572, 7382.274, 7227.743, 7151.149 |
| 32 | 39629.406, 41006.068, 39919.086, 40259.619, 41144.785, 41126.534, 41166.175, 41883.523, 38047.603, 38556.014, 38668.922, 38022.696, 37831.230, 22646.915, 41055.787 |
| 256 | 55192.133, 53882.219, 52455.936, 51771.368, 50748.899, 56076.193, 52644.192, 51017.509, 51665.367, 53171.383, 51746.593, 50627.197, 53921.354, 48207.037, 48604.216 |

**Peak:** 52115 mut/s at N=256 — 7.30× the sequential (N=1) baseline of 7136 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.7% | 0.7% | 56.0% | 37.7% | 3568.331 ms |
| 32 | 14.71 | 133920/0 | 24.5% | 3.6% | 46.0% | 25.8% | 4100.446 ms |
| 256 | 113.24 | 126720/0 | 36.8% | 7.0% | 36.2% | 20.1% | 2849.397 ms |
