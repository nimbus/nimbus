# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1994 | [1959, 2030] | 2004 | 3.2 | 1.00× | 490.0 | 573.1 | 699.2 | 1.0 |
| 32 | 16264 | [16107, 16422] | 16279 | 1.8 | 8.16× | 1895.1 | 2180.8 | 3993.2 | 31.9 |
| 256 | 28527 | [28127, 28927] | 28443 | 2.5 | 14.30× | 8484.0 | 13112.7 | 15986.9 | 251.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2050.039, 2024.886, 2031.053, 2073.967, 2074.373, 2059.155, 2069.986, 2003.634, 1920.271, 1912.847, 1939.968, 1966.892, 1948.027, 1941.444, 1898.607 |
| 32 | 16145.914, 16553.251, 16254.781, 16279.102, 15989.405, 16144.808, 15469.305, 16447.444, 16371.656, 16352.068, 16145.815, 16248.594, 16700.165, 16480.114, 16384.025 |
| 256 | 28442.729, 28890.002, 28734.034, 29684.879, 27698.202, 28104.322, 28189.207, 29090.504, 27726.276, 29056.398, 29796.669, 28416.594, 28867.942, 27852.026, 27358.631 |

**Peak:** 28527 mut/s at N=256 — 14.30× the sequential (N=1) baseline of 1994 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.5% | 0.2% | 53.5% | 44.8% | 2101.275 ms |
| 32 | 16.00 | 133920/0 | 11.4% | 1.4% | 56.0% | 31.3% | 8725.021 ms |
| 256 | 123.87 | 126720/0 | 21.0% | 3.8% | 56.7% | 18.5% | 4886.708 ms |
