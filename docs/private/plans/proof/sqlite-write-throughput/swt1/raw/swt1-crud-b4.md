# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2086 | [2067, 2105] | 2096 | 1.7 | 1.00× | 471.3 | 511.8 | 576.8 | 1.0 |
| 32 | 16770 | [16690, 16850] | 16790 | 0.9 | 8.04× | 1843.8 | 2043.4 | 3789.6 | 32.0 |
| 256 | 29282 | [28927, 29636] | 29294 | 2.2 | 14.04× | 8262.2 | 12561.8 | 15012.5 | 251.7 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2103.455, 2112.325, 1978.817, 2082.597, 2095.763, 2059.206, 2097.700, 2117.140, 2097.413, 2059.387, 2088.910, 2108.243, 2078.913, 2088.274, 2121.831 |
| 32 | 16926.620, 16483.700, 16537.331, 16690.640, 16808.413, 16662.684, 16949.869, 16706.532, 17009.928, 16878.449, 16768.302, 16718.085, 16789.817, 16824.945, 16800.841 |
| 256 | 29106.223, 29063.949, 28485.450, 30214.714, 30109.225, 29705.481, 28121.364, 29455.387, 29285.909, 29294.384, 29514.537, 28948.389, 30190.450, 29401.477, 28329.204 |

**Peak:** 29282 mut/s at N=256 — 14.04× the sequential (N=1) baseline of 2086 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.4% | 0.2% | 53.8% | 44.6% | 2011.380 ms |
| 32 | 16.01 | 133920/0 | 11.3% | 1.2% | 56.7% | 30.8% | 8453.929 ms |
| 256 | 127.36 | 126720/0 | 21.3% | 3.7% | 57.0% | 17.9% | 4761.007 ms |
