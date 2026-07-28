# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2018 | [1999, 2037] | 2022 | 1.7 | 1.00× | 485.2 | 552.0 | 690.9 | 1.0 |
| 32 | 16135 | [15997, 16273] | 16202 | 1.5 | 8.00× | 1903.0 | 2356.4 | 4059.9 | 31.9 |
| 256 | 27165 | [26523, 27807] | 27320 | 4.3 | 13.46× | 8825.7 | 14358.3 | 18975.3 | 251.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2021.634, 2068.426, 2015.924, 2022.676, 2042.785, 1987.898, 1934.104, 2059.173, 2055.916, 1995.819, 2006.657, 2005.792, 1993.138, 2021.633, 2036.511 |
| 32 | 16041.494, 16095.530, 16253.194, 16169.547, 15520.002, 16263.092, 16227.541, 16156.941, 16201.695, 16437.958, 16355.767, 16433.248, 15861.233, 15785.701, 16226.838 |
| 256 | 27763.499, 27405.881, 28176.221, 28236.513, 27300.008, 28562.014, 27293.716, 26557.248, 27769.480, 27319.854, 27974.197, 25646.997, 23980.499, 26592.833, 26897.163 |

**Peak:** 27165 mut/s at N=256 — 13.46× the sequential (N=1) baseline of 2018 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.7% | 0.2% | 53.2% | 44.9% | 2068.574 ms |
| 32 | 16.01 | 133920/0 | 11.3% | 1.4% | 55.9% | 31.4% | 8779.534 ms |
| 256 | 129.70 | 126720/0 | 20.7% | 3.6% | 57.4% | 18.3% | 5005.065 ms |
