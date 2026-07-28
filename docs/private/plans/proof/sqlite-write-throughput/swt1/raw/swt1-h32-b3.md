# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 527 | [523, 532] | 530 | 1.6 | 1.00× | 1878.3 | 2075.3 | 2300.1 | 1.0 |
| 32 | 3158 | [3144, 3172] | 3169 | 0.8 | 5.99× | 10051.3 | 10698.6 | 10998.3 | 31.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 517.684, 524.999, 530.176, 522.355, 540.606, 530.840, 524.567, 531.830, 538.623, 534.692, 513.172, 530.341, 533.626, 512.655, 524.733 |
| 32 | 3182.964, 3166.014, 3174.377, 3172.841, 3173.239, 3170.662, 3144.513, 3170.725, 3173.283, 3152.286, 3111.726, 3091.694, 3159.159, 3168.743, 3159.839 |

**Peak:** 3158 mut/s at N=32 — 5.99× the sequential (N=1) baseline of 527 mut/s.
