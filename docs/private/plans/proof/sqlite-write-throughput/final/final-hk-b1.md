# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 524 | [499, 549] | 509 | 8.6 | 1.00× | 1917.8 | 2241.3 | 2947.1 | 1.0 |
| 32 | 3065 | [3040, 3090] | 3084 | 1.5 | 5.84× | 10336.8 | 11093.9 | 11815.5 | 31.9 |
| 256 | 2556 | [2545, 2568] | 2556 | 0.8 | 4.87× | 62639.4 | 240111.0 | 375629.4 | 239.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 547.959, 560.942, 530.250, 509.310, 484.560, 491.997, 505.833, 516.107, 481.431, 467.908, 591.231, 618.259, 575.978, 505.684, 479.551 |
| 32 | 3080.894, 3091.965, 3104.184, 3009.400, 3109.299, 3091.227, 3084.460, 3085.752, 3075.021, 3106.435, 3086.494, 3045.460, 2947.853, 3019.103, 3032.305 |
| 256 | 2572.026, 2605.319, 2574.291, 2565.847, 2560.703, 2555.882, 2542.946, 2544.810, 2554.010, 2556.160, 2537.432, 2557.715, 2571.652, 2522.888, 2522.888 |

**Peak:** 3065 mut/s at N=32 — 5.84× the sequential (N=1) baseline of 524 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.6% | 0.1% | 0.4% | 96.0% | 897.987 ms |
| 32 | 0.00 | 48000/0 | 4.1% | 4.3% | 0.4% | 91.2% | 14923.459 ms |
| 256 | 0.00 | 134519/0 | 4.4% | 5.0% | 1.0% | 89.6% | 49526.771 ms |
