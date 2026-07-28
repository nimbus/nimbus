# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1946 | [1909, 1983] | 1956 | 3.4 | 1.00× | 498.0 | 575.0 | 722.6 | 1.0 |
| 32 | 16292 | [16128, 16456] | 16324 | 1.8 | 8.37× | 1891.8 | 2237.7 | 3918.2 | 31.9 |
| 256 | 28280 | [28107, 28453] | 28231 | 1.1 | 14.53× | 8514.5 | 12833.5 | 15323.6 | 250.7 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1964.813, 1955.581, 1949.004, 1946.679, 1958.404, 1908.570, 1933.748, 1946.795, 1943.447, 1955.749, 1967.702, 1729.611, 1995.956, 2018.070, 2017.672 |
| 32 | 16555.082, 16281.054, 15962.467, 16277.116, 16448.233, 16323.876, 16315.746, 16738.814, 15422.435, 16419.525, 16340.637, 16301.120, 16177.670, 16363.085, 16451.401 |
| 256 | 28077.483, 28604.776, 28424.924, 28207.906, 28630.891, 28787.099, 28009.554, 28231.166, 28315.357, 28494.738, 27941.201, 27887.362, 28156.172, 28657.283, 27766.753 |

**Peak:** 28280 mut/s at N=256 — 14.53× the sequential (N=1) baseline of 1946 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.6% | 0.2% | 53.9% | 44.3% | 15074.070 ms |
| 32 | 16.01 | 133920/0 | 11.3% | 1.4% | 56.3% | 31.0% | 8696.571 ms |
| 256 | 125.22 | 126720/0 | 20.9% | 3.8% | 57.0% | 18.2% | 4919.660 ms |
