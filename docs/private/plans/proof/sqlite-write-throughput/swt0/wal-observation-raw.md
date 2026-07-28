# Concurrent write-throughput (group-commit sweep)

backend: `sqlite`  |  workload=crud (insert+update+delete), base_units/worker=100, max_mut/round=9000, measure_rounds=15, warmup_rounds=3, seed_docs=0, ladder=[1, 256], wal_checkpoint_observation=true

Closed-loop, single-tenant, workload = `crud (insert+update+delete)`. Throughput is durable mutations/sec. N=1 (batch size 1) is this harness's own sequential anchor; it replays the sequential CRUD baseline's shape, so it should land NEAR ~2,661 mutations/s as a cross-check (not bit-identical — a separate harness);
`speedup` = mean_tps(N) / mean_tps(1). Little's Law: `N≈X·R` should ~match N.
Latency percentiles are closed-loop (queue) latency — not SLA service time at saturated rungs (coordinated omission).

| N | throughput mut/s (mean) | 95% CI | median | CV% | speedup | p50 µs | p95 µs | p99 µs | N≈X·R |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 1752 | [1650, 1853] | 1809 | 10.4 | 1.00× | 532.0 | 804.2 | 1245.6 | 1.0 |
| 256 | 25152 | [22901, 27403] | 26557 | 16.2 | 14.36× | 9316.0 | 15379.2 | 24088.7 | 261.1 |

## Raw measured-round samples

These durable-mutation throughput samples are the exact round inputs to the summary statistics and Student-t confidence intervals above.

| N | measured mut/s samples |
|---:|---|
| 1 | 1969.906, 1796.526, 1542.804, 1411.517, 1536.076, 1521.709, 1844.914, 1963.357, 1907.487, 1888.071, 1939.363, 1817.323, 1609.401, 1808.598, 1717.013 |
| 256 | 26567.047, 27977.432, 20878.783, 23697.437, 26490.931, 25550.074, 28092.434, 27973.193, 27426.000, 27148.976, 27983.660, 25570.990, 26557.394, 22752.532, 12613.519 |

**Peak:** 25152 mut/s at N=256 — 14.36× the sequential (N=1) baseline of 1752 mut/s.

## Commit phase split

Shares use measured-round commit phase time: `plan-CPU = prepare` (validation, authorization, serialization); `conflict-check` includes assign-time in-memory window validation and path C's sampled pre-append shadow observation, while path A's sampled shadow observation runs outside its serial closure; `apply = apply + publish` (storage apply plus engine visibility bookkeeping); `fsync/append = durable-append`. Ordered-publisher persistence phases execute after serial assignment, so this is not an under-gate measurement. Average effective batch is `journal_batch_size_sum / journal_batch_count`; each journal batch performs one durable append.

| N | avg effective batch | window/storage prepare | plan-CPU | conflict-check | apply | fsync/append | measured phase time |
|---|---|---|---|---|---|---|---|
| 1 | 1.00 | 4500/0 | 1.5% | 0.2% | 53.9% | 44.4% | 2400.094 ms |
| 256 | 128.91 | 126720/0 | 20.2% | 3.3% | 57.6% | 18.9% | 5514.281 ms |

## WAL/checkpoint observation (diagnostic only)

Enabled explicitly with `NIMBUS_CWB_WAL_CHECKPOINT_OBSERVATION=1`. It is off in canonical timed runs, and the test-only statement counters are not compiled into this benchmark. During measured rounds, each successful foreground COMMIT runs a read-only `wal_checkpoint(NOOP)` status query. `NOOP probe share` is that query's measured wall time divided by measured-round wall time, so throughput from this diagnostic mode is not acceptance evidence. The one `PASSIVE` probe runs only after each rung and is reported separately. A nonzero `probe errors` count means the frame and checkpoint columns are incomplete; treat that rung's diagnostic as invalid and rerun.

`automatic checkpoints` counts post-COMMIT samples whose WAL frame count reached the connection's auto-checkpoint threshold. Sampling runs after COMMIT releases the writer lock, so per-commit attribution relies on the per-tenant committer serializing writers, which holds for this benchmark's workloads; treat the columns as sampled aggregate WAL state. SQLite does not expose checkpoint-only COMMIT time, so `automatic COMMIT upper bound` includes all work in those COMMIT calls.

| N | foreground commits | automatic checkpoints | automatic COMMIT upper bound ms | WAL high-water frames | checkpointed high-water frames | auto threshold pages | NOOP probes | NOOP probe ms | NOOP probe share | probe errors | post-run PASSIVE busy/log/checkpointed | PASSIVE ms |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 9000 | 35 | 15.434 | 1007 | 1007 | 1000 | 9000 | 9.558 | 0.368% | 0 | 0/385/385 | 0.272 |
| 256 | 1966 | 75 | 320.331 | 1087 | 1087 | 1000 | 1966 | 3.724 | 0.071% | 0 | 0/773/773 | 3.035 |
