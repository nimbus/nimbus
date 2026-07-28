# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 542 | [525, 558] | 534 | 5.6 | 1.00× | 1870.9 | 2051.1 | 2212.9 | 1.0 |
| 32 | 3012 | [3005, 3018] | 3011 | 0.4 | 5.56× | 10559.1 | 11193.6 | 11504.4 | 31.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 586.479, 612.082, 571.783, 557.153, 560.812, 503.319, 537.029, 519.995, 529.465, 512.526, 512.211, 523.916, 524.391, 534.382, 537.293 |
| 32 | 3039.933, 3025.190, 3011.231, 3007.198, 3018.409, 2990.885, 3004.072, 3017.157, 3006.873, 3012.525, 3011.107, 3007.595, 3005.858, 3014.743, 3002.226 |

**Peak:** 3012 mut/s at N=32 — 5.56× the sequential (N=1) baseline of 542 mut/s.
