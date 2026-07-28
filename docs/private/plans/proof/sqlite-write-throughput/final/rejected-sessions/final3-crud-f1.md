# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6628 | [6193, 7063] | 7027 | 11.8 | 1.00× | 140.2 | 184.9 | 263.7 | 1.0 |
| 32 | 39189 | [36702, 41677] | 40380 | 11.5 | 5.91× | 725.8 | 1068.1 | 2472.4 | 32.5 |
| 256 | 52179 | [51103, 53254] | 52438 | 3.7 | 7.87× | 4522.3 | 8237.8 | 9573.4 | 251.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 6345.087, 7219.778, 7381.514, 7093.557, 7027.206, 7204.850, 4456.817, 7183.831, 7337.414, 7240.557, 6359.935, 6190.659, 6098.530, 6159.274, 6117.848 |
| 32 | 38205.345, 23347.074, 38953.510, 39397.853, 41028.273, 41836.526, 40600.281, 41294.819, 39810.482, 41640.666, 40729.563, 40087.542, 40380.208, 40624.420, 39900.859 |
| 256 | 50723.887, 51133.952, 53000.062, 51784.207, 54697.092, 54397.385, 52414.914, 54030.056, 52698.103, 53094.244, 53875.647, 49246.517, 52438.028, 51545.065, 47600.288 |

**Peak:** 52179 mut/s at N=256 — 7.87× the sequential (N=1) baseline of 6628 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.5% | 0.7% | 56.1% | 37.8% | 3841.563 ms |
| 32 | 14.58 | 133920/0 | 24.1% | 3.7% | 46.7% | 25.5% | 4039.143 ms |
| 256 | 113.35 | 126720/0 | 37.0% | 6.8% | 35.5% | 20.7% | 2827.206 ms |
