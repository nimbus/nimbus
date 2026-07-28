# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1952 | [1940, 1964] | 1954 | 1.1 | 1.00× | 499.5 | 577.6 | 745.5 | 1.0 |
| 32 | 15887 | [15772, 16002] | 15872 | 1.3 | 8.14× | 1946.0 | 2229.0 | 4042.0 | 31.9 |
| 256 | 27575 | [27286, 27865] | 27850 | 1.9 | 14.13× | 8802.5 | 13386.5 | 16224.4 | 250.7 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1965.884, 1973.945, 1903.265, 1968.598, 1929.163, 1945.855, 1973.464, 1946.254, 1953.175, 1954.407, 1923.427, 1975.463, 1973.392, 1939.029, 1955.482 |
| 32 | 15654.255, 15594.369, 15843.601, 16099.781, 15872.453, 16114.864, 16086.107, 16123.305, 16165.178, 15871.586, 15974.822, 15765.788, 15618.368, 15939.104, 15578.867 |
| 256 | 27337.454, 26900.161, 27931.108, 28028.259, 27170.055, 27850.266, 27312.158, 28193.887, 27883.282, 27350.612, 28072.713, 26443.649, 28094.034, 27945.537, 27115.877 |

**Peak:** 27575 mut/s at N=256 — 14.13× the sequential (N=1) baseline of 1952 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.6% | 0.2% | 53.5% | 44.8% | 14994.050 ms |
| 32 | 16.00 | 133920/0 | 11.3% | 1.4% | 56.4% | 30.9% | 8910.274 ms |
| 256 | 124.24 | 126720/0 | 21.5% | 3.8% | 56.9% | 17.8% | 5063.949 ms |
