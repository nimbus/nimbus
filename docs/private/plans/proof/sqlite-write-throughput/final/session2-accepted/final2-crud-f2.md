# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7122 | [6662, 7581] | 7353 | 11.6 | 1.00× | 131.0 | 177.9 | 463.2 | 1.0 |
| 32 | 38396 | [35967, 40825] | 39365 | 11.4 | 5.39× | 745.7 | 1110.4 | 2462.7 | 32.5 |
| 256 | 52490 | [51565, 53415] | 52194 | 3.2 | 7.37× | 4421.9 | 8203.3 | 9752.8 | 251.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 6927.800, 7150.895, 7457.593, 7257.969, 7353.159, 7383.714, 7514.920, 7538.414, 7327.648, 7299.662, 7410.213, 7457.794, 7403.515, 4179.544, 7159.954 |
| 32 | 41375.531, 39804.019, 40673.997, 40060.036, 38867.638, 39360.827, 37492.061, 38165.916, 39208.417, 38475.883, 39495.548, 22975.205, 39365.115, 39826.406, 40798.918 |
| 256 | 54588.851, 52843.569, 52133.465, 48095.302, 54227.032, 51457.508, 53275.764, 52100.242, 54942.440, 51100.122, 52184.858, 53780.214, 51860.315, 52194.167, 52567.090 |

**Peak:** 52490 mut/s at N=256 — 7.37× the sequential (N=1) baseline of 7122 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 6.0% | 0.7% | 57.8% | 35.5% | 499.929 ms |
| 32 | 14.50 | 133920/0 | 24.2% | 3.8% | 46.3% | 25.7% | 4122.137 ms |
| 256 | 111.06 | 126720/0 | 37.4% | 6.7% | 35.5% | 20.5% | 2866.907 ms |
