# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6581 | [6101, 7060] | 6353 | 13.2 | 1.00× | 144.5 | 184.7 | 295.5 | 1.0 |
| 32 | 38878 | [38383, 39372] | 38990 | 2.3 | 5.91× | 753.5 | 1067.2 | 2556.2 | 31.9 |
| 256 | 50052 | [48627, 51477] | 50622 | 5.1 | 7.61× | 4787.6 | 8366.5 | 9921.0 | 251.7 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 6236.609, 6296.817, 6315.668, 6234.523, 6288.328, 6349.446, 6353.437, 3969.590, 6971.689, 7245.046, 7282.696, 7278.893, 7341.732, 7248.367, 7294.752 |
| 32 | 38432.351, 39400.316, 38795.562, 40241.186, 39955.724, 38904.617, 38669.055, 39286.300, 38989.837, 39306.602, 37219.443, 39233.422, 36862.890, 38546.948, 39319.412 |
| 256 | 52934.722, 54607.538, 50673.355, 49118.599, 50799.530, 51572.927, 48746.852, 47856.690, 50893.214, 52018.596, 50622.014, 48364.278, 48942.337, 50206.731, 43420.885 |

**Peak:** 50052 mut/s at N=256 — 7.61× the sequential (N=1) baseline of 6581 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.4% | 0.7% | 59.2% | 34.8% | 3877.751 ms |
| 32 | 14.77 | 133920/0 | 25.0% | 3.9% | 44.0% | 27.0% | 3996.032 ms |
| 256 | 113.24 | 126720/0 | 38.1% | 6.7% | 35.3% | 19.9% | 2954.780 ms |
