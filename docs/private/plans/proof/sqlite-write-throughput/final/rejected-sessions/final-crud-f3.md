# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7259 | [7203, 7315] | 7272 | 1.4 | 1.00× | 131.0 | 163.8 | 229.4 | 1.0 |
| 32 | 38768 | [38356, 39181] | 38904 | 1.9 | 5.34× | 759.4 | 1089.2 | 2454.8 | 31.8 |
| 256 | 52590 | [51482, 53698] | 53188 | 3.8 | 7.25× | 4487.5 | 8193.1 | 9173.4 | 251.9 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7367.741, 7309.786, 7240.532, 7045.030, 7245.830, 7263.263, 7329.960, 7272.426, 7279.477, 7386.623, 7042.970, 7190.358, 7256.711, 7317.854, 7332.476 |
| 32 | 38977.581, 38888.984, 39225.270, 38422.578, 39199.480, 39159.555, 39267.732, 38694.145, 38904.200, 37995.599, 37302.718, 39284.377, 37841.592, 38049.265, 40310.061 |
| 256 | 53552.199, 51457.169, 55031.110, 54622.912, 54860.379, 53187.744, 51517.116, 54255.604, 52274.640, 53542.837, 53218.598, 52890.686, 48088.584, 50834.352, 49512.862 |

**Peak:** 52590 mut/s at N=256 — 7.25× the sequential (N=1) baseline of 7259 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.9% | 0.7% | 58.4% | 35.1% | 488.730 ms |
| 32 | 14.56 | 133920/0 | 24.9% | 3.9% | 44.1% | 27.1% | 3993.499 ms |
| 256 | 114.16 | 126720/0 | 37.1% | 6.4% | 35.6% | 20.8% | 2803.895 ms |
