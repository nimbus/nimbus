# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6907 | [6419, 7396] | 7290 | 12.8 | 1.00× | 134.5 | 177.5 | 260.8 | 1.0 |
| 32 | 38456 | [37631, 39280] | 38892 | 3.9 | 5.57× | 763.5 | 1136.3 | 2515.8 | 31.9 |
| 256 | 52002 | [50693, 53311] | 51746 | 4.5 | 7.53× | 4512.8 | 8016.3 | 9843.1 | 251.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7328.864, 7302.786, 7329.971, 7392.640, 7262.553, 7205.857, 7272.134, 6688.336, 6238.066, 6158.847, 4081.737, 7372.364, 7365.282, 7320.745, 7289.834 |
| 32 | 38892.280, 39813.937, 39265.394, 39940.084, 40669.929, 40146.660, 39652.860, 38951.385, 36557.595, 35877.022, 37923.986, 37047.308, 36755.526, 37806.746, 37532.633 |
| 256 | 50349.163, 51327.658, 50541.934, 54506.610, 53330.808, 53658.849, 51462.040, 51796.616, 49879.749, 49989.571, 51745.616, 53223.682, 55286.134, 55952.022, 46974.686 |

**Peak:** 52002 mut/s at N=256 — 7.53× the sequential (N=1) baseline of 6907 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.6% | 0.7% | 60.2% | 33.6% | 3711.990 ms |
| 32 | 14.52 | 133920/0 | 25.0% | 3.8% | 44.6% | 26.6% | 4054.070 ms |
| 256 | 112.74 | 126720/0 | 37.0% | 6.8% | 35.9% | 20.3% | 2856.204 ms |
