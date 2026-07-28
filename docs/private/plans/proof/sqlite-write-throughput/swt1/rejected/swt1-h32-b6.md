# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 504 | [490, 519] | 502 | 5.3 | 1.00× | 1957.4 | 2235.5 | 2932.8 | 1.0 |
| 32 | 3155 | [3138, 3172] | 3151 | 1.0 | 6.25× | 10086.5 | 10703.2 | 10997.9 | 31.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 517.578, 442.373, 497.959, 502.314, 536.921, 526.097, 465.986, 528.864, 539.276, 515.167, 491.386, 481.827, 517.708, 501.965, 501.227 |
| 32 | 3156.993, 3160.528, 3139.760, 3128.965, 3129.813, 3150.998, 3154.588, 3149.335, 3144.022, 3143.730, 3084.539, 3200.977, 3199.576, 3178.209, 3202.379 |

**Peak:** 3155 mut/s at N=32 — 6.25× the sequential (N=1) baseline of 504 mut/s.
