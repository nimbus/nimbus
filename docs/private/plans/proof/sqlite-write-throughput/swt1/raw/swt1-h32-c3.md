# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 517 | [508, 527] | 517 | 3.3 | 1.00× | 1926.1 | 2141.8 | 2457.4 | 1.0 |
| 32 | 2993 | [2981, 3005] | 2997 | 0.7 | 5.79× | 10609.7 | 11241.5 | 11563.3 | 31.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 534.660, 544.727, 485.429, 525.313, 527.117, 514.462, 510.776, 497.606, 509.703, 523.243, 487.050, 529.816, 533.795, 517.276, 515.364 |
| 32 | 2932.544, 3006.425, 3024.897, 3009.203, 2976.675, 3002.837, 3002.141, 3005.062, 2997.190, 2996.009, 3001.970, 2996.669, 2995.448, 2967.627, 2981.102 |

**Peak:** 2993 mut/s at N=32 — 5.79× the sequential (N=1) baseline of 517 mut/s.
