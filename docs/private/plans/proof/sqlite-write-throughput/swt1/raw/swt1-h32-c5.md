# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 512 | [493, 530] | 514 | 6.5 | 1.00× | 1916.8 | 2307.0 | 3025.5 | 1.0 |
| 32 | 2993 | [2980, 3005] | 2986 | 0.8 | 5.85× | 10616.2 | 11277.9 | 11655.5 | 31.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 513.166, 510.119, 528.426, 495.358, 445.724, 435.343, 511.128, 507.246, 514.362, 534.397, 533.514, 530.132, 565.334, 529.520, 524.142 |
| 32 | 3027.778, 3023.938, 3034.615, 2994.582, 2996.425, 2981.310, 3003.060, 2989.734, 2985.909, 2982.624, 2986.476, 2975.257, 2985.009, 2949.315, 2971.915 |

**Peak:** 2993 mut/s at N=32 — 5.85× the sequential (N=1) baseline of 512 mut/s.
