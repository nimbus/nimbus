# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 493 | [461, 526] | 493 | 11.9 | 1.00× | 1987.5 | 2792.9 | 3241.0 | 1.0 |
| 32 | 2988 | [2974, 3001] | 2992 | 0.8 | 6.05× | 10628.1 | 11294.4 | 11711.8 | 31.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 377.151, 484.628, 536.612, 488.543, 503.834, 492.507, 435.646, 483.641, 511.496, 520.509, 417.891, 449.731, 518.125, 562.935, 617.974 |
| 32 | 3017.440, 2999.323, 2994.654, 2991.512, 3005.357, 2985.122, 3007.456, 2981.610, 3000.417, 3006.467, 2916.418, 2957.245, 2978.251, 2981.324, 2990.830 |

**Peak:** 2988 mut/s at N=32 — 6.05× the sequential (N=1) baseline of 493 mut/s.
