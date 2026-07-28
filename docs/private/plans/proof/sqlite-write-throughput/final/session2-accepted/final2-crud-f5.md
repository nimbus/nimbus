# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6812 | [5920, 7705] | 7273 | 23.7 | 1.00× | 131.8 | 188.8 | 370.0 | 1.3 |
| 32 | 38313 | [37965, 38660] | 38424 | 1.6 | 5.62× | 766.5 | 1091.8 | 2530.6 | 31.9 |
| 256 | 51039 | [50125, 51953] | 50767 | 3.2 | 7.49× | 4673.7 | 8176.3 | 9734.3 | 251.6 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7272.580, 7608.878, 7551.302, 7530.758, 7439.607, 7502.829, 7230.978, 7290.622, 1135.338, 6169.217, 6816.607, 7227.588, 7083.017, 7016.525, 7308.406 |
| 32 | 39060.152, 37498.241, 38281.971, 39257.085, 39283.333, 38287.046, 38485.627, 38424.349, 38438.356, 37826.321, 37112.007, 38516.377, 37505.376, 38271.078, 38442.162 |
| 256 | 55330.370, 50767.146, 48931.907, 49999.938, 51958.729, 48847.323, 52573.087, 50713.560, 49583.709, 52655.554, 50232.604, 50424.808, 51548.433, 50895.488, 51122.104 |

**Peak:** 51039 mut/s at N=256 — 7.49× the sequential (N=1) baseline of 6812 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 4.1% | 0.5% | 68.2% | 27.2% | 712.813 ms |
| 32 | 14.71 | 133920/0 | 25.0% | 4.0% | 43.9% | 27.1% | 4050.579 ms |
| 256 | 111.75 | 126720/0 | 38.0% | 6.5% | 34.4% | 21.1% | 2953.050 ms |
