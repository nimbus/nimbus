# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 6416 | [6195, 6637] | 6297 | 6.2 | 1.00× | 151.4 | 186.7 | 252.0 | 1.0 |
| 32 | 38147 | [37778, 38516] | 38259 | 1.7 | 5.95× | 772.7 | 1112.5 | 2487.3 | 31.8 |
| 256 | 52849 | [51780, 53918] | 52894 | 3.7 | 8.24× | 4430.5 | 7821.5 | 9424.1 | 251.5 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 7320.689, 7319.357, 6722.833, 6047.970, 6354.098, 6296.811, 6328.174, 6288.336, 6329.965, 6214.472, 6068.094, 6211.898, 6134.650, 6239.914, 6360.733 |
| 32 | 39449.672, 38690.393, 38259.421, 38686.558, 38008.634, 37449.835, 37695.924, 36696.720, 38346.130, 38525.927, 38719.393, 37838.345, 37964.053, 37501.129, 38377.909 |
| 256 | 53581.664, 49905.396, 52423.561, 51859.254, 52951.062, 49427.596, 51371.563, 55701.282, 53585.899, 54765.186, 51011.784, 52893.721, 54639.870, 55843.530, 52771.374 |

**Peak:** 52849 mut/s at N=256 — 8.24× the sequential (N=1) baseline of 6416 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.4% | 0.7% | 57.4% | 36.5% | 551.814 ms |
| 32 | 14.57 | 133920/0 | 25.3% | 3.7% | 44.4% | 26.6% | 4110.129 ms |
| 256 | 111.16 | 126720/0 | 37.3% | 6.4% | 36.0% | 20.3% | 2833.942 ms |
