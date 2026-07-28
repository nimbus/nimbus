# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 487 | [463, 511] | 499 | 8.9 | 1.00× | 1985.4 | 2798.1 | 3400.0 | 1.0 |
| 32 | 3148 | [3129, 3167] | 3154 | 1.1 | 6.46× | 10107.6 | 10737.6 | 11067.1 | 31.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 475.430, 488.934, 513.423, 519.394, 540.249, 518.757, 409.014, 506.461, 520.317, 521.479, 499.026, 410.265, 410.965, 498.472, 478.367 |
| 32 | 3202.845, 3153.084, 3147.283, 3080.591, 3136.913, 3140.657, 3166.611, 3172.998, 3167.598, 3156.350, 3170.163, 3166.989, 3070.416, 3137.291, 3153.571 |

**Peak:** 3148 mut/s at N=32 — 6.46× the sequential (N=1) baseline of 487 mut/s.
