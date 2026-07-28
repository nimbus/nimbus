# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 626 | [601, 651] | 643 | 7.3 | 1.00× | 1545.9 | 2000.2 | 2482.9 | 1.0 |
| 32 | 7760 | [7716, 7805] | 7744 | 1.0 | 12.40× | 4070.2 | 4632.1 | 4856.8 | 31.8 |
| 256 | 6459 | [5818, 7100] | 6554 | 17.9 | 10.32× | 27663.0 | 82460.2 | 138992.8 | 258.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 654.584, 602.995, 557.230, 595.249, 662.821, 654.765, 648.699, 708.608, 675.023, 594.923, 591.623, 653.548, 596.210, 642.880, 548.552 |
| 32 | 7872.201, 7897.354, 7843.636, 7862.363, 7744.395, 7667.071, 7734.880, 7679.545, 7797.284, 7771.491, 7701.285, 7735.036, 7745.554, 7724.557, 7629.565 |
| 256 | 6453.140, 6554.029, 6473.381, 6391.205, 5978.056, 6490.964, 6713.329, 5920.040, 7326.574, 7419.018, 6791.847, 7206.175, 7329.610, 7191.439, 2647.069 |

**Peak:** 7760 mut/s at N=32 — 12.40× the sequential (N=1) baseline of 626 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 11.8% | 0.4% | 1.5% | 86.3% | 389.527 ms |
| 32 | 0.00 | 48000/0 | 11.6% | 12.0% | 1.1% | 75.3% | 5394.499 ms |
| 256 | 0.00 | 134859/0 | 63.0% | 4.7% | 0.5% | 31.8% | 47541.391 ms |
