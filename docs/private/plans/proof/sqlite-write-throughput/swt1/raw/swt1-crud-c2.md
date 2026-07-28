# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1933 | [1920, 1947] | 1940 | 1.3 | 1.00× | 508.7 | 566.1 | 670.8 | 1.0 |
| 32 | 20217 | [20001, 20433] | 20205 | 1.9 | 10.46× | 1518.3 | 1723.5 | 3493.2 | 31.9 |
| 256 | 45717 | [45207, 46226] | 45842 | 2.0 | 23.65× | 5132.1 | 8631.7 | 9889.7 | 249.7 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1940.778, 1952.514, 1952.420, 1943.637, 1952.856, 1969.418, 1969.557, 1940.084, 1903.674, 1929.888, 1904.648, 1902.923, 1917.312, 1891.311, 1928.179 |
| 32 | 19939.682, 20051.735, 20204.508, 20177.668, 20295.668, 19514.251, 20590.180, 20813.770, 20566.015, 20665.578, 20472.245, 19436.671, 20312.530, 20141.083, 20076.432 |
| 256 | 46385.618, 45842.442, 45447.974, 46039.004, 43902.620, 46012.477, 46260.521, 47269.637, 45145.116, 45227.664, 45987.002, 44460.642, 45090.058, 47270.254, 45409.854 |

**Peak:** 45717 mut/s at N=256 — 23.65× the sequential (N=1) baseline of 1933 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.5% | 0.2% | 54.3% | 44.0% | 2171.939 ms |
| 32 | 16.01 | 133920/0 | 13.6% | 1.5% | 47.5% | 37.4% | 7105.683 ms |
| 256 | 120.23 | 126720/0 | 31.5% | 5.7% | 38.5% | 24.4% | 3211.944 ms |
