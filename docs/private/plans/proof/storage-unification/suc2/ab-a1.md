# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=300, max_mut/round=24000, measure_rounds=5, warmup_rounds=2, seed_docs=0, ladder=[1, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 5796 | [3374, 8219] | 6358 | 33.7 | 1.00× | 137.0 | 196.0 | 417.3 | 1.2 |
| 256 | 47078 | [43100, 51056] | 48000 | 6.8 | 8.12× | 4842.3 | 9261.2 | 11972.7 | 253.9 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7187.621, 7022.602, 2414.802, 5997.073, 6358.426 |
| 256 | 50977.340, 48299.486, 45679.563, 42433.879, 48000.169 |

**Peak:** 47078 mut/s at N=256 — 8.12× the sequential (N=1) baseline of 5796 mut/s.
