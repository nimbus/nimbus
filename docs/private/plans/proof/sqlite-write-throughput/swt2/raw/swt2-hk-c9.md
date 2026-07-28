# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 683 | [665, 701] | 680 | 4.7 | 1.00× | 1451.9 | 1553.7 | 1699.5 | 1.0 |
| 32 | 8167 | [8076, 8257] | 8101 | 2.0 | 11.96× | 3862.7 | 4432.0 | 4679.8 | 31.8 |
| 256 | 6576 | [6391, 6761] | 6612 | 5.1 | 9.63× | 28550.9 | 83111.4 | 129557.0 | 242.3 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 720.147, 724.006, 706.303, 707.317, 697.129, 684.734, 677.092, 704.015, 668.748, 590.899, 673.146, 662.623, 680.489, 673.677, 672.782 |
| 32 | 8120.406, 8142.830, 8100.533, 8098.555, 8013.478, 7971.057, 8077.476, 8094.414, 7960.262, 8049.812, 8450.594, 8365.785, 8267.651, 8403.462, 8382.522 |
| 256 | 6743.581, 6903.777, 6967.015, 6039.866, 7014.774, 6804.508, 6746.121, 6828.526, 6139.150, 5942.969, 6426.809, 6385.533, 6556.464, 6611.615, 6527.275 |

**Peak:** 8167 mut/s at N=32 — 11.96× the sequential (N=1) baseline of 683 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 10.6% | 0.3% | 0.9% | 88.2% | 243.167 ms |
| 32 | 0.00 | 48000/0 | 12.3% | 11.7% | 1.1% | 75.0% | 5204.553 ms |
| 256 | 0.00 | 134400/0 | 15.1% | 12.7% | 1.2% | 71.0% | 18309.289 ms |
