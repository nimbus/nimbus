# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 598 | [593, 603] | 596 | 1.4 | 1.00× | 1667.2 | 1764.9 | 1856.9 | 1.0 |
| 32 | 2902 | [2877, 2927] | 2900 | 1.5 | 4.85× | 10932.0 | 11815.6 | 12783.9 | 31.9 |
| 256 | 2397 | [2356, 2437] | 2398 | 3.1 | 4.01× | 68393.2 | 253000.1 | 391385.4 | 239.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 609.761, 602.998, 612.193, 592.238, 595.804, 584.175, 596.275, 597.423, 588.798, 610.865, 592.796, 596.293, 606.113, 595.778, 588.783 |
| 32 | 2779.096, 2887.770, 2888.746, 2907.666, 2895.155, 2941.599, 2924.976, 2900.047, 2912.261, 2878.292, 2962.894, 2966.943, 2925.778, 2870.210, 2891.992 |
| 256 | 2468.242, 2505.572, 2492.982, 2476.680, 2459.162, 2404.561, 2352.783, 2302.693, 2381.015, 2414.752, 2270.711, 2369.250, 2398.446, 2352.590, 2302.478 |

**Peak:** 2902 mut/s at N=32 — 4.85× the sequential (N=1) baseline of 598 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.6% | 0.1% | 0.4% | 95.9% | 565.080 ms |
| 32 | 0.00 | 48000/0 | 3.9% | 4.0% | 0.4% | 91.7% | 15784.218 ms |
| 256 | 0.00 | 134551/0 | 4.5% | 4.4% | 1.0% | 90.1% | 53133.229 ms |
