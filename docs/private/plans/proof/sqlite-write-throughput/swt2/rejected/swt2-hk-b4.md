# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 480 | [449, 511] | 456 | 11.8 | 1.00× | 2146.8 | 2520.2 | 3158.0 | 1.0 |
| 32 | 2224 | [2196, 2252] | 2202 | 2.2 | 4.64× | 14538.1 | 15392.6 | 15876.5 | 31.8 |
| 256 | 1908 | [1855, 1962] | 1927 | 5.1 | 3.98× | 105217.6 | 270874.9 | 397795.3 | 243.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 559.143, 479.372, 436.511, 450.722, 429.458, 524.570, 456.379, 446.608, 583.469, 562.807, 446.615, 449.060, 496.814, 490.879, 383.873 |
| 32 | 2256.078, 2193.772, 2188.010, 2201.840, 2149.634, 2289.332, 2200.929, 2180.190, 2176.777, 2231.545, 2196.517, 2332.235, 2233.109, 2249.277, 2281.893 |
| 256 | 1979.810, 2001.056, 1981.221, 1958.604, 1954.616, 1942.046, 1916.177, 1884.715, 1870.695, 1927.475, 1908.128, 1942.054, 1871.628, 1590.865, 1893.646 |

**Peak:** 2224 mut/s at N=32 — 4.64× the sequential (N=1) baseline of 480 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.6% | 0.1% | 0.7% | 95.6% | 1137.323 ms |
| 32 | 0.00 | 48000/0 | 4.1% | 3.9% | 0.5% | 91.5% | 20332.017 ms |
| 256 | 0.00 | 134560/0 | 4.5% | 4.7% | 1.1% | 89.7% | 66245.766 ms |
