# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 614 | [610, 617] | 615 | 1.1 | 1.00× | 1619.3 | 1732.2 | 1791.5 | 1.0 |
| 32 | 2841 | [2796, 2886] | 2860 | 2.9 | 4.63× | 11064.2 | 12682.7 | 14334.7 | 31.9 |
| 256 | 2359 | [2274, 2444] | 2409 | 6.5 | 3.84× | 74058.8 | 249278.4 | 380329.8 | 241.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 622.374, 604.019, 620.690, 616.389, 615.076, 620.165, 608.550, 615.601, 618.366, 603.956, 601.549, 617.306, 613.522, 611.391, 614.747 |
| 32 | 2893.507, 2898.857, 2879.170, 2860.458, 2876.189, 2832.374, 2884.708, 2836.669, 2606.813, 2723.301, 2807.991, 2828.286, 2859.064, 2922.539, 2904.864 |
| 256 | 2421.583, 2436.236, 2456.805, 2493.690, 2440.405, 2375.543, 2308.019, 1981.673, 2009.298, 2398.578, 2441.632, 2394.442, 2420.824, 2399.529, 2409.284 |

**Peak:** 2841 mut/s at N=32 — 4.63× the sequential (N=1) baseline of 614 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 3.6% | 0.1% | 0.3% | 96.0% | 511.661 ms |
| 32 | 0.00 | 48000/0 | 3.9% | 4.0% | 0.3% | 91.8% | 16130.041 ms |
| 256 | 0.00 | 134525/0 | 4.2% | 4.4% | 1.0% | 90.4% | 53991.486 ms |
