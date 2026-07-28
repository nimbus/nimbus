# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2068 | [2053, 2084] | 2074 | 1.3 | 1.00× | 475.9 | 524.3 | 651.0 | 1.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2075.140, 2071.013, 2051.126, 2078.550, 2073.530, 2080.482, 2102.740, 2089.648, 2047.409, 2047.807, 2057.404, 2069.059, 2073.760, 2112.945, 1994.511 |

**Peak:** 2068 mut/s at N=1 — 1.00× the sequential (N=1) baseline of 2068 mut/s.
