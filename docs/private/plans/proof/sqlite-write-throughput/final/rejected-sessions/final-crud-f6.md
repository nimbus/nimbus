# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6451 | [6156, 6746] | 6281 | 8.3 | 1.00× | 150.2 | 190.5 | 265.0 | 1.0 |
| 32 | 36314 | [35874, 36754] | 36534 | 2.2 | 5.63× | 811.3 | 1157.8 | 2676.7 | 31.9 |
| 256 | 49955 | [49280, 50630] | 50264 | 2.4 | 7.74× | 4732.9 | 8371.3 | 9887.4 | 251.2 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7525.658, 7493.404, 7171.522, 5821.263, 6080.167, 6273.340, 6464.037, 6280.678, 6506.810, 6368.999, 6297.213, 6280.076, 6177.877, 6227.312, 5795.352 |
| 32 | 38073.469, 35850.874, 35100.134, 36607.993, 34554.566, 36123.878, 35859.220, 36322.891, 36561.201, 36663.165, 36623.961, 36622.734, 36465.232, 36533.928, 36749.594 |
| 256 | 50870.294, 51061.605, 48949.958, 50693.741, 50264.497, 51180.213, 50274.717, 49306.374, 48134.969, 48842.228, 52081.987, 49403.810, 49259.212, 47950.562, 51047.900 |

**Peak:** 49955 mut/s at N=256 — 7.74× the sequential (N=1) baseline of 6451 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.9% | 0.7% | 56.8% | 36.7% | 548.750 ms |
| 32 | 14.54 | 133920/0 | 25.0% | 3.8% | 44.9% | 26.2% | 4282.626 ms |
| 256 | 111.75 | 126720/0 | 37.6% | 6.6% | 35.1% | 20.8% | 2982.774 ms |
