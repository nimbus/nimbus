# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 491 | [469, 512] | 495 | 8.0 | 1.00× | 1979.0 | 2702.2 | 3312.0 | 1.0 |
| 32 | 3178 | [3168, 3188] | 3184 | 0.6 | 6.48× | 10012.7 | 10651.6 | 10922.0 | 31.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 459.041, 486.893, 517.751, 514.786, 407.404, 495.474, 530.952, 479.200, 493.001, 451.771, 528.437, 524.284, 522.934, 521.577, 425.580 |
| 32 | 3194.119, 3190.116, 3192.581, 3194.448, 3153.626, 3202.115, 3194.900, 3185.308, 3138.770, 3183.565, 3171.025, 3169.428, 3164.133, 3162.133, 3172.771 |

**Peak:** 3178 mut/s at N=32 — 6.48× the sequential (N=1) baseline of 491 mut/s.
