# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2017 | [1992, 2042] | 2022 | 2.2 | 1.00× | 483.7 | 562.9 | 707.1 | 1.0 |
| 32 | 15453 | [13674, 17231] | 16418 | 20.8 | 7.66× | 1890.1 | 2178.7 | 3974.2 | 36.7 |
| 256 | 28090 | [27760, 28419] | 28335 | 2.1 | 13.93× | 8598.8 | 13504.4 | 15490.2 | 251.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2013.087, 2015.131, 2043.337, 2049.679, 2089.765, 2055.810, 1988.612, 2028.071, 2010.249, 1901.035, 1953.637, 2037.791, 2044.283, 2002.962, 2022.483 |
| 32 | 16686.340, 16375.842, 16447.512, 16477.418, 3906.695, 16469.895, 16298.828, 15729.062, 16417.547, 16491.803, 16121.819, 16464.570, 16049.462, 15412.473, 16443.053 |
| 256 | 27809.271, 26941.260, 28186.605, 28422.123, 28627.487, 28632.079, 28472.318, 28394.153, 28878.941, 28335.186, 27784.980, 27273.615, 27493.758, 27409.749, 28682.203 |

**Peak:** 28090 mut/s at N=256 — 13.93× the sequential (N=1) baseline of 2017 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.4% | 0.2% | 53.5% | 44.9% | 2078.020 ms |
| 32 | 16.01 | 133920/0 | 9.3% | 1.1% | 47.4% | 42.2% | 10413.530 ms |
| 256 | 124.85 | 126720/0 | 21.6% | 3.9% | 57.2% | 17.4% | 5008.349 ms |
