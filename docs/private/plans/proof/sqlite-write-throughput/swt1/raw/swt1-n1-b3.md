# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1988 | [1913, 2063] | 2013 | 6.8 | 1.00× | 485.9 | 568.0 | 722.6 | 1.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2069.902, 1992.884, 2039.173, 2037.468, 2059.518, 2074.881, 2049.051, 2000.128, 1995.888, 2025.818, 2013.339, 1970.825, 2001.518, 1979.430, 1511.569 |

**Peak:** 1988 mut/s at N=1 — 1.00× the sequential (N=1) baseline of 1988 mut/s.
