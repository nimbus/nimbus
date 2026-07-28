# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6347 | [6155, 6539] | 6228 | 5.5 | 1.00× | 151.8 | 191.2 | 248.0 | 1.0 |
| 32 | 36552 | [36108, 36996] | 36522 | 2.2 | 5.76× | 806.5 | 1144.5 | 2553.5 | 31.9 |
| 256 | 51001 | [49880, 52121] | 51267 | 4.0 | 8.04× | 4549.9 | 8473.9 | 10402.5 | 251.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7232.713, 7144.037, 6236.509, 6169.299, 6137.866, 6265.825, 6107.628, 6179.001, 6227.947, 6196.439, 6202.288, 6220.509, 6275.270, 6317.010, 6288.574 |
| 32 | 38635.455, 37140.073, 35710.292, 36453.854, 36561.769, 36243.891, 36892.200, 36583.985, 36522.097, 37001.534, 35973.703, 35032.011, 36085.740, 37085.627, 36355.782 |
| 256 | 52069.361, 50621.825, 50446.927, 52828.478, 51538.933, 53337.023, 53682.803, 51266.828, 47790.881, 52994.022, 50666.163, 47484.105, 51590.288, 51191.662, 47499.434 |

**Peak:** 51001 mut/s at N=256 — 8.04× the sequential (N=1) baseline of 6347 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.8% | 0.7% | 57.2% | 36.3% | 3858.509 ms |
| 32 | 14.52 | 133920/0 | 24.9% | 3.9% | 45.3% | 25.9% | 4248.427 ms |
| 256 | 110.67 | 126720/0 | 37.7% | 6.6% | 35.4% | 20.3% | 2996.173 ms |
