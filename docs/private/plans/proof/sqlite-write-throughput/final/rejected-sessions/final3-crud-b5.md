# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1925 | [1905, 1946] | 1938 | 1.9 | 1.00× | 504.2 | 586.9 | 765.0 | 1.0 |
| 32 | 15961 | [15801, 16121] | 16102 | 1.8 | 8.29× | 1930.1 | 2200.4 | 3970.1 | 31.9 |
| 256 | 27852 | [27521, 28182] | 27782 | 2.1 | 14.47× | 8717.1 | 13362.5 | 15860.4 | 251.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1935.555, 1948.900, 1938.687, 1948.431, 1940.534, 1937.795, 1946.579, 1845.056, 1930.081, 1923.372, 1945.858, 1952.448, 1934.429, 1829.839, 1918.639 |
| 32 | 15664.807, 15626.534, 15730.707, 15232.720, 16134.156, 15978.898, 16114.377, 16113.216, 16159.542, 16259.718, 16180.649, 16288.716, 15958.264, 16101.994, 15873.338 |
| 256 | 27646.124, 27659.660, 28191.872, 27781.500, 26761.885, 28038.511, 27492.986, 29296.789, 28439.170, 27701.104, 28008.501, 28226.744, 28038.577, 27409.290, 27081.639 |

**Peak:** 27852 mut/s at N=256 — 14.47× the sequential (N=1) baseline of 1925 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.6% | 0.2% | 53.7% | 44.5% | 15210.438 ms |
| 32 | 16.01 | 133920/0 | 11.0% | 1.3% | 56.8% | 30.9% | 8840.526 ms |
| 256 | 123.87 | 126720/0 | 21.6% | 3.9% | 56.8% | 17.7% | 5035.697 ms |
