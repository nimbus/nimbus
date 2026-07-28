# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2036 | [2023, 2049] | 2046 | 1.1 | 1.00× | 483.4 | 537.6 | 657.8 | 1.0 |
| 32 | 16909 | [16859, 16959] | 16904 | 0.5 | 8.31× | 1833.7 | 2039.8 | 3867.6 | 31.9 |
| 256 | 29100 | [28696, 29505] | 29375 | 2.5 | 14.29× | 8286.5 | 13046.6 | 16172.8 | 251.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1982.511, 2047.382, 2046.065, 2045.382, 2045.720, 2047.760, 2051.087, 2009.088, 2063.661, 2047.273, 2018.076, 2011.354, 2037.188, 2065.298, 2021.337 |
| 32 | 17084.579, 16868.250, 17002.109, 16897.258, 16744.207, 16760.356, 16913.014, 16903.735, 16937.008, 16874.074, 16893.532, 17022.054, 16839.616, 16927.632, 16965.776 |
| 256 | 29607.848, 29458.010, 29447.177, 29517.171, 29445.039, 29374.952, 29852.715, 28811.484, 28770.780, 29162.606, 28574.034, 29418.414, 29316.580, 28969.144, 26780.789 |

**Peak:** 29100 mut/s at N=256 — 14.29× the sequential (N=1) baseline of 2036 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.7% | 0.2% | 53.0% | 45.1% | 2052.059 ms |
| 32 | 16.01 | 133920/0 | 11.6% | 1.4% | 55.9% | 31.1% | 8420.595 ms |
| 256 | 126.34 | 126720/0 | 21.2% | 3.9% | 56.7% | 18.3% | 4746.817 ms |
