# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1929 | [1871, 1986] | 1951 | 5.4 | 1.00× | 496.5 | 577.1 | 773.1 | 1.0 |
| 32 | 15913 | [15750, 16077] | 15915 | 1.9 | 8.25× | 1940.8 | 2259.1 | 4026.2 | 31.9 |
| 256 | 27813 | [27489, 28136] | 27956 | 2.1 | 14.42× | 8756.9 | 13232.8 | 15523.0 | 251.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1968.708, 1962.456, 1936.862, 1632.022, 1951.281, 1943.307, 1972.156, 1987.887, 1947.272, 1946.177, 1737.617, 1961.007, 2020.664, 2013.645, 1951.350 |
| 32 | 16421.525, 16343.154, 15380.657, 16155.952, 15501.054, 15914.798, 15943.507, 16086.294, 15903.173, 15928.096, 15697.902, 15847.667, 15601.741, 15793.240, 16181.926 |
| 256 | 28187.220, 27408.493, 27581.257, 28246.119, 27955.771, 28806.797, 27993.400, 26857.067, 26614.356, 27693.023, 28042.459, 27606.294, 27467.610, 28416.578, 28312.866 |

**Peak:** 27813 mut/s at N=256 — 14.42× the sequential (N=1) baseline of 1929 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.5% | 0.2% | 54.3% | 43.9% | 15248.991 ms |
| 32 | 16.01 | 133920/0 | 11.4% | 1.4% | 56.1% | 31.2% | 8898.106 ms |
| 256 | 124.11 | 126720/0 | 21.7% | 3.8% | 56.3% | 18.3% | 5049.192 ms |
