# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6931 | [6484, 7378] | 7268 | 11.6 | 1.00× | 134.0 | 175.3 | 246.2 | 1.0 |
| 32 | 38138 | [37757, 38519] | 38199 | 1.8 | 5.50× | 773.2 | 1103.5 | 2523.5 | 31.8 |
| 256 | 48578 | [44918, 52238] | 50261 | 13.6 | 7.01× | 4692.5 | 8744.1 | 10804.0 | 258.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7394.386, 7333.835, 7299.217, 7186.375, 7268.322, 7265.209, 6972.794, 6242.339, 6287.374, 4325.032, 7299.678, 7273.412, 7377.523, 7268.779, 7168.573 |
| 32 | 37991.934, 38542.212, 38131.053, 38937.102, 39101.622, 38536.653, 38199.494, 38332.396, 38585.174, 36994.641, 38870.543, 38194.612, 37187.759, 37020.483, 37448.388 |
| 256 | 52439.207, 51808.422, 26028.537, 50499.448, 50260.920, 51839.511, 52877.995, 45273.373, 48775.817, 52223.273, 52365.192, 50129.929, 48391.139, 47952.717, 47806.262 |

**Peak:** 48578 mut/s at N=256 — 7.01× the sequential (N=1) baseline of 6931 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.6% | 0.7% | 59.7% | 34.1% | 3683.590 ms |
| 32 | 14.52 | 133920/0 | 25.0% | 3.8% | 44.9% | 26.3% | 4071.991 ms |
| 256 | 110.38 | 126720/0 | 35.8% | 6.4% | 34.0% | 23.7% | 3139.304 ms |
