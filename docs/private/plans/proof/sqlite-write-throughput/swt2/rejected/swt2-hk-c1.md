# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 654 | [626, 682] | 627 | 7.6 | 1.00× | 1543.2 | 1736.2 | 2090.2 | 1.0 |
| 32 | 5840 | [5331, 6349] | 6028 | 15.7 | 8.93× | 5152.6 | 6076.0 | 10462.1 | 33.4 |
| 256 | 4932 | [4431, 5433] | 5126 | 18.3 | 7.54× | 37825.0 | 103357.0 | 161056.5 | 264.7 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 707.769, 720.330, 716.146, 706.781, 698.843, 713.068, 596.121, 616.986, 654.710, 626.513, 622.504, 603.411, 614.196, 611.340, 600.939 |
| 32 | 6028.357, 6381.982, 5986.146, 6431.197, 6000.139, 6071.775, 6288.186, 6028.004, 5975.958, 5878.044, 6236.896, 2589.673, 5760.538, 5850.535, 6096.087 |
| 256 | 5255.533, 5323.608, 5093.220, 5233.363, 1687.076, 5093.012, 5098.925, 5126.116, 5214.093, 5018.369, 5088.241, 5239.190, 4936.758, 5314.806, 5259.511 |

**Peak:** 5840 mut/s at N=32 — 8.93× the sequential (N=1) baseline of 654 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 12.1% | 0.4% | 1.9% | 85.6% | 319.479 ms |
| 32 | 0.00 | 48000/0 | 18.5% | 9.8% | 1.3% | 70.5% | 7958.959 ms |
| 256 | 0.00 | 135135/0 | 76.9% | 3.0% | 0.5% | 19.6% | 98292.677 ms |
