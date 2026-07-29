# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=300, max_mut/round=24000, measure_rounds=5, warmup_rounds=2, seed_docs=0, ladder=[1, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7225 | [6996, 7455] | 7162 | 2.6 | 1.00× | 131.8 | 164.3 | 228.8 | 1.0 |
| 256 | 47254 | [37868, 56639] | 50942 | 16.0 | 6.54× | 4714.6 | 8314.8 | 9806.1 | 260.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7401.148, 7439.951, 7094.232, 7029.365, 7161.768 |
| 256 | 50942.017, 48612.425, 51347.297, 51476.441, 33890.502 |

**Peak:** 47254 mut/s at N=256 — 6.54× the sequential (N=1) baseline of 7225 mut/s.
