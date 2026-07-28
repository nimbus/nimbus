# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7162 | [6995, 7328] | 7297 | 4.2 | 1.00× | 132.8 | 171.8 | 232.2 | 1.0 |
| 32 | 36244 | [35959, 36528] | 36246 | 1.4 | 5.06× | 813.5 | 1165.5 | 2629.5 | 31.8 |
| 256 | 50305 | [49145, 51464] | 50293 | 4.2 | 7.02× | 4697.4 | 8377.3 | 9856.4 | 251.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7351.464, 7418.881, 7296.621, 7362.681, 7331.177, 7312.754, 7217.919, 7055.922, 6707.410, 6931.895, 7258.874, 7368.572, 7188.163, 7296.655, 6323.817 |
| 32 | 35901.344, 37095.771, 35429.069, 36426.574, 36300.651, 37004.167, 35936.724, 36640.037, 35855.698, 35529.679, 36096.159, 36979.650, 35936.218, 36278.046, 36245.620 |
| 256 | 53152.495, 51340.304, 50959.295, 52223.542, 49203.016, 47871.888, 48741.965, 48556.538, 51882.530, 50293.298, 50098.590, 46471.906, 52743.726, 52978.831, 48049.984 |

**Peak:** 50305 mut/s at N=256 — 7.02× the sequential (N=1) baseline of 7162 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.8% | 0.7% | 58.5% | 35.1% | 3495.937 ms |
| 32 | 14.43 | 133920/0 | 25.0% | 3.8% | 45.4% | 25.8% | 4287.968 ms |
| 256 | 112.54 | 126720/0 | 37.8% | 6.9% | 36.4% | 18.9% | 2976.082 ms |
