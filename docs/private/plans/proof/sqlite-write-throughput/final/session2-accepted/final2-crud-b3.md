# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1961 | [1930, 1993] | 1956 | 2.9 | 1.00× | 497.9 | 577.0 | 705.2 | 1.0 |
| 32 | 16041 | [15883, 16199] | 16097 | 1.8 | 8.18× | 1920.5 | 2221.7 | 3962.0 | 31.9 |
| 256 | 27900 | [27495, 28306] | 27923 | 2.6 | 14.23× | 8695.0 | 13329.2 | 16340.0 | 251.4 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 2010.287, 2036.793, 2084.087, 1956.263, 1983.736, 1910.879, 1855.275, 1927.346, 1986.679, 1975.299, 1980.732, 1940.371, 1910.856, 1925.833, 1935.024 |
| 32 | 15961.730, 16008.610, 16345.922, 16443.935, 16233.252, 16116.651, 15805.585, 15684.626, 16315.064, 16096.719, 16089.300, 16160.456, 16236.133, 15398.548, 15715.975 |
| 256 | 28445.877, 28803.854, 29187.732, 27450.894, 27033.730, 26770.637, 27335.058, 28013.056, 28571.908, 27408.719, 28831.797, 27993.922, 27211.429, 27922.734, 27525.495 |

**Peak:** 27900 mut/s at N=256 — 14.23× the sequential (N=1) baseline of 1961 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.6% | 0.2% | 53.3% | 45.0% | 2133.976 ms |
| 32 | 16.00 | 133920/0 | 11.2% | 1.4% | 56.1% | 31.3% | 8827.457 ms |
| 256 | 126.47 | 126720/0 | 21.3% | 3.7% | 56.7% | 18.3% | 4952.433 ms |
