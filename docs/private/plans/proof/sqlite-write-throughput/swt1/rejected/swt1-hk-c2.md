# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 470 | [441, 499] | 462 | 11.1 | 1.00× | 2047.0 | 2872.2 | 3379.7 | 1.0 |
| 32 | 3007 | [3000, 3015] | 3009 | 0.5 | 6.40× | 10557.1 | 11183.1 | 11542.3 | 31.8 |
| 256 | 2469 | [2457, 2482] | 2469 | 0.9 | 5.25× | 64016.1 | 250808.5 | 391581.8 | 238.9 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 419.151, 535.018, 568.032, 525.877, 444.715, 508.602, 472.296, 461.966, 457.322, 383.348, 479.675, 453.699, 427.684, 402.293, 512.746 |
| 32 | 3033.477, 3002.706, 2978.458, 3010.547, 3001.136, 3019.978, 3024.772, 3011.585, 3001.814, 3014.232, 3011.495, 2997.641, 3009.343, 3002.533, 2988.063 |
| 256 | 2434.648, 2469.474, 2463.114, 2458.258, 2455.242, 2478.528, 2443.999, 2439.574, 2485.370, 2505.165, 2473.688, 2461.023, 2511.589, 2481.137, 2481.382 |

**Peak:** 3007 mut/s at N=32 — 6.40× the sequential (N=1) baseline of 470 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.6% | 0.1% | 0.5% | 95.9% | 1221.892 ms |
| 32 | 0.00 | 48000/0 | 4.0% | 4.0% | 0.3% | 91.7% | 15284.399 ms |
| 256 | 0.00 | 134576/0 | 4.0% | 4.5% | 0.9% | 90.5% | 51523.968 ms |
