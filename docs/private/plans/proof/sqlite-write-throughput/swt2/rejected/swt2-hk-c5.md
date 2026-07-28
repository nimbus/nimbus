# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 609 | [596, 622] | 605 | 3.8 | 1.00× | 1621.8 | 1853.1 | 2452.4 | 1.0 |
| 32 | 6119 | [6003, 6235] | 6065 | 3.4 | 10.05× | 5223.5 | 5894.0 | 6142.0 | 31.9 |
| 256 | 5150 | [5026, 5274] | 5135 | 4.3 | 8.46× | 37778.9 | 102863.5 | 151530.5 | 242.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 638.578, 645.761, 614.766, 603.634, 620.715, 583.652, 599.205, 577.862, 568.931, 593.883, 598.258, 604.866, 645.240, 622.553, 612.670 |
| 32 | 6199.600, 6064.619, 6078.137, 5980.836, 6170.599, 5959.267, 6134.336, 6037.519, 6051.093, 6084.921, 5976.603, 6026.877, 6039.159, 6147.278, 6833.569 |
| 256 | 5234.807, 5297.388, 5068.047, 4555.779, 5461.123, 5389.011, 5187.665, 5092.749, 5216.937, 5425.038, 5122.615, 4902.956, 5135.077, 5103.280, 5056.634 |

**Peak:** 6119 mut/s at N=32 — 10.05× the sequential (N=1) baseline of 609 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 12.5% | 0.4% | 1.8% | 85.2% | 424.834 ms |
| 32 | 0.00 | 48000/0 | 11.9% | 11.9% | 1.4% | 74.8% | 6503.401 ms |
| 256 | 0.00 | 134400/0 | 14.3% | 13.1% | 2.1% | 70.5% | 22402.078 ms |
