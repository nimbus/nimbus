# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1920 | [1854, 1985] | 1951 | 6.1 | 1.00× | 498.5 | 577.8 | 767.4 | 1.0 |
| 32 | 15925 | [15227, 16623] | 16553 | 7.9 | 8.30× | 1888.0 | 2211.6 | 3947.6 | 32.0 |
| 256 | 27702 | [27353, 28051] | 27843 | 2.3 | 14.43× | 8794.1 | 13011.0 | 15733.5 | 251.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2012.714, 2004.027, 1950.799, 1660.488, 1989.089, 1998.499, 1990.569, 1959.905, 1901.238, 1621.392, 1966.683, 1940.773, 1934.096, 1947.681, 1916.109 |
| 32 | 15671.569, 15583.656, 13496.636, 16876.229, 16309.695, 16568.570, 16552.759, 16696.762, 16743.504, 16832.409, 16795.518, 16661.672, 12660.505, 15417.221, 16007.869 |
| 256 | 27276.411, 27930.047, 27843.459, 28113.043, 27137.747, 28538.922, 26944.020, 28184.423, 28566.264, 28079.832, 28511.379, 26665.653, 27058.277, 27266.522, 27408.067 |

**Peak:** 27702 mut/s at N=256 — 14.43× the sequential (N=1) baseline of 1920 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.5% | 0.2% | 53.2% | 45.1% | 15344.964 ms |
| 32 | 16.00 | 133920/0 | 10.9% | 1.3% | 57.0% | 30.7% | 8927.775 ms |
| 256 | 124.48 | 126720/0 | 21.6% | 3.9% | 56.8% | 17.6% | 5043.990 ms |
