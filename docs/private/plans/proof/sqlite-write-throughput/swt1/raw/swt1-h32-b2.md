# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 535 | [529, 540] | 535 | 1.8 | 1.00× | 1865.0 | 2048.0 | 2230.8 | 1.0 |
| 32 | 3175 | [3140, 3210] | 3168 | 2.0 | 5.94× | 10021.0 | 10685.1 | 11057.3 | 31.9 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 525.412, 537.805, 545.716, 536.460, 523.895, 534.956, 529.421, 546.483, 538.720, 531.571, 532.459, 514.660, 527.962, 547.599, 547.654 |
| 32 | 3380.192, 3173.424, 3065.191, 3169.880, 3162.675, 3177.267, 3162.019, 3167.900, 3187.634, 3182.875, 3177.070, 3155.264, 3157.189, 3152.498, 3156.945 |

**Peak:** 3175 mut/s at N=32 — 5.94× the sequential (N=1) baseline of 535 mut/s.
