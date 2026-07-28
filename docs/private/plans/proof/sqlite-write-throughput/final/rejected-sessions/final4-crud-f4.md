# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6537 | [6083, 6990] | 6316 | 12.5 | 1.00× | 146.0 | 184.7 | 258.8 | 1.0 |
| 32 | 38333 | [37867, 38799] | 38481 | 2.2 | 5.86× | 763.2 | 1088.9 | 2627.4 | 31.8 |
| 256 | 50893 | [49883, 51903] | 50327 | 3.6 | 7.79× | 4644.8 | 8336.2 | 9882.1 | 251.7 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7222.207, 7292.129, 7221.148, 6294.698, 6270.751, 6251.622, 6198.771, 6260.538, 6259.033, 6316.260, 6374.615, 4184.528, 7147.299, 7342.674, 7414.298 |
| 32 | 39712.778, 38480.768, 37723.313, 38129.262, 39514.267, 38608.444, 39313.720, 38924.617, 37914.108, 38671.526, 38547.253, 37817.909, 36797.502, 37706.989, 37130.471 |
| 256 | 49011.548, 51317.304, 49497.680, 52527.160, 49526.323, 49925.771, 54723.370, 49641.884, 51538.252, 53129.416, 51943.301, 52548.057, 49719.599, 48019.713, 50327.379 |

**Peak:** 50893 mut/s at N=256 — 7.79× the sequential (N=1) baseline of 6537 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.3% | 0.6% | 59.4% | 34.7% | 3887.231 ms |
| 32 | 14.59 | 133920/0 | 25.0% | 3.9% | 44.4% | 26.8% | 4054.008 ms |
| 256 | 112.64 | 126720/0 | 38.2% | 6.4% | 35.3% | 20.0% | 2950.383 ms |
