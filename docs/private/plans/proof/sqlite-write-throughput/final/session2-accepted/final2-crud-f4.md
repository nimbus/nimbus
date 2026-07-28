# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7336 | [7277, 7395] | 7297 | 1.5 | 1.00× | 129.9 | 159.8 | 235.4 | 1.0 |
| 32 | 38116 | [37465, 38767] | 37751 | 3.1 | 5.20× | 771.8 | 1106.3 | 2565.4 | 31.9 |
| 256 | 50605 | [49500, 51710] | 50399 | 3.9 | 6.90× | 4678.8 | 8256.8 | 10067.7 | 251.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7255.322, 7499.898, 7457.863, 7297.206, 7442.837, 7516.952, 7214.725, 7370.841, 7408.772, 7353.287, 7213.807, 7229.671, 7287.966, 7254.693, 7241.967 |
| 32 | 40400.719, 39594.044, 40406.715, 38127.287, 38177.428, 37188.256, 37558.007, 37751.393, 37593.393, 37481.410, 38147.223, 37007.573, 37219.308, 36542.788, 38545.811 |
| 256 | 47723.927, 52191.641, 52686.614, 47464.519, 49243.898, 53544.308, 49625.238, 51010.206, 48922.096, 54332.779, 51643.166, 50622.975, 50399.451, 49924.148, 49738.748 |

**Peak:** 50605 mut/s at N=256 — 6.90× the sequential (N=1) baseline of 7336 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.9% | 0.7% | 58.0% | 35.3% | 484.859 ms |
| 32 | 14.47 | 133920/0 | 25.1% | 3.6% | 44.6% | 26.7% | 4079.523 ms |
| 256 | 110.29 | 126720/0 | 37.6% | 6.7% | 34.8% | 20.9% | 2967.458 ms |
