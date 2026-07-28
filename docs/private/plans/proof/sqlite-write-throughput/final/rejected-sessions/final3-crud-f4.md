# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6810 | [6274, 7347] | 7217 | 14.2 | 1.00× | 132.7 | 175.0 | 272.5 | 1.0 |
| 32 | 38183 | [36085, 40282] | 39077 | 9.9 | 5.61× | 752.9 | 1136.8 | 2536.7 | 32.3 |
| 256 | 48114 | [44336, 51891] | 49243 | 14.2 | 7.06× | 4782.2 | 8650.2 | 11101.6 | 259.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7120.158, 7011.330, 6888.530, 6946.607, 7217.027, 7220.673, 7222.486, 6889.551, 4358.469, 7310.025, 4569.859, 7296.827, 7349.886, 7443.688, 7311.500 |
| 32 | 39222.018, 38500.453, 38625.649, 39477.139, 39557.087, 38260.288, 39076.735, 38964.426, 24759.067, 40614.394, 39697.901, 39225.320, 37698.981, 40318.056, 38753.708 |
| 256 | 25697.527, 49243.312, 53471.273, 47475.544, 52225.075, 50517.830, 51032.996, 45260.113, 52387.516, 54001.721, 47102.751, 48449.604, 47485.084, 52143.454, 45213.423 |

**Peak:** 48114 mut/s at N=256 — 7.06× the sequential (N=1) baseline of 6810 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.4% | 0.6% | 61.6% | 32.4% | 3816.239 ms |
| 32 | 14.61 | 133920/0 | 24.6% | 3.6% | 43.8% | 28.0% | 4129.362 ms |
| 256 | 113.45 | 126720/0 | 35.8% | 6.3% | 38.0% | 19.9% | 3120.495 ms |
