# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=300, max_mut/round=24000, measure_rounds=5, warmup_rounds=2, seed_docs=0, ladder=[1, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6983 | [6725, 7241] | 7094 | 3.0 | 1.00× | 134.5 | 181.5 | 271.7 | 1.0 |
| 256 | 47694 | [46607, 48782] | 47897 | 1.8 | 6.83× | 4945.9 | 8684.2 | 10925.6 | 253.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 6653.555, 6904.384, 7107.828, 7155.967, 7094.388 |
| 256 | 48502.393, 47897.016, 48519.551, 46843.792, 46709.253 |

**Peak:** 47694 mut/s at N=256 — 6.83× the sequential (N=1) baseline of 6983 mut/s.
