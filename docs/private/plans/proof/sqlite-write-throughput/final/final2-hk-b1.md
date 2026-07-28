# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 546 | [535, 557] | 544 | 3.6 | 1.00× | 1817.4 | 2011.8 | 2236.0 | 1.0 |
| 32 | 3035 | [3014, 3055] | 3047 | 1.2 | 5.56× | 10426.8 | 11164.5 | 11832.2 | 31.9 |
| 256 | 2559 | [2546, 2571] | 2559 | 0.9 | 4.69× | 61920.6 | 241954.9 | 376645.7 | 238.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 533.664, 559.108, 578.457, 543.637, 547.075, 518.559, 531.592, 535.432, 540.943, 511.141, 538.692, 564.583, 576.792, 560.799, 548.766 |
| 32 | 3065.680, 3034.008, 3066.788, 3051.578, 2994.676, 3058.281, 2938.564, 2975.967, 3053.686, 3046.223, 3063.276, 3038.528, 3034.427, 3053.291, 3046.640 |
| 256 | 2562.143, 2604.825, 2561.539, 2557.473, 2545.628, 2591.343, 2564.681, 2535.251, 2558.947, 2557.322, 2561.516, 2502.612, 2566.499, 2556.557, 2554.864 |

**Peak:** 3035 mut/s at N=32 — 5.56× the sequential (N=1) baseline of 546 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.6% | 0.1% | 0.4% | 96.0% | 778.912 ms |
| 32 | 0.00 | 48000/0 | 4.1% | 4.3% | 0.3% | 91.2% | 15067.188 ms |
| 256 | 0.00 | 134543/0 | 4.3% | 4.9% | 1.0% | 89.8% | 49352.598 ms |
