# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 632 | [624, 639] | 634 | 2.2 | 1.00× | 1580.6 | 1764.7 | 2155.4 | 1.0 |
| 32 | 6065 | [5913, 6217] | 6022 | 4.5 | 9.60× | 5264.0 | 5998.2 | 6314.0 | 31.9 |
| 256 | 4949 | [4492, 5406] | 5150 | 16.7 | 7.83× | 37930.7 | 103311.9 | 158835.9 | 254.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 624.486, 634.393, 635.088, 633.847, 622.093, 646.942, 610.812, 646.594, 626.615, 653.821, 637.441, 622.194, 616.265, 612.151, 652.129 |
| 32 | 6025.842, 6135.053, 6085.330, 6120.447, 5823.145, 5884.504, 6105.162, 5917.704, 5835.969, 6994.790, 6022.488, 6051.633, 6000.844, 6002.836, 5970.480 |
| 256 | 5149.793, 4968.441, 4550.087, 5052.408, 5325.045, 5130.669, 5083.386, 2061.507, 5144.280, 5418.243, 5282.574, 5195.496, 5275.681, 5351.041, 5242.110 |

**Peak:** 6065 mut/s at N=32 — 9.60× the sequential (N=1) baseline of 632 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 12.4% | 0.4% | 2.0% | 85.2% | 369.850 ms |
| 32 | 0.00 | 48000/0 | 12.1% | 12.0% | 1.5% | 74.4% | 6557.184 ms |
| 256 | 0.00 | 134801/0 | 60.6% | 5.3% | 0.9% | 33.2% | 55146.851 ms |
