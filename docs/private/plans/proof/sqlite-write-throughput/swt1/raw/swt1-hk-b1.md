# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 522 | [495, 549] | 522 | 9.4 | 1.00× | 1904.0 | 2371.1 | 3017.2 | 1.0 |
| 32 | 3154 | [3139, 3169] | 3154 | 0.9 | 6.04× | 10082.6 | 10723.5 | 11170.4 | 31.8 |
| 256 | 2616 | [2591, 2642] | 2598 | 1.8 | 5.01× | 61970.9 | 234498.0 | 361871.7 | 239.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 541.380, 621.530, 592.343, 540.959, 536.834, 543.007, 522.004, 522.307, 518.909, 516.928, 481.006, 408.827, 477.547, 488.150, 518.144 |
| 32 | 3075.050, 3195.457, 3167.598, 3179.773, 3176.594, 3134.482, 3145.011, 3153.788, 3168.651, 3141.765, 3153.942, 3149.103, 3168.127, 3150.196, 3146.269 |
| 256 | 2668.797, 2697.267, 2681.403, 2639.599, 2583.229, 2688.659, 2587.434, 2565.998, 2606.631, 2578.274, 2576.435, 2598.399, 2610.634, 2583.500, 2577.568 |

**Peak:** 3154 mut/s at N=32 — 6.04× the sequential (N=1) baseline of 522 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.5% | 0.1% | 0.4% | 96.0% | 919.533 ms |
| 32 | 0.00 | 48000/0 | 4.1% | 4.3% | 0.4% | 91.3% | 14483.194 ms |
| 256 | 0.00 | 134521/0 | 4.8% | 4.9% | 1.0% | 89.4% | 48661.497 ms |
