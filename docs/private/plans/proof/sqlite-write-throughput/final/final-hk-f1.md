# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 677 | [670, 683] | 678 | 1.7 | 1.00× | 1470.5 | 1573.1 | 1685.6 | 1.0 |
| 32 | 8041 | [7445, 8637] | 8441 | 13.4 | 11.89× | 3750.0 | 4352.5 | 4923.9 | 32.6 |
| 256 | 6714 | [6548, 6880] | 6751 | 4.5 | 9.92× | 27866.3 | 81402.4 | 124152.8 | 242.2 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 658.915, 662.002, 686.633, 677.888, 679.155, 680.442, 674.496, 673.591, 692.209, 696.927, 685.242, 665.295, 660.332, 678.119, 677.496 |
| 32 | 8558.793, 8458.749, 8187.195, 8210.814, 8160.237, 5888.902, 8340.804, 8320.257, 8551.576, 8562.463, 8488.677, 8441.431, 8487.157, 5036.316, 8928.414 |
| 256 | 7009.182, 6954.389, 6681.456, 6614.895, 7005.437, 6962.814, 6509.232, 6502.927, 5968.463, 7058.704, 6901.763, 6389.063, 6855.877, 6750.922, 6541.893 |

**Peak:** 8041 mut/s at N=32 — 11.89× the sequential (N=1) baseline of 677 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 11.5% | 0.3% | 1.0% | 87.2% | 251.292 ms |
| 32 | 0.00 | 48000/0 | 14.5% | 10.9% | 0.9% | 73.7% | 5649.692 ms |
| 256 | 0.00 | 134400/0 | 14.0% | 12.7% | 1.2% | 72.2% | 17982.591 ms |
