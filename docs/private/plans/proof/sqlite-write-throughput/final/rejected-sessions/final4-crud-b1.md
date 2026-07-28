# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1950 | [1923, 1976] | 1953 | 2.5 | 1.00× | 498.2 | 583.5 | 743.0 | 1.0 |
| 32 | 16629 | [16498, 16760] | 16664 | 1.4 | 8.53× | 1851.5 | 2129.5 | 3871.0 | 31.9 |
| 256 | 28375 | [28061, 28690] | 28396 | 2.0 | 14.55× | 8511.7 | 13268.3 | 16147.5 | 251.2 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1979.343, 1948.401, 1964.330, 1976.724, 1946.885, 1952.458, 1942.255, 1974.848, 1957.524, 1925.145, 1937.908, 1952.843, 1790.941, 1997.117, 1996.523 |
| 32 | 16094.196, 16687.338, 16684.760, 16469.227, 17086.956, 16744.887, 16967.914, 16428.334, 16793.037, 16664.310, 16513.457, 16692.653, 16442.316, 16593.253, 16573.075 |
| 256 | 28449.717, 28166.971, 28889.055, 28346.183, 28817.749, 28586.873, 28986.191, 28622.500, 28230.132, 29530.317, 27753.610, 27734.495, 27720.105, 28395.815, 27400.593 |

**Peak:** 28375 mut/s at N=256 — 14.55× the sequential (N=1) baseline of 1950 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.6% | 0.2% | 53.4% | 44.8% | 14959.921 ms |
| 32 | 16.00 | 133920/0 | 11.4% | 1.4% | 56.2% | 31.0% | 8535.707 ms |
| 256 | 126.85 | 126720/0 | 21.1% | 3.6% | 57.1% | 18.2% | 4827.399 ms |
