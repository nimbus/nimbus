# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6313 | [6160, 6466] | 6256 | 4.4 | 1.00× | 153.3 | 190.8 | 246.5 | 1.0 |
| 32 | 40547 | [40097, 40998] | 40672 | 2.0 | 6.42× | 724.6 | 1033.9 | 2430.0 | 31.8 |
| 256 | 47060 | [40273, 53846] | 49587 | 26.0 | 7.45× | 4650.1 | 8507.1 | 10699.9 | 450.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7236.384, 6566.051, 6214.414, 6140.652, 6255.553, 6193.452, 6148.692, 6256.922, 6255.575, 6218.494, 6259.066, 6208.413, 6131.931, 6347.817, 6260.162 |
| 32 | 40745.355, 40672.052, 40008.761, 40681.812, 41617.737, 40046.590, 40782.091, 41340.798, 39755.329, 40282.515, 41102.922, 41810.941, 38516.564, 40614.217, 40234.627 |
| 256 | 50895.309, 47688.691, 49502.417, 47877.179, 50323.732, 49587.032, 49471.532, 53957.300, 51243.259, 48977.486, 52970.333, 3482.470, 52547.035, 51801.182, 45571.474 |

**Peak:** 47060 mut/s at N=256 — 7.45× the sequential (N=1) baseline of 6313 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.6% | 0.7% | 57.3% | 36.4% | 3903.103 ms |
| 32 | 14.54 | 133920/0 | 25.1% | 3.8% | 44.7% | 26.4% | 3861.204 ms |
| 256 | 110.48 | 126720/0 | 21.5% | 3.7% | 63.4% | 11.4% | 5276.821 ms |
