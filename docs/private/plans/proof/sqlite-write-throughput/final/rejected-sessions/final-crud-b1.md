# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 2007 | [1992, 2022] | 2004 | 1.4 | 1.00× | 487.5 | 563.5 | 690.6 | 1.0 |
| 32 | 16250 | [16029, 16471] | 16412 | 2.5 | 8.10× | 1890.8 | 2224.0 | 4034.5 | 31.9 |
| 256 | 28480 | [28208, 28752] | 28566 | 1.7 | 14.19× | 8527.3 | 12950.1 | 16049.6 | 251.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1971.742, 2050.367, 1998.626, 1990.259, 1981.953, 2003.938, 1983.568, 2018.841, 2028.446, 2038.132, 2039.959, 2013.492, 1955.296, 1997.582, 2033.856 |
| 32 | 16613.515, 16520.173, 16249.961, 15272.889, 16449.274, 16361.496, 16592.589, 16468.656, 16388.510, 16413.273, 16412.425, 16227.679, 15841.694, 16440.492, 15492.114 |
| 256 | 28102.020, 28241.980, 28598.470, 27687.286, 28664.511, 29485.059, 28168.196, 28896.730, 29025.666, 28291.836, 27587.517, 28547.428, 28613.597, 28566.123, 28722.486 |

**Peak:** 28480 mut/s at N=256 — 14.19× the sequential (N=1) baseline of 2007 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.4% | 0.2% | 53.7% | 44.7% | 2088.348 ms |
| 32 | 16.01 | 133920/0 | 11.3% | 1.4% | 56.1% | 31.3% | 8705.257 ms |
| 256 | 125.96 | 126720/0 | 21.0% | 4.0% | 56.9% | 18.1% | 4854.514 ms |
