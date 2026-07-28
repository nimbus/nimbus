# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 4911 | [4413, 5410] | 4613 | 18.3 | 1.00× | 200.0 | 278.3 | 657.4 | 1.0 |
| 32 | 27402 | [26418, 28385] | 27928 | 6.5 | 5.58× | 1059.3 | 2119.8 | 3407.9 | 32.0 |
| 256 | 40982 | [37619, 44346] | 42260 | 14.8 | 8.34× | 5511.9 | 10415.5 | 13399.0 | 259.2 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 3727.590, 6756.028, 6813.491, 5165.997, 4514.285, 4462.559, 4612.602, 5017.334, 5862.586, 4369.945, 4469.038, 4651.097, 4061.285, 4617.862, 4567.615 |
| 32 | 27927.775, 23560.730, 28642.643, 29739.682, 26220.633, 24795.586, 27949.632, 26474.549, 27518.365, 28371.548, 26167.072, 26775.087, 28417.438, 30269.413, 28195.636 |
| 256 | 45242.197, 20733.663, 42319.905, 42259.704, 41975.189, 42938.302, 37605.094, 46748.525, 41795.927, 40863.051, 39932.870, 40521.743, 43525.012, 46006.943, 42267.897 |

**Peak:** 40982 mut/s at N=256 — 8.34× the sequential (N=1) baseline of 4911 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.7% | 0.9% | 57.9% | 35.6% | 682.230 ms |
| 32 | 15.21 | 133920/0 | 23.9% | 4.4% | 46.1% | 25.7% | 5316.504 ms |
| 256 | 114.78 | 126720/0 | 34.1% | 6.7% | 36.6% | 22.6% | 3540.691 ms |
