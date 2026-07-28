# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7347 | [7235, 7460] | 7354 | 2.8 | 1.00× | 130.2 | 159.4 | 223.8 | 1.0 |
| 32 | 38473 | [37033, 39913] | 38932 | 6.8 | 5.24× | 754.7 | 1323.3 | 2589.9 | 32.0 |
| 256 | 51117 | [50218, 52016] | 51075 | 3.2 | 6.96× | 4522.1 | 8455.8 | 9911.9 | 251.6 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7418.566, 7613.006, 7645.503, 7361.188, 7694.297, 7458.821, 7170.208, 6990.644, 7224.100, 7147.935, 7155.472, 7424.303, 7354.316, 7197.603, 7352.236 |
| 32 | 39439.899, 37600.755, 40097.249, 38907.308, 38932.256, 38179.075, 38465.826, 38366.969, 36169.545, 30158.299, 39849.641, 41126.803, 39801.498, 40079.511, 39914.773 |
| 256 | 53941.294, 48631.239, 53632.647, 49148.735, 49339.982, 51078.830, 49764.923, 49958.616, 51005.702, 50258.416, 51962.537, 51981.375, 52154.855, 51074.558, 52823.275 |

**Peak:** 51117 mut/s at N=256 — 6.96× the sequential (N=1) baseline of 7347 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.9% | 0.7% | 58.3% | 35.1% | 484.243 ms |
| 32 | 14.48 | 133920/0 | 24.5% | 4.2% | 44.6% | 26.7% | 4094.386 ms |
| 256 | 112.54 | 126720/0 | 37.8% | 6.4% | 35.6% | 20.2% | 2921.611 ms |
