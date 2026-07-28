# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1598 | [1378, 1817] | 1769 | 24.8 | 1.00× | 533.1 | 907.7 | 2571.5 | 1.1 |
| 32 | 15155 | [14335, 15976] | 15592 | 9.8 | 9.49× | 1935.6 | 2927.2 | 4394.1 | 32.3 |
| 256 | 23214 | [20053, 26376] | 24028 | 24.6 | 14.53× | 9114.2 | 17681.5 | 44488.4 | 290.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1791.733, 1862.565, 1423.363, 1218.969, 1311.654, 1378.624, 1159.384, 614.922, 1733.079, 1948.299, 1964.986, 1918.390, 1769.035, 1943.612, 1924.486 |
| 32 | 15923.826, 14434.430, 15491.722, 12625.539, 15592.075, 15316.675, 15105.943, 15759.489, 11036.716, 15193.344, 15862.329, 16408.836, 16338.239, 16449.220, 15791.245 |
| 256 | 28153.427, 27897.723, 27442.137, 26568.471, 28361.247, 27469.761, 6256.424, 20663.877, 21813.910, 25766.244, 24027.702, 21413.639, 19307.951, 19443.163, 23631.053 |

**Peak:** 23214 mut/s at N=256 — 14.53× the sequential (N=1) baseline of 1598 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.2% | 0.2% | 50.8% | 47.8% | 2883.774 ms |
| 32 | 16.02 | 133920/0 | 10.6% | 1.4% | 57.7% | 30.3% | 9390.165 ms |
| 256 | 130.50 | 126720/0 | 15.8% | 2.9% | 64.4% | 17.0% | 6597.470 ms |
