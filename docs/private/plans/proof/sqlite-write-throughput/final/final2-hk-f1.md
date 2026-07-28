# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 625 | [601, 648] | 634 | 6.8 | 1.00× | 1543.8 | 1950.1 | 2403.5 | 1.0 |
| 32 | 7987 | [7607, 8367] | 8099 | 8.6 | 12.79× | 3880.1 | 4443.6 | 4715.9 | 32.1 |
| 256 | 6566 | [5906, 7227] | 6941 | 18.2 | 10.51× | 27737.9 | 78963.7 | 130847.5 | 261.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 665.720, 556.355, 591.079, 639.322, 576.512, 625.635, 684.878, 663.040, 633.823, 654.714, 616.647, 644.537, 663.891, 613.111, 540.090 |
| 32 | 8107.476, 8124.283, 7962.247, 8022.989, 8099.440, 8068.590, 8062.355, 8084.215, 8050.903, 5566.739, 8513.920, 8337.815, 8349.448, 8333.612, 8119.634 |
| 256 | 6940.964, 7005.780, 6736.671, 6635.499, 5989.355, 7110.530, 7091.199, 6255.389, 7225.980, 7317.580, 7221.742, 6999.738, 6566.121, 2469.068, 6929.862 |

**Peak:** 7987 mut/s at N=32 — 12.79× the sequential (N=1) baseline of 625 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 11.3% | 0.4% | 1.4% | 86.9% | 396.731 ms |
| 32 | 0.00 | 48000/0 | 14.1% | 11.1% | 1.0% | 73.8% | 5516.705 ms |
| 256 | 0.00 | 134855/0 | 66.9% | 4.2% | 0.4% | 28.5% | 52876.426 ms |
