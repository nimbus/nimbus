# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=hotkey (one shared-document update), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `hotkey (one shared-document update)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor;
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 514 | [500, 527] | 518 | 4.7 | 1.00× | 1909.5 | 2187.6 | 2938.8 | 1.0 |
| 32 | 3007 | [2996, 3018] | 3014 | 0.7 | 5.86× | 10564.7 | 11208.0 | 11535.3 | 31.8 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 527.052, 470.248, 510.829, 477.303, 535.256, 520.917, 514.726, 518.058, 511.607, 541.074, 494.400, 473.739, 530.473, 537.402, 540.445 |
| 32 | 3015.797, 3019.026, 3010.571, 2994.635, 2978.560, 3014.492, 3037.057, 3029.867, 3024.983, 3019.489, 3018.326, 2987.446, 2978.205, 2976.631, 3001.927 |

**Peak:** 3007 mut/s at N=32 — 5.86× the sequential (N=1) baseline of 514 mut/s.
