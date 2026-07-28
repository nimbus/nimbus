# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 608 | [607, 610] | 608 | 0.5 | 1.00× | 1633.5 | 1731.5 | 1782.5 | 1.0 |
| 32 | 2816 | [2791, 2841] | 2829 | 1.6 | 4.63× | 11227.0 | 12275.4 | 13049.9 | 31.9 |
| 256 | 2367 | [2352, 2382] | 2371 | 1.1 | 3.89× | 68697.6 | 256023.0 | 394883.2 | 239.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 608.068, 607.313, 603.637, 608.261, 615.032, 607.967, 608.670, 610.507, 607.520, 606.220, 607.420, 603.207, 614.074, 611.281, 606.711 |
| 32 | 2837.417, 2798.989, 2812.327, 2736.221, 2755.030, 2906.096, 2859.778, 2780.730, 2840.091, 2864.319, 2829.327, 2833.869, 2759.522, 2794.339, 2831.807 |
| 256 | 2370.853, 2346.666, 2348.633, 2373.496, 2371.529, 2381.652, 2357.638, 2364.561, 2396.212, 2381.203, 2316.469, 2346.595, 2406.887, 2413.751, 2333.327 |

**Peak:** 2816 mut/s at N=32 — 4.63× the sequential (N=1) baseline of 608 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.4% | 0.1% | 0.3% | 96.2% | 525.658 ms |
| 32 | 0.00 | 48000/0 | 3.9% | 3.9% | 0.3% | 91.9% | 16181.262 ms |
| 256 | 0.00 | 134548/0 | 4.3% | 4.4% | 1.0% | 90.3% | 53657.247 ms |
