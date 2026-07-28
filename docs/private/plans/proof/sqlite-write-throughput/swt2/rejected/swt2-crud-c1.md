# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6245 | [5737, 6753] | 6440 | 14.7 | 1.00× | 139.3 | 214.5 | 377.2 | 1.0 |
| 32 | 36979 | [36177, 37781] | 36899 | 3.9 | 5.92× | 784.1 | 1178.0 | 2607.8 | 31.9 |
| 256 | 44182 | [42517, 45847] | 44970 | 6.8 | 7.07× | 5158.9 | 9772.0 | 17195.2 | 252.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 6841.850, 6804.348, 7079.451, 4171.940, 6412.649, 6272.225, 6439.645, 5563.398, 6670.805, 6495.141, 6304.795, 7088.414, 6915.689, 4208.705, 6406.133 |
| 32 | 36711.174, 36899.322, 37219.043, 38098.577, 33942.875, 36195.102, 38123.936, 38969.981, 39162.783, 36327.645, 36867.057, 34749.271, 37999.103, 37577.313, 35840.398 |
| 256 | 45387.327, 44799.983, 44045.604, 45169.556, 43713.359, 37509.484, 44161.369, 44969.628, 46214.463, 39625.981, 45417.920, 48051.066, 48558.678, 45057.922, 40052.143 |

**Peak:** 44182 mut/s at N=256 — 7.07× the sequential (N=1) baseline of 6245 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.2% | 0.7% | 56.0% | 38.1% | 584.020 ms |
| 32 | 14.70 | 133920/0 | 24.2% | 4.0% | 44.9% | 26.8% | 4145.997 ms |
| 256 | 115.20 | 126720/0 | 34.0% | 6.3% | 36.5% | 23.2% | 3219.557 ms |
