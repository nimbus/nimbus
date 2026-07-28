# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 7331 | [7259, 7404] | 7342 | 1.8 | 1.00× | 129.8 | 162.0 | 250.9 | 1.0 |
| 32 | 38514 | [36204, 40825] | 39660 | 10.8 | 5.25× | 748.0 | 1129.8 | 2428.5 | 32.4 |
| 256 | 51843 | [50884, 52803] | 52005 | 3.3 | 7.07× | 4544.2 | 8038.7 | 9500.6 | 251.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7251.274, 7155.749, 7352.213, 7236.661, 7426.677, 7212.059, 7341.702, 7133.699, 7342.137, 7429.835, 7434.422, 7528.341, 7589.197, 7282.776, 7255.395 |
| 32 | 41052.680, 40455.731, 40159.309, 39659.796, 40172.425, 40074.504, 39582.473, 37586.950, 38848.252, 38770.684, 37947.056, 23845.071, 39910.691, 39048.926, 40602.635 |
| 256 | 54385.858, 52752.262, 52750.862, 53321.930, 50974.681, 49125.395, 49810.219, 51396.371, 52245.531, 52004.854, 49007.994, 51929.811, 50554.586, 52578.881, 54809.837 |

**Peak:** 51843 mut/s at N=256 — 7.07× the sequential (N=1) baseline of 7331 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.9% | 0.7% | 58.2% | 35.1% | 485.125 ms |
| 32 | 14.44 | 133920/0 | 24.4% | 3.6% | 43.2% | 28.7% | 4116.808 ms |
| 256 | 113.85 | 126720/0 | 37.0% | 6.8% | 36.6% | 19.7% | 2836.985 ms |
