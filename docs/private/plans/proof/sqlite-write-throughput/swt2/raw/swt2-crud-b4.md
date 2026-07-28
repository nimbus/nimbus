# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1308 | [1272, 1344] | 1308 | 5.0 | 1.00× | 744.2 | 959.5 | 1400.8 | 1.0 |
| 32 | 14522 | [14273, 14772] | 14594 | 3.1 | 11.11× | 2101.9 | 3046.4 | 4766.5 | 31.9 |
| 256 | 35627 | [34608, 36646] | 35978 | 5.2 | 27.24× | 6642.6 | 10971.6 | 12942.0 | 249.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1483.158, 1288.234, 1274.153, 1308.364, 1187.457, 1258.540, 1257.114, 1274.601, 1358.692, 1315.001, 1329.082, 1320.495, 1291.116, 1362.123, 1307.596 |
| 32 | 15073.349, 14688.556, 14313.173, 15114.301, 13897.533, 14469.980, 14786.182, 14553.323, 14706.575, 13440.701, 14960.668, 14593.571, 14699.461, 14481.180, 14058.061 |
| 256 | 36669.465, 37452.135, 36105.605, 36845.724, 31887.794, 35768.615, 35803.292, 35043.606, 33664.261, 35977.642, 32524.770, 39089.757, 36139.862, 34997.863, 36435.983 |

**Peak:** 35627 mut/s at N=256 — 27.24× the sequential (N=1) baseline of 1308 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.6% | 0.2% | 54.6% | 43.7% | 3123.017 ms |
| 32 | 16.07 | 133920/0 | 12.9% | 1.7% | 49.2% | 36.2% | 9533.251 ms |
| 256 | 119.10 | 126720/0 | 29.2% | 6.0% | 39.5% | 25.3% | 4004.480 ms |
