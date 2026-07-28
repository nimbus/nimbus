# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 546 | [501, 591] | 566 | 14.9 | 1.00× | 1629.7 | 2835.8 | 4000.2 | 1.0 |
| 32 | 2871 | [2790, 2952] | 2911 | 5.1 | 5.26× | 10734.9 | 12604.6 | 15433.5 | 31.9 |
| 256 | 2295 | [2117, 2474] | 2260 | 14.0 | 4.21× | 80187.5 | 258492.4 | 426952.5 | 246.2 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 426.541, 463.164, 565.936, 493.854, 377.712, 556.962, 482.180, 614.347, 508.651, 621.406, 600.630, 609.078, 615.786, 621.453, 626.590 |
| 32 | 2801.862, 2885.167, 2979.270, 2938.375, 2970.499, 2803.552, 2940.612, 2922.365, 2487.779, 3050.839, 2762.334, 3078.430, 2769.572, 2768.938, 2911.298 |
| 256 | 2595.428, 2042.218, 2725.423, 2260.142, 2619.928, 2665.066, 2088.792, 2141.619, 1771.281, 1947.606, 1991.707, 1960.888, 2648.868, 2417.325, 2551.131 |

**Peak:** 2871 mut/s at N=32 — 5.26× the sequential (N=1) baseline of 546 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.8% | 0.1% | 0.5% | 95.6% | 807.008 ms |
| 32 | 0.00 | 48000/0 | 4.0% | 4.0% | 0.4% | 91.7% | 15911.188 ms |
| 256 | 0.00 | 134741/0 | 5.1% | 4.5% | 1.3% | 89.1% | 54716.089 ms |
