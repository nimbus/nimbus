# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 498 | [476, 520] | 508 | 8.0 | 1.00× | 1952.7 | 2545.8 | 3315.0 | 1.0 |
| 32 | 2848 | [2712, 2984] | 2861 | 8.6 | 5.72× | 10799.3 | 14122.0 | 18777.1 | 32.1 |
| 256 | 2481 | [2339, 2624] | 2531 | 10.4 | 4.98× | 64516.6 | 240550.8 | 385572.0 | 243.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 529.844, 512.976, 482.170, 430.572, 472.739, 560.931, 527.681, 494.714, 515.190, 543.071, 536.269, 463.386, 508.142, 462.758, 432.329 |
| 32 | 3173.658, 3184.367, 3091.567, 2812.358, 3078.731, 2861.398, 3100.683, 2561.910, 2751.147, 2603.992, 2367.956, 2631.934, 2902.102, 2736.813, 2860.978 |
| 256 | 2626.849, 2716.408, 2613.662, 2480.685, 2662.036, 1613.658, 2639.643, 2534.224, 2494.134, 2418.398, 2548.272, 2418.328, 2530.769, 2503.262, 2418.779 |

**Peak:** 2848 mut/s at N=32 — 5.72× the sequential (N=1) baseline of 498 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.7% | 0.1% | 0.5% | 95.7% | 1026.050 ms |
| 32 | 0.00 | 48000/0 | 4.0% | 4.2% | 0.4% | 91.4% | 16119.277 ms |
| 256 | 0.00 | 135008/0 | 38.6% | 3.0% | 0.6% | 57.7% | 80960.560 ms |
