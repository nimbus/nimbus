# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 447 | [425, 469] | 440 | 9.0 | 1.00× | 2142.2 | 2997.1 | 3593.8 | 1.0 |
| 32 | 2996 | [2981, 3012] | 3004 | 0.9 | 6.70× | 10574.9 | 11222.4 | 11626.1 | 31.8 |
| 256 | 2441 | [2428, 2454] | 2441 | 1.0 | 5.46× | 65485.6 | 252153.9 | 390396.1 | 239.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 510.251, 463.962, 413.827, 398.040, 433.478, 436.909, 488.233, 485.944, 395.086, 497.467, 413.877, 482.218, 458.887, 439.579, 388.974 |
| 32 | 2945.434, 3012.985, 3012.368, 2992.606, 2926.553, 2996.701, 3021.050, 3014.896, 3002.709, 2996.007, 3020.498, 3015.717, 3003.763, 3009.468, 2974.733 |
| 256 | 2458.605, 2481.494, 2434.786, 2441.979, 2449.634, 2474.266, 2426.850, 2406.858, 2471.074, 2413.992, 2453.398, 2403.263, 2441.185, 2428.612, 2429.953 |

**Peak:** 2996 mut/s at N=32 — 6.70× the sequential (N=1) baseline of 447 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.5% | 0.1% | 0.5% | 95.9% | 1357.915 ms |
| 32 | 0.00 | 48000/0 | 4.0% | 4.1% | 0.3% | 91.6% | 15333.282 ms |
| 256 | 0.00 | 134536/0 | 4.0% | 4.6% | 1.0% | 90.5% | 52083.586 ms |
