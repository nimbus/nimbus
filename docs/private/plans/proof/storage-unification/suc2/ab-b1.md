# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=300, max_mut/round=24000, measure_rounds=5, warmup_rounds=2, seed_docs=0, ladder=[1, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6748 | [6279, 7216] | 6540 | 5.6 | 1.00× | 139.4 | 186.3 | 251.2 | 1.0 |
| 256 | 46229 | [42067, 50391] | 46838 | 7.3 | 6.85× | 5088.8 | 9116.5 | 11321.2 | 252.6 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7387.426, 6538.735, 6539.543, 6792.256, 6479.975 |
| 256 | 46022.027, 46838.404, 40964.138, 50240.859, 47081.426 |

**Peak:** 46229 mut/s at N=256 — 6.85× the sequential (N=1) baseline of 6748 mut/s.
