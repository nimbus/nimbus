# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 452 | [430, 475] | 438 | 9.0 | 1.00× | 2160.0 | 2908.0 | 3444.5 | 1.0 |
| 32 | 3202 | [3143, 3261] | 3157 | 3.3 | 7.08× | 9949.1 | 10653.4 | 10983.5 | 31.9 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 409.586, 437.825, 411.391, 484.874, 504.287, 414.119, 437.648, 444.434, 479.627, 433.413, 407.707, 430.378, 518.319, 442.057, 530.446 |
| 32 | 3166.332, 3062.405, 3229.974, 3238.603, 3157.035, 3147.726, 3100.580, 3140.330, 3129.713, 3141.995, 3096.806, 3300.169, 3395.060, 3357.267, 3365.221 |

**Peak:** 3202 mut/s at N=32 — 7.08× the sequential (N=1) baseline of 452 mut/s.
