# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 4434 | [4225, 4642] | 4432 | 8.5 | 1.00× | 213.5 | 304.9 | 627.8 | 1.0 |
| 32 | 28727 | [28330, 29123] | 28810 | 2.5 | 6.48× | 1020.1 | 1968.3 | 3361.9 | 31.9 |
| 256 | 40812 | [38839, 42784] | 41857 | 8.7 | 9.21× | 5552.6 | 10145.2 | 22498.8 | 253.2 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 4197.338, 4429.278, 5548.953, 4121.087, 4141.199, 3823.543, 4263.971, 4327.074, 4506.887, 4432.163, 4692.354, 4512.777, 4526.696, 4516.315, 4463.220 |
| 32 | 27860.915, 29535.538, 28308.019, 28063.205, 28810.056, 29194.582, 28863.962, 28676.921, 29533.494, 29659.713, 27421.586, 28754.931, 29475.447, 29031.857, 27711.221 |
| 256 | 41829.453, 41599.703, 42719.248, 43825.772, 40858.835, 42109.644, 41856.767, 42438.693, 43781.482, 43921.450, 42666.406, 39191.081, 39089.975, 35661.883, 30626.284 |

**Peak:** 40812 mut/s at N=256 — 9.21× the sequential (N=1) baseline of 4434 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 5.5% | 0.9% | 58.9% | 34.7% | 733.237 ms |
| 32 | 15.27 | 133920/0 | 23.7% | 4.5% | 45.9% | 25.9% | 5060.454 ms |
| 256 | 116.26 | 126720/0 | 33.4% | 7.0% | 37.4% | 22.3% | 3435.310 ms |
