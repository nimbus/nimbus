# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1990 | [1945, 2036] | 2011 | 4.1 | 1.00× | 486.1 | 564.5 | 742.4 | 1.0 |
| 32 | 16157 | [16031, 16284] | 16182 | 1.4 | 8.12× | 1913.4 | 2214.6 | 3948.5 | 31.9 |
| 256 | 27777 | [26957, 28596] | 28100 | 5.3 | 13.96× | 8620.5 | 13368.1 | 17365.4 | 252.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2043.801, 1721.829, 1923.172, 1969.657, 1981.158, 2011.346, 2009.666, 2017.014, 2024.243, 1986.555, 2031.326, 2049.351, 2045.303, 2040.331, 2000.337 |
| 32 | 16321.228, 16049.410, 16122.245, 16008.754, 15856.752, 16423.150, 16092.924, 16318.531, 16287.909, 16300.821, 16181.565, 16465.166, 16030.152, 15604.227, 16296.465 |
| 256 | 28621.611, 28258.178, 28433.790, 28542.927, 28832.408, 27767.343, 27536.710, 28099.929, 27831.236, 27915.088, 27279.644, 28526.450, 28043.976, 28304.791, 22654.199 |

**Peak:** 27777 mut/s at N=256 — 13.96× the sequential (N=1) baseline of 1990 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.4% | 0.2% | 53.7% | 44.7% | 2111.791 ms |
| 32 | 16.00 | 133920/0 | 11.3% | 1.4% | 56.4% | 31.0% | 8752.140 ms |
| 256 | 126.97 | 126720/0 | 20.9% | 3.9% | 57.4% | 17.9% | 4952.600 ms |
