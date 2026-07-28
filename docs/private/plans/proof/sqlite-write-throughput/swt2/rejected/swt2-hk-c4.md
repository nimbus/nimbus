# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 641 | [625, 657] | 641 | 4.5 | 1.00× | 1584.5 | 1736.2 | 1946.6 | 1.0 |
| 32 | 6382 | [6066, 6699] | 6402 | 8.9 | 9.96× | 4976.9 | 5639.4 | 6168.8 | 32.1 |
| 256 | 5195 | [5132, 5257] | 5156 | 2.2 | 8.11× | 37631.7 | 101345.9 | 149379.1 | 241.7 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 656.818, 608.512, 622.926, 657.795, 719.397, 667.266, 648.362, 617.803, 607.112, 619.387, 643.828, 641.287, 630.946, 648.494, 618.327 |
| 32 | 6729.976, 4689.634, 6393.433, 6399.451, 6323.618, 6402.471, 6531.636, 7014.451, 6171.479, 6438.342, 6338.783, 6438.434, 6042.830, 7372.756, 6448.766 |
| 256 | 5318.817, 5220.841, 5134.276, 5189.643, 5425.672, 5108.504, 5098.133, 5117.058, 5061.822, 5184.566, 5069.523, 5145.539, 5333.645, 5357.323, 5155.719 |

**Peak:** 6382 mut/s at N=32 — 9.96× the sequential (N=1) baseline of 641 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 12.3% | 0.4% | 1.9% | 85.3% | 356.168 ms |
| 32 | 0.00 | 48000/0 | 12.0% | 11.8% | 1.4% | 74.7% | 6342.589 ms |
| 256 | 0.00 | 134400/0 | 13.8% | 13.2% | 2.1% | 70.9% | 22015.678 ms |
