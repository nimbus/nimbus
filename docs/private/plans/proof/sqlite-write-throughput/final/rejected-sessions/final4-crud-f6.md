# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7285 | [7165, 7404] | 7323 | 3.0 | 1.00× | 131.1 | 165.2 | 229.8 | 1.0 |
| 32 | 40805 | [40454, 41157] | 40731 | 1.6 | 5.60× | 714.6 | 1032.7 | 2420.6 | 31.8 |
| 256 | 51592 | [50506, 52679] | 52203 | 3.8 | 7.08× | 4559.7 | 7932.2 | 9363.2 | 251.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7177.128, 7446.200, 7314.905, 7319.362, 7328.094, 7400.126, 7306.885, 7227.423, 7536.344, 7323.075, 7346.106, 7356.004, 7343.951, 7274.472, 6568.291 |
| 32 | 41214.650, 41057.211, 40814.601, 40566.782, 40531.783, 40831.423, 40730.608, 40505.487, 41594.559, 40910.176, 40325.287, 40114.702, 40109.641, 42543.256, 40229.513 |
| 256 | 53019.146, 52737.964, 51077.620, 54826.822, 53960.818, 49581.138, 50103.022, 52320.977, 50837.883, 49270.787, 52803.891, 52203.131, 50932.335, 52818.005, 47392.935 |

**Peak:** 51592 mut/s at N=256 — 7.08× the sequential (N=1) baseline of 7285 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 5.9% | 0.7% | 58.5% | 35.0% | 3423.792 ms |
| 32 | 14.60 | 133920/0 | 25.2% | 3.7% | 44.7% | 26.4% | 3832.756 ms |
| 256 | 111.65 | 126720/0 | 38.1% | 6.4% | 35.7% | 19.7% | 2938.824 ms |
