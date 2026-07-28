# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 5537 | [4957, 6117] | 5534 | 18.9 | 1.00× | 145.0 | 238.7 | 413.9 | 1.0 |
| 32 | 29825 | [27598, 32052] | 30142 | 13.5 | 5.39× | 956.4 | 1681.5 | 3255.7 | 32.7 |
| 256 | 44769 | [44016, 45522] | 44643 | 3.0 | 8.08× | 5142.6 | 9329.1 | 12439.0 | 250.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 6450.792, 4129.695, 5122.069, 6159.216, 4979.057, 5334.147, 5534.166, 6063.464, 6749.384, 6901.073, 5050.052, 5780.147, 3450.666, 6969.889, 4386.946 |
| 32 | 29762.282, 31594.136, 30141.665, 29221.580, 29291.531, 16591.507, 29652.309, 31024.561, 29994.041, 30486.797, 29380.272, 30670.272, 30996.273, 35586.510, 32983.658 |
| 256 | 44642.531, 43543.436, 46243.365, 44057.338, 45942.247, 45117.641, 43708.025, 43915.343, 42679.690, 46166.163, 42595.316, 46102.685, 46792.609, 44208.050, 45816.990 |

**Peak:** 44769 mut/s at N=256 — 8.08× the sequential (N=1) baseline of 5537 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 4.7% | 0.7% | 60.2% | 34.5% | 672.721 ms |
| 32 | 15.11 | 133920/0 | 22.2% | 4.0% | 47.3% | 26.5% | 5048.383 ms |
| 256 | 116.15 | 126720/0 | 34.8% | 6.6% | 37.0% | 21.5% | 3139.625 ms |
