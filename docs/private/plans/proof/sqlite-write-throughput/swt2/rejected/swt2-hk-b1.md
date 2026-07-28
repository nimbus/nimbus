# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 499 | [465, 534] | 485 | 12.5 | 1.00× | 1811.9 | 2628.1 | 3159.1 | 1.0 |
| 32 | 2120 | [2091, 2148] | 2124 | 2.4 | 4.25× | 15094.9 | 16262.8 | 18128.1 | 31.9 |
| 256 | 1865 | [1851, 1880] | 1870 | 1.4 | 3.74× | 107618.8 | 276008.4 | 396068.1 | 242.6 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 436.453, 485.242, 466.007, 577.657, 481.476, 503.052, 456.774, 432.108, 577.353, 575.295, 496.757, 399.261, 449.042, 586.493, 566.481 |
| 32 | 2046.476, 2047.738, 2141.957, 2246.825, 2112.708, 2133.911, 2170.626, 2133.308, 2075.978, 2081.507, 2118.198, 2071.513, 2124.177, 2158.141, 2131.494 |
| 256 | 1892.486, 1870.160, 1893.183, 1863.295, 1854.613, 1899.366, 1883.751, 1866.939, 1876.438, 1874.978, 1837.440, 1806.642, 1881.107, 1848.078, 1833.310 |

**Peak:** 2120 mut/s at N=32 — 4.25× the sequential (N=1) baseline of 499 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.8% | 0.1% | 0.6% | 95.4% | 1062.966 ms |
| 32 | 0.00 | 48000/0 | 4.0% | 3.9% | 0.5% | 91.5% | 21283.549 ms |
| 256 | 0.00 | 134514/0 | 4.5% | 4.8% | 1.1% | 89.6% | 67614.226 ms |
