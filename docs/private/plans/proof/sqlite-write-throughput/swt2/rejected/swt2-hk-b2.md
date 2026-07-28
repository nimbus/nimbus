# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 555 | [519, 592] | 585 | 11.7 | 1.00× | 1730.7 | 2441.0 | 2900.3 | 1.0 |
| 32 | 2067 | [1922, 2212] | 2116 | 12.7 | 3.72× | 14935.0 | 17062.9 | 19388.2 | 32.7 |
| 256 | 1840 | [1810, 1871] | 1847 | 3.0 | 3.31× | 108975.7 | 282058.3 | 405719.5 | 242.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 598.645, 585.155, 594.258, 599.223, 593.190, 596.309, 595.089, 591.100, 585.190, 562.518, 582.588, 548.640, 434.331, 413.086, 452.738 |
| 32 | 2275.435, 1144.956, 2173.069, 2233.893, 2128.027, 2147.792, 2141.010, 2057.689, 2115.926, 2113.689, 2086.667, 2105.603, 2145.654, 2088.564, 2043.140 |
| 256 | 1825.716, 1847.334, 1922.210, 1878.102, 1881.898, 1868.709, 1906.224, 1904.346, 1856.285, 1807.894, 1780.353, 1731.269, 1787.044, 1809.995, 1798.481 |

**Peak:** 2067 mut/s at N=32 — 3.72× the sequential (N=1) baseline of 555 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.5% | 0.1% | 0.6% | 95.8% | 810.725 ms |
| 32 | 0.00 | 48000/0 | 3.8% | 3.8% | 0.5% | 91.9% | 22452.819 ms |
| 256 | 0.00 | 134545/0 | 5.0% | 4.9% | 1.1% | 89.0% | 68794.763 ms |
