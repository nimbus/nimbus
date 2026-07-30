# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=insert, base_units/worker=300, max_mut/round=24000, measure_rounds=5, warmup_rounds=2, seed_docs=0, ladder=[1, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `insert`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 4124 | [3913, 4335] | 4143 | 4.1 | 1.00× | 232.2 | 305.6 | 376.3 | 1.0 |
| 256 | 22113 | [21505, 22721] | 22064 | 2.2 | 5.36× | 11254.4 | 15261.1 | 17547.4 | 253.0 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 4142.724, 4170.832, 3918.283, 4021.310, 4369.164 |
| 256 | 22063.962, 22716.568, 21386.936, 22038.229, 22359.375 |

**Peak:** 22113 mut/s at N=256 — 5.36× the sequential (N=1) baseline of 4124 mut/s.

## Open-loop service latency (SUC6.1)

Calibrated closed-loop capacity at the top rung: **22113 mut/s**. Each round drives single-document inserts on a fixed arrival schedule for 30s; latency is measured from the scheduled arrival (coordinated-omission-free). A saturation-breached round means the offered rate was not sustainable and its numbers are not service-latency evidence; a round with shed arrivals means the admission gate rejected bursts at this rate, so its percentiles describe only the admitted subset.

| Fraction | Target mut/s | Round | Sched | Done | Shed | Achieved | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | Verdict |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 0.25 | 5528 | 1 | 165847 | 165847 | 0 | 5528 | 1.72 | 2.36 | 2.93 | 36.46 | 58.90 | ok |
| 0.25 | 5528 | 2 | 165847 | 165847 | 0 | 5528 | 1.75 | 2.39 | 2.95 | 4.66 | 9.73 | ok |
| 0.25 | 5528 | 3 | 165847 | 165847 | 0 | 5528 | 1.76 | 2.39 | 2.95 | 6.27 | 11.65 | ok |
| 0.5 | 11057 | 1 | 331695 | 331695 | 0 | 11056 | 2.11 | 2.77 | 3.54 | 13.83 | 21.08 | ok |
| 0.5 | 11057 | 2 | 331695 | 331695 | 0 | 11056 | 2.18 | 2.85 | 3.60 | 4.27 | 10.04 | ok |
| 0.5 | 11057 | 3 | 331695 | 331695 | 664 | 11034 | 2.23 | 2.93 | 3.82 | 80.73 | 104.67 | SHED — rate not absorbed |
| 0.75 | 16585 | 1 | 497542 | 497542 | 0 | 16583 | 3.94 | 5.27 | 7.07 | 31.60 | 40.99 | ok |
| 0.75 | 16585 | 2 | 497542 | 497542 | 0 | 16583 | 4.47 | 6.06 | 7.98 | 13.31 | 19.68 | ok |
| 0.75 | 16585 | 3 | 497542 | 497542 | 0 | 16583 | 4.86 | 6.56 | 8.52 | 10.38 | 14.00 | ok |
