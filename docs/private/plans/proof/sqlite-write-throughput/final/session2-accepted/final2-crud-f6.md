# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6791 | [5978, 7603] | 7121 | 21.6 | 1.00× | 133.3 | 175.8 | 283.0 | 1.2 |
| 32 | 38689 | [38280, 39097] | 38955 | 1.9 | 5.70× | 762.6 | 1088.3 | 2579.8 | 31.9 |
| 256 | 50292 | [49173, 51412] | 49723 | 4.0 | 7.41× | 4654.0 | 8475.5 | 9922.8 | 251.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7284.965, 7046.761, 7114.062, 7125.087, 7103.513, 6979.808, 7032.768, 6980.837, 7221.035, 7175.210, 7120.986, 7333.641, 7611.050, 7210.968, 1519.285 |
| 32 | 39632.500, 39151.463, 38958.035, 37858.234, 39264.005, 38609.195, 37526.231, 38955.379, 39310.871, 39215.069, 38855.966, 37536.854, 39202.908, 38862.435, 37394.138 |
| 256 | 50854.562, 49699.843, 49588.887, 46818.217, 49722.977, 48497.641, 49299.972, 51540.060, 53709.281, 50703.477, 50432.434, 50791.284, 54948.918, 48609.996, 49164.932 |

**Peak:** 50292 mut/s at N=256 — 7.41× the sequential (N=1) baseline of 6791 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 4.4% | 0.6% | 45.3% | 49.7% | 651.169 ms |
| 32 | 14.62 | 133920/0 | 25.1% | 3.8% | 44.9% | 26.1% | 4037.496 ms |
| 256 | 112.44 | 126720/0 | 37.6% | 6.5% | 35.8% | 20.1% | 3000.114 ms |
