# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1944 | [1933, 1955] | 1949 | 1.0 | 1.00× | 502.8 | 579.6 | 714.7 | 1.0 |
| 32 | 16383 | [16280, 16487] | 16384 | 1.1 | 8.43× | 1886.0 | 2148.8 | 3919.8 | 31.9 |
| 256 | 28531 | [28145, 28917] | 28692 | 2.4 | 14.68× | 8486.4 | 12984.4 | 16485.5 | 251.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1949.012, 1938.322, 1953.615, 1932.332, 1947.044, 1979.195, 1944.141, 1957.527, 1907.748, 1951.513, 1950.509, 1899.699, 1942.746, 1951.270, 1956.294 |
| 32 | 16529.899, 16554.150, 16082.869, 16339.172, 16327.859, 16577.498, 16568.671, 16384.223, 16008.476, 16150.744, 16519.111, 16453.654, 16312.924, 16596.247, 16346.346 |
| 256 | 28320.178, 27294.330, 29781.893, 28899.782, 27644.763, 28546.801, 28847.168, 28989.527, 28691.616, 29364.337, 28113.312, 28757.362, 28871.502, 28481.417, 27357.985 |

**Peak:** 28531 mut/s at N=256 — 14.68× the sequential (N=1) baseline of 1944 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.6% | 0.2% | 53.5% | 44.7% | 15046.572 ms |
| 32 | 16.01 | 133920/0 | 11.3% | 1.4% | 56.2% | 31.2% | 8657.197 ms |
| 256 | 126.72 | 126720/0 | 21.0% | 3.6% | 57.2% | 18.2% | 4817.168 ms |
