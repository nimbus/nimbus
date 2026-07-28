# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 531 | [517, 545] | 530 | 4.8 | 1.00× | 1865.3 | 2125.8 | 2366.4 | 1.0 |
| 32 | 3046 | [3022, 3069] | 3066 | 1.4 | 5.73× | 10407.3 | 11195.7 | 11733.0 | 31.9 |
| 256 | 2544 | [2533, 2556] | 2544 | 0.8 | 4.79× | 63129.0 | 240927.5 | 371554.6 | 238.9 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 491.638, 473.029, 529.949, 531.755, 523.054, 529.761, 529.486, 538.406, 530.482, 506.493, 562.434, 554.815, 551.262, 564.760, 549.957 |
| 32 | 3079.919, 3066.882, 3022.071, 3065.693, 2956.050, 3016.742, 3046.681, 3068.538, 3111.395, 2977.809, 3085.062, 3069.434, 3077.263, 3033.724, 3008.241 |
| 256 | 2553.027, 2575.866, 2556.071, 2548.496, 2518.289, 2539.780, 2523.186, 2512.161, 2560.834, 2534.516, 2544.171, 2548.932, 2588.301, 2518.243, 2544.251 |

**Peak:** 3046 mut/s at N=32 — 5.73× the sequential (N=1) baseline of 531 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.4% | 0.1% | 0.3% | 96.1% | 853.212 ms |
| 32 | 0.00 | 48000/0 | 4.1% | 4.3% | 0.3% | 91.2% | 15006.519 ms |
| 256 | 0.00 | 134528/0 | 4.3% | 5.0% | 1.0% | 89.7% | 49707.818 ms |
