# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2011 | [1994, 2029] | 2003 | 1.5 | 1.00× | 487.3 | 555.8 | 671.0 | 1.0 |
| 32 | 16450 | [16274, 16626] | 16502 | 1.9 | 8.18× | 1863.9 | 2133.8 | 4157.2 | 31.9 |
| 256 | 28692 | [28058, 29327] | 28817 | 4.0 | 14.26× | 8316.6 | 13123.6 | 15537.8 | 251.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2039.427, 2055.963, 2046.022, 2043.579, 2061.444, 2012.724, 1967.348, 1997.299, 1967.640, 2002.921, 1985.747, 1996.637, 1990.171, 1989.026, 2014.986 |
| 32 | 16907.496, 16754.435, 16501.784, 15839.456, 16039.787, 16464.090, 16012.640, 16523.917, 16225.790, 16809.435, 16464.210, 16605.479, 16688.664, 16221.177, 16694.059 |
| 256 | 28678.860, 28817.425, 28155.049, 26181.105, 26375.126, 28343.901, 29078.155, 30027.040, 29788.640, 29192.738, 28556.041, 30289.255, 29012.035, 28732.251, 29157.041 |

**Peak:** 28692 mut/s at N=256 — 14.26× the sequential (N=1) baseline of 2011 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.6% | 0.2% | 53.4% | 44.8% | 2079.795 ms |
| 32 | 16.01 | 133920/0 | 11.2% | 1.3% | 56.0% | 31.4% | 8609.612 ms |
| 256 | 125.59 | 126720/0 | 21.0% | 3.7% | 56.8% | 18.5% | 4836.599 ms |
