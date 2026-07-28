# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7193 | [7136, 7250] | 7207 | 1.4 | 1.00× | 131.9 | 167.0 | 245.2 | 1.0 |
| 32 | 38664 | [38250, 39078] | 38768 | 1.9 | 5.38× | 762.1 | 1087.2 | 2628.8 | 31.9 |
| 256 | 49334 | [48417, 50251] | 49455 | 3.4 | 6.86× | 4848.9 | 8550.0 | 10088.3 | 251.2 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7207.070, 7309.244, 6936.623, 7031.250, 7249.208, 7267.691, 7130.026, 7150.518, 7187.939, 7164.144, 7169.087, 7273.014, 7293.030, 7253.341, 7275.336 |
| 32 | 39873.364, 38346.748, 39792.510, 38197.907, 38321.263, 38767.983, 38603.165, 39385.535, 37921.167, 39214.804, 37729.278, 39052.193, 37178.203, 38769.211, 38803.705 |
| 256 | 50350.626, 49360.174, 48786.181, 49730.867, 51985.106, 46838.258, 47751.025, 49454.880, 49827.871, 49619.093, 52889.899, 47491.768, 48523.674, 47419.848, 49977.138 |

**Peak:** 49334 mut/s at N=256 — 6.86× the sequential (N=1) baseline of 7193 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.9% | 0.8% | 58.4% | 34.9% | 491.710 ms |
| 32 | 14.65 | 133920/0 | 25.3% | 3.8% | 44.4% | 26.5% | 4035.078 ms |
| 256 | 113.65 | 126720/0 | 37.5% | 6.6% | 35.8% | 20.1% | 2986.113 ms |
