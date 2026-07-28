# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 723 | [721, 725] | 723 | 0.4 | 1.00× | 1379.3 | 1422.9 | 1498.2 | 1.0 |
| 32 | 8040 | [7444, 8635] | 8501 | 13.4 | 11.12× | 3731.7 | 4420.8 | 5089.8 | 32.6 |
| 256 | 6670 | [6479, 6861] | 6772 | 5.2 | 9.22× | 28494.2 | 80135.6 | 125748.1 | 242.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 717.891, 719.685, 727.832, 720.045, 726.451, 725.011, 721.286, 725.261, 722.404, 727.530, 723.246, 723.706, 725.741, 718.178, 722.390 |
| 32 | 7795.940, 7926.866, 5376.544, 8519.538, 8566.446, 8751.030, 8500.614, 8687.173, 8309.204, 8611.565, 8314.063, 8650.490, 8541.172, 8474.595, 5568.957 |
| 256 | 6869.590, 6966.488, 6886.518, 6279.960, 6623.412, 6904.632, 6760.668, 6710.998, 6941.262, 6320.609, 6734.852, 6823.369, 6792.441, 6771.764, 5659.564 |

**Peak:** 8040 mut/s at N=32 — 11.12× the sequential (N=1) baseline of 723 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 11.4% | 0.3% | 0.9% | 87.4% | 148.561 ms |
| 32 | 0.00 | 48000/0 | 14.6% | 10.6% | 1.0% | 73.8% | 5610.930 ms |
| 256 | 0.00 | 134400/0 | 13.7% | 12.9% | 1.4% | 72.0% | 17837.025 ms |
