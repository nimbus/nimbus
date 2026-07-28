# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1374 | [1290, 1459] | 1329 | 11.1 | 1.00× | 742.7 | 871.2 | 1076.6 | 1.0 |
| 32 | 14871 | [14562, 15179] | 14696 | 3.7 | 10.82× | 2066.8 | 2737.7 | 4645.1 | 31.9 |
| 256 | 37086 | [36228, 37945] | 37049 | 4.2 | 26.98× | 6363.2 | 10549.9 | 12217.6 | 249.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1356.981, 1302.681, 1274.129, 1346.777, 1347.235, 1303.887, 1324.233, 1303.968, 1350.980, 1277.087, 1329.378, 1362.067, 1257.142, 1758.407, 1721.969 |
| 32 | 14106.308, 14557.054, 14998.290, 14696.109, 14020.216, 15208.173, 15158.561, 14661.965, 14669.034, 15203.188, 16238.014, 15175.365, 15369.818, 14442.464, 14554.304 |
| 256 | 39155.274, 38538.000, 39440.374, 39291.044, 36267.855, 37763.204, 35517.493, 36044.185, 37750.765, 36384.041, 36001.835, 37141.304, 37049.145, 35728.635, 34222.282 |

**Peak:** 37086 mut/s at N=256 — 26.98× the sequential (N=1) baseline of 1374 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.4% | 0.2% | 54.4% | 44.0% | 3024.390 ms |
| 32 | 16.09 | 133920/0 | 12.6% | 1.6% | 49.6% | 36.1% | 9329.272 ms |
| 256 | 119.77 | 126720/0 | 29.0% | 5.8% | 40.0% | 25.2% | 3824.078 ms |
