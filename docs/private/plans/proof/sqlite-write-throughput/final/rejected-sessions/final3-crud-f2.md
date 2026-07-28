# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6709 | [6409, 7009] | 6330 | 8.1 | 1.00× | 145.2 | 184.4 | 237.0 | 1.0 |
| 32 | 38157 | [35279, 41035] | 39344 | 13.6 | 5.69× | 743.0 | 1140.1 | 2524.8 | 32.9 |
| 256 | 50391 | [48899, 51883] | 50455 | 5.3 | 7.51× | 4708.9 | 8262.5 | 10051.3 | 252.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7292.748, 7254.097, 7171.823, 7268.860, 7242.805, 7289.755, 7338.955, 6330.030, 6142.079, 6293.042, 6263.367, 6145.129, 6247.215, 6211.370, 6143.161 |
| 32 | 37668.369, 19582.161, 38490.480, 40339.066, 39410.541, 39756.391, 39046.870, 40254.234, 39155.276, 39343.590, 39601.735, 40439.331, 39296.020, 40667.443, 39296.575 |
| 256 | 53100.127, 54803.215, 53301.702, 53341.921, 48210.143, 51563.851, 50098.021, 51850.077, 48203.530, 50493.650, 48168.396, 49560.487, 48127.600, 50454.760, 44583.975 |

**Peak:** 50391 mut/s at N=256 — 7.51× the sequential (N=1) baseline of 6709 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.7% | 0.7% | 57.8% | 35.9% | 3709.041 ms |
| 32 | 14.42 | 133920/0 | 23.6% | 3.6% | 46.3% | 26.5% | 4183.880 ms |
| 256 | 111.06 | 126720/0 | 37.0% | 6.9% | 34.9% | 21.2% | 2958.205 ms |
