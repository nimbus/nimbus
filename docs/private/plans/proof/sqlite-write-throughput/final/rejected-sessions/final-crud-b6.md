# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1964 | [1802, 2126] | 2041 | 14.9 | 1.00× | 480.8 | 564.2 | 769.3 | 1.0 |
| 32 | 16097 | [15987, 16206] | 16076 | 1.2 | 8.19× | 1921.1 | 2212.5 | 3997.8 | 31.9 |
| 256 | 27566 | [27208, 27925] | 27755 | 2.4 | 14.03× | 8773.8 | 13452.5 | 15713.2 | 251.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2016.735, 2060.434, 2081.479, 2027.163, 2058.984, 2015.660, 2045.503, 1997.736, 2074.603, 2043.625, 2002.119, 2037.747, 909.053, 2041.470, 2051.429 |
| 32 | 16303.927, 16235.384, 16552.129, 16291.211, 16141.319, 16076.143, 16129.188, 15723.970, 15911.944, 16071.081, 15990.985, 16092.191, 16054.128, 15933.743, 15945.976 |
| 256 | 28013.586, 26140.356, 26364.984, 27755.407, 27244.849, 27405.874, 28117.808, 27842.338, 26976.769, 28001.887, 27674.811, 27871.357, 27937.274, 27649.132, 28501.063 |

**Peak:** 27566 mut/s at N=256 — 14.03× the sequential (N=1) baseline of 1964 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.3% | 0.2% | 50.3% | 48.2% | 2236.660 ms |
| 32 | 16.01 | 133920/0 | 11.3% | 1.3% | 56.4% | 30.9% | 8780.269 ms |
| 256 | 125.59 | 126720/0 | 21.5% | 3.9% | 56.7% | 17.8% | 5058.220 ms |
