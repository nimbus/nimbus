# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2030 | [2020, 2040] | 2025 | 0.9 | 1.00× | 484.3 | 542.2 | 643.6 | 1.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2008.027, 2036.353, 2024.958, 2015.439, 2052.515, 2029.233, 2051.843, 2017.578, 2005.689, 2022.629, 2033.937, 2072.383, 2023.450, 2025.582, 2025.383 |

**Peak:** 2030 mut/s at N=1 — 1.00× the sequential (N=1) baseline of 2030 mut/s.
