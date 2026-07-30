# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=insert, base_units/worker=300, max_mut/round=24000, measure_rounds=5, warmup_rounds=2, seed_docs=0, ladder=[1, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `insert`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 4164 | [4134, 4194] | 4160 | 0.6 | 1.00× | 230.4 | 293.2 | 483.6 | 1.0 |
| 256 | 22378 | [22106, 22650] | 22481 | 1.0 | 5.37× | 11113.0 | 15234.1 | 17584.0 | 252.6 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 4172.697, 4159.826, 4135.738, 4200.042, 4152.914 |
| 256 | 22136.954, 22550.139, 22145.368, 22480.572, 22577.116 |

**Peak:** 22378 mut/s at N=256 — 5.37× the sequential (N=1) baseline of 4164 mut/s.

## Open-loop service latency (SUC6.1)

Calibrated closed-loop capacity at the top rung: **22378 mut/s**. Each round drives single-document inserts on a fixed arrival schedule for 30s; latency is measured from the scheduled arrival (coordinated-omission-free). A saturation-breached round means the offered rate was not sustainable and its numbers are not service-latency evidence; a round with shed arrivals means the admission gate rejected bursts at this rate, so its percentiles describe only the admitted subset.


**Fraction 0.25: cross-round CV gate PASS** (achieved-rate CV 0.0%, p99 CV 0.2%, gate ≤10% each; p99.9 ungated by design).

| Fraction | Target mut/s | Round | Sched | Done | Shed | Achieved | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | max disp lag ms | Verdict |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 0.25 | 5595 | 1 | 167835 | 167835 | 0 | 5594 | 1.72 | 2.37 | 2.95 | 37.61 | 60.52 | 50.98 | ok |
| 0.25 | 5595 | 2 | 167835 | 167835 | 0 | 5594 | 1.74 | 2.38 | 2.96 | 4.67 | 9.16 | 3.23 | ok |
| 0.25 | 5595 | 3 | 167835 | 167835 | 0 | 5594 | 1.75 | 2.39 | 2.95 | 5.64 | 11.21 | 3.02 | ok |

**Fraction 0.5: cross-round CV gate PASS** (achieved-rate CV 0.1%, p99 CV 4.7%, gate ≤10% each; p99.9 ungated by design).

| Fraction | Target mut/s | Round | Sched | Done | Shed | Achieved | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | max disp lag ms | Verdict |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 0.5 | 11189 | 1 | 335670 | 335670 | 0 | 11188 | 2.13 | 2.81 | 3.61 | 19.32 | 31.11 | 4.45 | ok |
| 0.5 | 11189 | 2 | 335670 | 335670 | 0 | 11188 | 2.22 | 2.91 | 3.65 | 4.31 | 6.34 | 2.40 | ok |
| 0.5 | 11189 | 3 | 335670 | 335670 | 722 | 11164 | 2.25 | 2.98 | 3.93 | 72.38 | 104.77 | 82.03 | SHED — rate not absorbed |

**Fraction 0.75: cross-round CV gate FAIL** (achieved-rate CV 0.0%, p99 CV 72.0%, gate ≤10% each; p99.9 ungated by design).

| Fraction | Target mut/s | Round | Sched | Done | Shed | Achieved | p50 ms | p90 ms | p99 ms | p99.9 ms | max ms | max disp lag ms | Verdict |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 0.75 | 16784 | 1 | 503505 | 503505 | 0 | 16782 | 4.27 | 5.82 | 7.70 | 25.55 | 33.34 | 4.26 | ok |
| 0.75 | 16784 | 2 | 503505 | 503505 | 0 | 16782 | 5.06 | 6.89 | 9.00 | 11.01 | 17.42 | 4.55 | ok |
| 0.75 | 16784 | 3 | 503505 | 503505 | 0 | 16782 | 5.69 | 7.92 | 26.11 | 55.22 | 64.97 | 6.59 | ok |
