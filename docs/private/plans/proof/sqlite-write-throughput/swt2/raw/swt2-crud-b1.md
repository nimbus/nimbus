# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1813 | [1773, 1852] | 1834 | 3.9 | 1.00× | 537.0 | 666.8 | 784.5 | 1.0 |
| 32 | 18610 | [18181, 19040] | 18704 | 4.2 | 10.27× | 1620.5 | 1948.9 | 4064.2 | 32.0 |
| 256 | 39859 | [38741, 40977] | 40224 | 5.1 | 21.99× | 5636.6 | 11420.0 | 17776.5 | 250.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1883.129, 1855.269, 1875.270, 1852.136, 1898.188, 1887.034, 1852.943, 1782.138, 1810.750, 1834.323, 1705.678, 1791.534, 1764.836, 1734.152, 1663.024 |
| 32 | 17236.116, 17386.982, 17580.898, 18444.776, 19439.223, 19355.171, 18704.969, 18930.186, 19223.837, 18427.023, 18704.376, 17972.621, 19532.657, 18650.124, 19567.747 |
| 256 | 40087.462, 39792.012, 42622.439, 40370.501, 42230.498, 41672.113, 40894.593, 40276.776, 40224.166, 39766.320, 38362.988, 36150.082, 41153.722, 38921.056, 35357.544 |

**Peak:** 39859 mut/s at N=256 — 21.99× the sequential (N=1) baseline of 1813 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.4% | 0.2% | 54.6% | 43.8% | 2317.924 ms |
| 32 | 16.02 | 133920/0 | 13.0% | 1.7% | 48.6% | 36.7% | 7667.050 ms |
| 256 | 122.20 | 126720/0 | 29.3% | 5.5% | 38.9% | 26.3% | 3575.976 ms |
