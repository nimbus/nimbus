# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=700, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 32, 256], wal_checkpoint_observation=false

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1954 | [1946, 1961] | 1951 | 0.7 | 1.00× | 501.0 | 576.9 | 703.1 | 1.0 |
| 32 | 14737 | [13055, 16420] | 15721 | 20.6 | 7.54× | 1944.8 | 2611.8 | 4280.3 | 35.6 |
| 256 | 27563 | [27223, 27904] | 27609 | 2.2 | 14.11× | 8809.2 | 13434.2 | 15681.7 | 251.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1937.418, 1964.776, 1949.384, 1961.475, 1984.377, 1948.217, 1956.336, 1951.351, 1946.727, 1962.982, 1944.835, 1956.637, 1964.978, 1928.198, 1948.584 |
| 32 | 15721.230, 15926.023, 15686.639, 15457.923, 16077.838, 15505.402, 4466.708, 11895.266, 16213.836, 14461.450, 15979.659, 16028.370, 15643.732, 15976.129, 16015.744 |
| 256 | 27471.767, 27921.354, 26971.784, 27781.904, 27609.373, 28235.864, 27589.871, 27822.605, 28132.247, 26404.954, 26258.406, 27408.671, 28168.227, 28236.643, 27435.631 |

**Peak:** 27563 mut/s at N=256 — 14.11× the sequential (N=1) baseline of 1954 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 31500/0 | 1.6% | 0.2% | 53.5% | 44.7% | 14971.241 ms |
| 32 | 16.00 | 133920/0 | 9.5% | 1.2% | 48.2% | 41.1% | 10610.654 ms |
| 256 | 125.96 | 126720/0 | 21.6% | 3.9% | 56.2% | 18.3% | 5053.233 ms |
