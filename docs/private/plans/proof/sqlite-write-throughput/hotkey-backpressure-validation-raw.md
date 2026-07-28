# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256]

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 586 | [571, 601] | 596 | 4.7 | 1.00× | 1672.3 | 1815.9 | 2294.8 | 1.0 |
| 32 | 2487 | [2321, 2653] | 2556 | 12.1 | 4.24× | 12284.0 | 14044.2 | 49700.4 | 32.5 |
| 256 | 1970 | [1673, 2268] | 2178 | 27.2 | 3.36× | 83298.5 | 405635.5 | 932907.5 | 282.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 592.384, 503.720, 595.492, 600.706, 540.794, 587.337, 600.600, 595.885, 595.370, 602.195, 581.759, 595.508, 598.526, 600.375, 599.712 |
| 32 | 1458.116, 2718.308, 2585.572, 2404.224, 2679.877, 2674.032, 2481.730, 2617.031, 2624.119, 2586.477, 2506.468, 2489.605, 2555.884, 2521.841, 2404.998 |
| 256 | 2196.430, 2265.700, 2227.607, 447.672, 2269.659, 2214.762, 2178.395, 2205.434, 2146.719, 920.701, 2138.573, 2262.456, 1981.279, 2019.787, 2081.571 |

**Peak:** 2487 mut/s at N=32 — 4.24× the sequential (N=1) baseline of 586 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 0.00 | 1500/0 | 4.2% | 0.1% | 0.5% | 95.1% | 603.510 ms |
| 32 | 0.00 | 48000/0 | 4.8% | 5.4% | 0.4% | 89.3% | 18914.765 ms |
| 256 | 0.00 | 137035/0 | 5.3% | 4.4% | 1.0% | 89.3% | 81528.320 ms |
