# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7258 | [7193, 7323] | 7252 | 1.6 | 1.00× | 130.8 | 163.0 | 243.2 | 1.0 |
| 32 | 37183 | [36453, 37912] | 36923 | 3.5 | 5.12× | 786.8 | 1123.7 | 2739.8 | 31.9 |
| 256 | 50073 | [47065, 53081] | 51226 | 10.8 | 6.90× | 4644.7 | 8261.7 | 10067.5 | 255.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7076.438, 6988.459, 7250.624, 7193.691, 7294.493, 7369.702, 7246.457, 7251.632, 7347.336, 7318.077, 7246.712, 7469.701, 7345.043, 7192.908, 7284.036 |
| 32 | 39263.530, 37990.998, 38730.549, 39367.307, 36864.171, 34056.026, 37395.854, 36106.147, 36923.357, 36898.699, 36694.269, 36754.064, 37121.979, 36509.439, 37063.169 |
| 256 | 49459.934, 48925.732, 31141.156, 51533.615, 50848.745, 51296.362, 53535.584, 50934.216, 52624.776, 51226.415, 51008.615, 52639.108, 52015.914, 54159.256, 49742.617 |

**Peak:** 50073 mut/s at N=256 — 6.90× the sequential (N=1) baseline of 7258 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 6.0% | 0.7% | 58.2% | 35.1% | 488.169 ms |
| 32 | 14.70 | 133920/0 | 24.8% | 3.8% | 45.6% | 25.8% | 4171.828 ms |
| 256 | 111.35 | 126720/0 | 36.8% | 6.3% | 36.5% | 20.4% | 3040.334 ms |
