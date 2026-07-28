# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 690 | [681, 699] | 689 | 2.3 | 1.00× | 1447.5 | 1529.4 | 1635.8 | 1.0 |
| 32 | 8046 | [7841, 8252] | 7885 | 4.6 | 11.66× | 3927.8 | 4553.5 | 4869.8 | 31.9 |
| 256 | 6441 | [5770, 7112] | 6885 | 18.8 | 9.34× | 28208.3 | 81372.5 | 134104.1 | 256.6 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 675.666, 667.224, 680.150, 721.838, 716.551, 697.653, 683.561, 708.332, 692.427, 686.250, 672.717, 688.532, 691.584, 674.755, 692.118 |
| 32 | 8346.822, 8512.165, 8443.576, 8520.498, 8532.264, 8382.456, 8145.141, 7688.126, 7603.854, 7700.571, 7760.892, 7884.624, 7642.536, 7758.936, 7774.215 |
| 256 | 7169.370, 7224.950, 7197.364, 6998.105, 2345.241, 6713.267, 5812.410, 6429.630, 6971.928, 7078.293, 6234.477, 6335.539, 6227.853, 6989.624, 6884.926 |

**Peak:** 8046 mut/s at N=32 — 11.66× the sequential (N=1) baseline of 690 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 11.6% | 0.3% | 1.0% | 87.2% | 221.755 ms |
| 32 | 0.00 | 48000/0 | 12.3% | 12.0% | 1.1% | 74.7% | 5265.739 ms |
| 256 | 0.00 | 134700/0 | 57.8% | 5.4% | 0.6% | 36.2% | 42378.189 ms |
