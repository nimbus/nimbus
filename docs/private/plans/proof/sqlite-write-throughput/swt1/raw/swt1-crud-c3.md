# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1931 | [1917, 1946] | 1930 | 1.4 | 1.00× | 509.5 | 567.4 | 681.2 | 1.0 |
| 32 | 20135 | [19993, 20278] | 20153 | 1.3 | 10.43× | 1530.4 | 1746.6 | 3455.3 | 31.9 |
| 256 | 45020 | [44088, 45952] | 44985 | 3.7 | 23.31× | 5265.7 | 8957.4 | 10143.2 | 249.9 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1960.111, 1979.771, 1972.473, 1884.912, 1942.209, 1895.890, 1917.041, 1929.567, 1936.896, 1930.423, 1948.638, 1918.595, 1909.697, 1929.605, 1913.033 |
| 32 | 20336.180, 20008.873, 20094.444, 20152.578, 19993.386, 20236.281, 20522.685, 19603.073, 20329.298, 20338.865, 19966.199, 19683.968, 20337.273, 20353.660, 20072.877 |
| 256 | 46541.213, 44414.837, 44984.924, 45502.307, 43079.466, 43352.386, 46524.125, 43570.713, 45936.324, 48367.070, 45170.744, 43984.747, 47294.645, 42418.689, 44161.513 |

**Peak:** 45020 mut/s at N=256 — 23.31× the sequential (N=1) baseline of 1931 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.5% | 0.2% | 54.3% | 44.0% | 2175.477 ms |
| 32 | 16.00 | 133920/0 | 13.8% | 1.6% | 47.3% | 37.3% | 7141.058 ms |
| 256 | 119.43 | 126720/0 | 31.6% | 5.6% | 37.6% | 25.1% | 3261.514 ms |
