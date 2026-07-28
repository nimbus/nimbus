# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 715 | [705, 724] | 720 | 2.4 | 1.00× | 1386.5 | 1463.2 | 1843.5 | 1.0 |
| 32 | 7920 | [7571, 8268] | 8052 | 7.9 | 11.08× | 3887.5 | 4615.9 | 6465.5 | 32.1 |
| 256 | 6610 | [6093, 7126] | 6868 | 14.1 | 9.25× | 28162.8 | 78733.8 | 120075.6 | 247.6 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 714.412, 712.318, 721.839, 721.940, 725.568, 721.111, 722.284, 655.245, 719.665, 711.439, 717.238, 719.873, 719.444, 712.059, 724.331 |
| 32 | 7751.972, 7547.874, 7635.169, 7735.337, 8051.570, 8145.843, 8355.572, 8143.415, 7860.721, 7927.432, 8274.184, 8176.030, 5969.004, 8598.541, 8623.185 |
| 256 | 6828.807, 3276.243, 6963.800, 6873.243, 6902.204, 6848.455, 6611.222, 6669.639, 7001.727, 6982.766, 7009.089, 6946.442, 6868.377, 6797.468, 6567.869 |

**Peak:** 7920 mut/s at N=32 — 11.08× the sequential (N=1) baseline of 715 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 12.9% | 0.3% | 1.0% | 85.8% | 165.172 ms |
| 32 | 0.00 | 48000/0 | 15.1% | 11.1% | 1.0% | 72.8% | 5545.302 ms |
| 256 | 0.00 | 134555/0 | 35.1% | 9.2% | 0.9% | 54.8% | 24998.454 ms |
