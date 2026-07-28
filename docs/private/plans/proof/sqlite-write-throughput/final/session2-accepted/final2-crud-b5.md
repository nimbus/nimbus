# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1979 | [1914, 2045] | 2024 | 6.0 | 1.00× | 487.6 | 572.6 | 836.6 | 1.0 |
| 32 | 15899 | [15791, 16008] | 15963 | 1.2 | 8.03× | 1944.6 | 2241.4 | 4008.2 | 31.9 |
| 256 | 27706 | [27479, 27934] | 27762 | 1.5 | 14.00× | 8786.2 | 13383.4 | 15151.8 | 250.9 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2029.335, 1616.261, 2071.316, 2084.716, 2023.790, 2059.129, 2065.938, 2036.220, 1995.348, 2018.989, 2025.007, 1879.399, 1941.295, 1902.314, 1940.700 |
| 32 | 16162.161, 15981.558, 16043.641, 15744.517, 15950.755, 15475.021, 16068.522, 15834.644, 16008.127, 15963.075, 16011.704, 15937.509, 15967.985, 15842.670, 15498.222 |
| 256 | 27260.366, 28153.431, 27411.035, 27479.862, 28047.735, 27816.650, 27598.557, 28536.098, 27510.713, 27582.687, 27781.531, 27978.547, 26794.366, 27881.437, 27762.297 |

**Peak:** 27706 mut/s at N=256 — 14.00× the sequential (N=1) baseline of 1979 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.5% | 0.2% | 53.4% | 45.0% | 2122.363 ms |
| 32 | 16.01 | 133920/0 | 11.3% | 1.3% | 56.3% | 31.1% | 8897.821 ms |
| 256 | 124.60 | 126720/0 | 21.6% | 3.9% | 56.7% | 17.8% | 5036.473 ms |
