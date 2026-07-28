# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 616 | [614, 618] | 616 | 0.5 | 1.00× | 1614.8 | 1690.0 | 1802.5 | 1.0 |
| 32 | 2587 | [2266, 2907] | 2728 | 22.4 | 4.20× | 11508.1 | 24013.0 | 78403.7 | 38.3 |
| 256 | 2202 | [1996, 2409] | 2285 | 16.9 | 3.57× | 78612.1 | 253637.0 | 392518.5 | 257.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 617.608, 614.303, 619.667, 614.037, 613.517, 618.773, 614.915, 619.031, 617.345, 617.117, 610.156, 614.235, 621.682, 615.519, 613.974 |
| 32 | 2705.155, 2679.809, 2797.664, 2830.944, 2811.583, 2699.148, 2783.436, 2566.053, 2771.601, 2555.874, 2927.850, 2909.295, 2727.830, 2488.587, 543.972 |
| 256 | 2368.082, 2448.034, 2200.002, 2231.325, 2363.346, 2267.289, 876.107, 2307.846, 2315.061, 2249.168, 2285.491, 2241.653, 2335.768, 2340.655, 2203.648 |

**Peak:** 2587 mut/s at N=32 — 4.20× the sequential (N=1) baseline of 616 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.5% | 0.1% | 0.3% | 96.1% | 503.208 ms |
| 32 | 0.00 | 48000/0 | 5.8% | 3.4% | 0.4% | 90.4% | 20967.242 ms |
| 256 | 0.00 | 135701/0 | 62.7% | 1.5% | 0.3% | 35.5% | 159097.073 ms |
