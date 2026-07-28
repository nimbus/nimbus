# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 440 | [429, 452] | 436 | 4.8 | 1.00× | 2307.7 | 2732.1 | 3315.4 | 1.0 |
| 32 | 2087 | [2061, 2114] | 2098 | 2.3 | 4.74× | 15359.7 | 16423.1 | 17517.0 | 31.9 |
| 256 | 1830 | [1807, 1854] | 1832 | 2.4 | 4.16× | 109325.2 | 282585.6 | 407944.2 | 242.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 449.490, 419.740, 493.293, 413.075, 435.968, 443.915, 472.344, 421.334, 420.820, 449.896, 428.512, 436.351, 436.226, 448.964, 433.537 |
| 32 | 2113.646, 2118.846, 2125.495, 1962.582, 2110.251, 2120.210, 2082.174, 2139.862, 2068.825, 2061.273, 2050.543, 2097.583, 2146.985, 2067.712, 2044.559 |
| 256 | 1836.839, 1837.757, 1827.639, 1816.485, 1801.759, 1811.830, 1736.627, 1810.940, 1932.318, 1870.685, 1870.480, 1831.982, 1835.611, 1790.681, 1845.093 |

**Peak:** 2087 mut/s at N=32 — 4.74× the sequential (N=1) baseline of 440 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.8% | 0.1% | 0.6% | 95.4% | 1376.439 ms |
| 32 | 0.00 | 48000/0 | 4.0% | 4.0% | 0.5% | 91.4% | 21541.789 ms |
| 256 | 0.00 | 134530/0 | 4.6% | 4.9% | 1.1% | 89.5% | 68798.970 ms |
