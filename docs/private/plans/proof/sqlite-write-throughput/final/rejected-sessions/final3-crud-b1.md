# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1942 | [1932, 1953] | 1943 | 1.0 | 1.00× | 501.8 | 589.2 | 726.0 | 1.0 |
| 32 | 16562 | [16428, 16695] | 16575 | 1.5 | 8.53× | 1864.8 | 2111.2 | 3839.1 | 31.9 |
| 256 | 28785 | [28369, 29200] | 28815 | 2.6 | 14.82× | 8394.9 | 12742.8 | 15212.2 | 251.2 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1962.551, 1978.756, 1932.540, 1927.494, 1907.253, 1918.638, 1932.425, 1957.578, 1945.825, 1957.469, 1929.562, 1947.284, 1943.180, 1935.862, 1957.096 |
| 32 | 16843.278, 16910.786, 16652.707, 16782.216, 16842.457, 16087.956, 16466.703, 16157.424, 16426.047, 16410.064, 16598.342, 16575.098, 16419.587, 16557.618, 16692.956 |
| 256 | 28726.836, 28556.644, 28870.742, 29403.251, 28990.734, 28427.451, 29249.947, 28814.739, 30194.452, 29384.957, 28123.896, 28634.328, 29004.187, 28641.872, 26744.986 |

**Peak:** 28785 mut/s at N=256 — 14.82× the sequential (N=1) baseline of 1942 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.6% | 0.2% | 53.5% | 44.7% | 15040.727 ms |
| 32 | 16.01 | 133920/0 | 11.3% | 1.4% | 56.1% | 31.2% | 8552.055 ms |
| 256 | 124.60 | 126720/0 | 21.1% | 3.9% | 57.1% | 17.9% | 4871.500 ms |
