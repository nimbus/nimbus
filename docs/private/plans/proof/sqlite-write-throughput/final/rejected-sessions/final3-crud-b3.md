# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1946 | [1939, 1954] | 1949 | 0.7 | 1.00× | 502.9 | 582.3 | 707.0 | 1.0 |
| 32 | 16026 | [15833, 16220] | 16117 | 2.2 | 8.23× | 1919.0 | 2216.8 | 3964.8 | 31.9 |
| 256 | 28091 | [27662, 28521] | 28340 | 2.8 | 14.43× | 8560.1 | 13271.0 | 15868.7 | 251.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1917.488, 1954.764, 1955.406, 1957.302, 1955.927, 1939.757, 1959.172, 1960.011, 1946.003, 1955.837, 1948.099, 1922.207, 1941.948, 1949.178, 1933.597 |
| 32 | 15949.098, 16142.876, 14936.040, 16006.900, 16394.104, 15959.780, 16159.443, 16117.446, 16117.006, 15756.522, 16117.402, 16211.039, 15918.124, 16448.719, 16159.510 |
| 256 | 28986.626, 28563.942, 28447.082, 27590.798, 28368.231, 29179.932, 28485.542, 28084.340, 28340.030, 28731.913, 27750.039, 27668.432, 28011.887, 26937.233, 26222.801 |

**Peak:** 28091 mut/s at N=256 — 14.43× the sequential (N=1) baseline of 1946 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.6% | 0.2% | 53.4% | 44.8% | 15022.803 ms |
| 32 | 16.02 | 133920/0 | 11.2% | 1.3% | 56.3% | 31.1% | 8837.261 ms |
| 256 | 123.75 | 126720/0 | 21.2% | 3.8% | 57.2% | 17.8% | 4979.297 ms |
