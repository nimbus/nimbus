# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 540 | [523, 557] | 535 | 5.6 | 1.00× | 1873.3 | 2079.5 | 2224.4 | 1.0 |
| 32 | 2984 | [2907, 3062] | 3018 | 4.7 | 5.53× | 10513.5 | 11163.4 | 11567.3 | 31.9 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 516.444, 525.412, 540.413, 495.482, 510.957, 553.870, 534.661, 545.321, 539.577, 533.944, 517.725, 586.903, 611.724, 564.631, 520.872 |
| 32 | 3027.787, 3031.876, 3009.350, 2483.713, 3046.423, 3030.092, 3041.368, 3053.369, 3018.368, 2988.748, 2968.158, 3023.314, 3016.505, 3016.394, 3006.082 |

**Peak:** 2984 mut/s at N=32 — 5.53× the sequential (N=1) baseline of 540 mut/s.
