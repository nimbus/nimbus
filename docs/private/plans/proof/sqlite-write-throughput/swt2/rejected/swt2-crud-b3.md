# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1312 | [1232, 1393] | 1260 | 11.1 | 1.00× | 773.0 | 913.5 | 1288.2 | 1.0 |
| 32 | 14253 | [13820, 14685] | 14397 | 5.5 | 10.86× | 2138.2 | 3361.5 | 4894.3 | 31.9 |
| 256 | 36915 | [36420, 37410] | 36921 | 2.4 | 28.13× | 6363.1 | 10812.6 | 13128.4 | 249.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1777.353, 1480.499, 1331.903, 1260.494, 1240.062, 1261.248, 1338.375, 1289.047, 1231.520, 1227.534, 1233.838, 1308.439, 1252.782, 1238.425, 1210.618 |
| 32 | 14093.043, 14224.113, 15027.459, 14922.580, 14227.408, 14071.591, 11627.914, 14296.994, 14593.453, 14509.639, 14526.799, 14679.842, 14106.056, 14484.871, 14397.273 |
| 256 | 36921.174, 38999.176, 37559.520, 34884.261, 36453.409, 36812.857, 37242.362, 37132.475, 36771.269, 37109.483, 36688.111, 37045.726, 36125.323, 37756.959, 36225.004 |

**Peak:** 36915 mut/s at N=256 — 28.13× the sequential (N=1) baseline of 1312 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.4% | 0.2% | 54.3% | 44.1% | 3152.016 ms |
| 32 | 16.07 | 133920/0 | 12.7% | 1.6% | 50.1% | 35.6% | 9745.786 ms |
| 256 | 119.89 | 126720/0 | 28.8% | 5.8% | 39.1% | 26.3% | 3872.575 ms |
