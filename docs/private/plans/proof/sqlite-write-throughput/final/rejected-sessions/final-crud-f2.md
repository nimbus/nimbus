# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7315 | [7214, 7415] | 7253 | 2.5 | 1.00× | 130.0 | 163.3 | 223.2 | 1.0 |
| 32 | 39164 | [38578, 39751] | 39135 | 2.7 | 5.35× | 754.4 | 1084.1 | 2417.8 | 31.9 |
| 256 | 52449 | [50824, 54075] | 53258 | 5.6 | 7.17× | 4431.2 | 8329.7 | 9817.8 | 251.6 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7221.484, 7290.726, 7311.478, 7119.035, 7253.217, 7224.730, 7153.687, 7035.379, 7183.922, 7453.679, 7242.717, 7459.377, 7649.329, 7581.485, 7539.070 |
| 32 | 41170.288, 40816.093, 39172.463, 39307.943, 37800.410, 38691.937, 37129.312, 39532.741, 38834.093, 40291.263, 39134.794, 39463.579, 38870.331, 38158.895, 39089.053 |
| 256 | 51214.420, 54174.075, 53327.105, 54287.898, 53258.117, 55827.201, 50665.213, 50243.832, 52745.688, 55701.252, 53674.063, 51212.351, 44101.074, 51222.611, 55083.333 |

**Peak:** 52449 mut/s at N=256 — 7.17× the sequential (N=1) baseline of 7315 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.9% | 0.7% | 58.2% | 35.2% | 484.815 ms |
| 32 | 14.66 | 133920/0 | 25.2% | 3.9% | 44.8% | 26.1% | 3978.650 ms |
| 256 | 112.14 | 126720/0 | 36.8% | 6.7% | 36.2% | 20.3% | 2847.056 ms |
