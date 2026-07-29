# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=300, max_mut/round=24000, measure_rounds=5, warmup_rounds=2, seed_docs=0, ladder=[1, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7225 | [7094, 7356] | 7258 | 1.5 | 1.00× | 131.8 | 163.2 | 229.6 | 1.0 |
| 256 | 48369 | [44706, 52031] | 46831 | 6.1 | 6.69× | 4852.8 | 8752.8 | 10939.3 | 254.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7067.633, 7257.957, 7182.408, 7272.857, 7345.697 |
| 256 | 45695.692, 52485.861, 50458.558, 46371.535, 46831.068 |

**Peak:** 48369 mut/s at N=256 — 6.69× the sequential (N=1) baseline of 7225 mut/s.
