# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2040 | [2014, 2066] | 2049 | 2.3 | 1.00× | 480.5 | 548.1 | 666.9 | 1.0 |
| 32 | 15885 | [15650, 16119] | 15894 | 2.7 | 7.79× | 1931.7 | 2326.6 | 4056.0 | 31.9 |
| 256 | 27953 | [27608, 28298] | 27931 | 2.2 | 13.70× | 8676.5 | 13320.5 | 16040.5 | 251.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2081.655, 2048.734, 2045.248, 2062.983, 2048.584, 2046.234, 2076.768, 2100.404, 2052.253, 2040.712, 2048.332, 2052.892, 1903.134, 1995.406, 1996.005 |
| 32 | 16211.825, 15335.237, 15893.862, 15892.590, 15789.079, 15177.679, 16264.902, 16418.147, 16277.347, 16355.896, 15320.008, 15833.315, 15940.716, 15314.359, 16244.411 |
| 256 | 27894.652, 27121.111, 28994.112, 28265.462, 28301.993, 28273.924, 27931.232, 27482.112, 26855.363, 28824.919, 27828.478, 27103.944, 27896.886, 27936.254, 28585.048 |

**Peak:** 27953 mut/s at N=256 — 13.70× the sequential (N=1) baseline of 2040 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.5% | 0.2% | 53.5% | 44.8% | 2054.880 ms |
| 32 | 16.01 | 133920/0 | 11.2% | 1.4% | 56.4% | 31.0% | 8922.777 ms |
| 256 | 123.99 | 126720/0 | 21.6% | 3.7% | 56.5% | 18.1% | 5035.593 ms |
