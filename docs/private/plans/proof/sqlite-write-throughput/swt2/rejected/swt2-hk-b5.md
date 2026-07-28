# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 561 | [537, 584] | 572 | 7.5 | 1.00× | 1736.4 | 1936.2 | 2828.8 | 1.0 |
| 32 | 2232 | [2101, 2362] | 2275 | 10.6 | 3.98× | 14049.2 | 15251.6 | 16703.2 | 32.4 |
| 256 | 1838 | [1781, 1896] | 1882 | 5.6 | 3.28× | 108881.5 | 284093.0 | 411046.5 | 243.2 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 584.624, 593.591, 590.928, 451.819, 585.447, 572.137, 526.521, 566.141, 569.201, 585.546, 523.339, 618.468, 571.730, 565.825, 506.909 |
| 32 | 1400.501, 2238.082, 2413.150, 2289.652, 2249.071, 2382.094, 2274.557, 2255.038, 2278.864, 2278.194, 2264.048, 2248.173, 2283.499, 2258.809, 2362.449 |
| 256 | 1926.893, 1915.959, 1882.114, 1818.811, 1881.821, 1893.877, 1904.315, 1908.057, 1885.222, 1829.349, 1573.356, 1691.942, 1719.856, 1800.318, 1940.881 |

**Peak:** 2232 mut/s at N=32 — 3.98× the sequential (N=1) baseline of 561 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.3% | 0.1% | 0.5% | 96.1% | 764.947 ms |
| 32 | 0.00 | 48000/0 | 4.0% | 4.0% | 0.5% | 91.5% | 20548.813 ms |
| 256 | 0.00 | 134589/0 | 4.6% | 4.9% | 1.1% | 89.4% | 68549.421 ms |
