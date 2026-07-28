# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1939 | [1915, 1963] | 1946 | 2.2 | 1.00× | 502.7 | 581.9 | 721.7 | 1.0 |
| 32 | 15318 | [13427, 17209] | 16218 | 22.3 | 7.90× | 1904.8 | 2234.1 | 4085.6 | 39.1 |
| 256 | 28288 | [27948, 28629] | 28333 | 2.2 | 14.59× | 8570.3 | 13169.3 | 16192.3 | 251.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1932.431, 1933.693, 1798.310, 2006.951, 1926.228, 1956.553, 1933.015, 1952.386, 1958.021, 1931.013, 1955.266, 1946.572, 1956.389, 1946.142, 1945.309 |
| 32 | 16423.925, 16221.378, 15809.874, 16218.347, 16423.496, 16409.755, 16264.398, 16162.395, 15965.111, 16223.675, 16105.461, 2992.448, 16018.436, 16318.595, 16217.251 |
| 256 | 28283.970, 28452.484, 28896.792, 28332.700, 29100.755, 27832.393, 28490.686, 28016.424, 28924.342, 27367.923, 26784.922, 28191.680, 28765.286, 28676.037, 28207.793 |

**Peak:** 28288 mut/s at N=256 — 14.59× the sequential (N=1) baseline of 1939 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.6% | 0.2% | 53.6% | 44.6% | 15097.914 ms |
| 32 | 16.01 | 133920/0 | 8.9% | 1.1% | 65.5% | 24.5% | 11204.323 ms |
| 256 | 125.71 | 126720/0 | 21.0% | 3.8% | 57.3% | 17.9% | 4901.125 ms |
