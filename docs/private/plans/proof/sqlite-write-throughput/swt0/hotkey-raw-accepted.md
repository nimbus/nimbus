# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 559 | [549, 570] | 562 | 3.3 | 1.00× | 1773.0 | 1972.1 | 2086.0 | 1.0 |
| 32 | 3150 | [3128, 3173] | 3169 | 1.3 | 5.63× | 10080.8 | 10838.1 | 11457.1 | 31.9 |
| 256 | 2639 | [2623, 2655] | 2631 | 1.1 | 4.72× | 60260.5 | 234241.8 | 360181.3 | 239.2 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 514.693, 571.034, 557.886, 561.887, 577.484, 561.132, 576.486, 567.467, 564.692, 553.514, 529.976, 541.361, 582.367, 556.505, 573.845 |
| 32 | 3158.636, 3179.125, 3044.049, 3141.273, 3193.067, 3178.786, 3181.899, 3114.015, 3189.717, 3169.348, 3127.958, 3113.805, 3115.283, 3175.683, 3173.619 |
| 256 | 2680.443, 2679.074, 2665.544, 2648.512, 2652.670, 2674.838, 2617.181, 2664.343, 2621.075, 2618.691, 2602.748, 2605.369, 2630.939, 2612.099, 2606.669 |

**Peak:** 3150 mut/s at N=32 — 5.63× the sequential (N=1) baseline of 559 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.5% | 0.1% | 0.3% | 96.1% | 726.562 ms |
| 32 | 0.00 | 48000/0 | 4.0% | 4.4% | 0.4% | 91.2% | 14472.867 ms |
| 256 | 0.00 | 134506/0 | 4.4% | 4.9% | 1.0% | 89.7% | 48008.491 ms |
