# Encryption-at-Rest Embedded Storage Benchmark Report (Plaintext)

Captured via the repo-owned embedded-provider benchmark harness for the
2026-04-21 evidence bundle summarized in
[Encryption-at-Rest Benchmark Report](./encryption-at-rest-benchmark-report.md).

Generated with:

```bash
make bench-embedded-providers \
  REPORT=docs/plans/research/encryption-at-rest-embedded-plaintext-benchmark-report.md
```

## Methodology

- local encryption mode: `disabled`
- backend order alternates every round inside each workload and lane: round 1 runs `redb -> sqlite`, round 2 runs `sqlite -> redb`, then repeats
- steady-state warmup rounds: `2`; steady-state measured rounds: `12`
- cold-start warmup rounds: `1`; cold-start measured rounds: `10`
- cold-start read/query/journal lanes seed one canonical on-disk dataset per backend, clone that dataset before each sample, and then time only the fresh open plus first representative execution
- 95% confidence intervals use a two-sided Student-t interval on mean per-operation latency
- subscription cold-start includes fresh subscription registration/bootstrap because subscriptions are in-memory and do not survive reopen
- when encryption is enabled, benchmark-only runs write a 32-byte master key file into each temporary dataset root so cloned cold-start samples reopen through the same manifest-backed key path

## Configuration

- CRUD documents per sample: `300`
- point reads per sample: `200` over `2000` seeded documents
- indexed queries per sample: `24` over `4000` seeded documents
- journal dataset size: `1000` writes with stream page limit `256`
- subscription fan-out count: `24`
- mixed-load tenants: `4` with `120` ops per tenant per sample
- local encryption posture: `plaintext local files`
- local encryption notes: uses the current plaintext local-file path with no manifest or DEK unwrap work
- report path: `docs/plans/research/encryption-at-rest-embedded-plaintext-benchmark-report.md`

## Winner Scorecard

Winner is determined by higher median ops/s, which is equivalent here to lower
median per-op latency.

### Steady-State summary

| Workload | SQLite vs redb | Winner |
| --- | ---: | --- |
| document CRUD throughput | 23.16x | sqlite |
| point read latency | 1.01x | sqlite |
| indexed query latency | 1.12x | sqlite |
| composite indexed query latency | 1.10x | sqlite |
| durable journal stream latency | 1.03x | sqlite |
| durable journal bootstrap latency | 0.78x | redb |
| subscription fan-out latency | 5.61x | sqlite |
| concurrent multi-tenant mixed read/write load | 15.14x | sqlite |
| Total lanes won | sqlite 7, redb 1 | sqlite |

### Cold-Start summary

| Workload | SQLite vs redb | Winner |
| --- | ---: | --- |
| document CRUD throughput | 20.70x | sqlite |
| point read latency | 1.46x | sqlite |
| indexed query latency | 1.29x | sqlite |
| composite indexed query latency | 1.36x | sqlite |
| durable journal stream latency | 1.66x | sqlite |
| durable journal bootstrap latency | 1.44x | sqlite |
| subscription fan-out latency | 2.00x | sqlite |
| concurrent multi-tenant mixed read/write load | 13.13x | sqlite |
| Total lanes won | sqlite 8, redb 0 | sqlite |

### Overall total

| Scope | SQLite lanes won | redb lanes won | Overall winner |
| --- | ---: | ---: | --- |
| All measured lanes | 15 | 1 | sqlite |

## document CRUD throughput

async insert + update + delete through the Service mutation path

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 8.99 ms | 11.86 ms | 9.42 ms | 1.16 ms | 12.31% | 8.68 ms - 10.16 ms | 111.23 |
| sqlite | 12 | 388.23 us | 413.98 us | 392.62 us | 17.00 us | 4.33% | 381.81 us - 403.42 us | 2575.81 |

SQLite vs redb on the steady-state lane: `23.16x` median ops/s, `23.16x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 8.94 ms | 9.21 ms | 9.21 ms | 903.85 us | 9.81% | 8.56 ms - 9.86 ms | 111.84 |
| sqlite | 10 | 431.86 us | 457.68 us | 437.41 us | 16.59 us | 3.79% | 425.54 us - 449.27 us | 2315.55 |

SQLite vs redb on the cold-start lane: `20.70x` median ops/s, `20.70x` median per-op latency

## point read latency

batched async `get_document_async` over preseeded documents

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 786.00 ns | 839.00 ns | 795.00 ns | 28.00 ns | 3.55% | 777.00 ns - 813.00 ns | 1272264.63 |
| sqlite | 12 | 779.00 ns | 806.00 ns | 790.00 ns | 27.00 ns | 3.47% | 772.00 ns - 807.00 ns | 1283697.05 |

SQLite vs redb on the steady-state lane: `1.01x` median ops/s, `1.01x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 116.22 us | 121.88 us | 116.07 us | 4.02 us | 3.46% | 113.20 us - 118.95 us | 8604.22 |
| sqlite | 10 | 79.44 us | 83.42 us | 79.20 us | 3.66 us | 4.62% | 76.58 us - 81.81 us | 12588.12 |

SQLite vs redb on the cold-start lane: `1.46x` median ops/s, `1.46x` median per-op latency

## indexed query latency

single-field `status` equality query through planner-selected index path

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 1.25 ms | 1.26 ms | 1.24 ms | 12.53 us | 1.01% | 1.24 ms - 1.25 ms | 803.07 |
| sqlite | 12 | 1.11 ms | 1.13 ms | 1.11 ms | 18.74 us | 1.69% | 1.10 ms - 1.12 ms | 900.45 |

SQLite vs redb on the steady-state lane: `1.12x` median ops/s, `1.12x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 2.47 ms | 2.51 ms | 2.48 ms | 25.51 us | 1.03% | 2.46 ms - 2.49 ms | 405.05 |
| sqlite | 10 | 1.91 ms | 1.94 ms | 1.92 ms | 24.44 us | 1.28% | 1.90 ms - 1.93 ms | 522.62 |

SQLite vs redb on the cold-start lane: `1.29x` median ops/s, `1.29x` median per-op latency

### SQLite EXPLAIN QUERY PLAN

Captured against the seeded SQLite benchmark dataset for this workload.

```sql
SELECT id, creation_time, data_json
         FROM documents
         WHERE table_name = ?1 AND json_extract(data_json, '$."status"') = ?2
         ORDER BY id
```

```text
4 | 0 | 0 | SEARCH documents USING INDEX idx_tasks_by_status (table_name=? AND <expr>=?)
```

## composite indexed query latency

three-field composite index query with exact-prefix + range filters

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 682.83 us | 694.47 us | 682.14 us | 16.83 us | 2.47% | 671.45 us - 692.83 us | 1464.50 |
| sqlite | 12 | 619.47 us | 648.02 us | 623.57 us | 17.86 us | 2.86% | 612.22 us - 634.91 us | 1614.27 |

SQLite vs redb on the steady-state lane: `1.10x` median ops/s, `1.10x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 1.56 ms | 1.61 ms | 1.56 ms | 43.83 us | 2.80% | 1.53 ms - 1.59 ms | 640.22 |
| sqlite | 10 | 1.15 ms | 1.18 ms | 1.15 ms | 29.56 us | 2.58% | 1.13 ms - 1.17 ms | 871.36 |

SQLite vs redb on the cold-start lane: `1.36x` median ops/s, `1.36x` median per-op latency

### SQLite EXPLAIN QUERY PLAN

Captured against the seeded SQLite benchmark dataset for this workload.

```sql
SELECT id, creation_time, data_json
         FROM documents
         WHERE table_name = ?1 AND json_extract(data_json, '$."team"') = ?2 AND json_extract(data_json, '$."status"') = ?3 AND json_extract(data_json, '$."rank"') >= ?4 AND json_extract(data_json, '$."rank"') < ?5
         ORDER BY json_extract(data_json, '$."rank"'), id
```

```text
4 | 0 | 0 | SEARCH documents USING INDEX idx_tasks_by_team_status_rank (table_name=? AND <expr>=? AND <expr>=? AND <expr>>? AND <expr><?)
```

## durable journal stream latency

async `stream_durable_journal_async` from cursor 0 with a fixed page limit

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 658.94 us | 678.29 us | 652.31 us | 35.71 us | 5.47% | 629.63 us - 675.00 us | 1517.59 |
| sqlite | 12 | 636.90 us | 664.71 us | 642.51 us | 24.64 us | 3.83% | 626.86 us - 658.16 us | 1570.12 |

SQLite vs redb on the steady-state lane: `1.03x` median ops/s, `1.03x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 12.79 ms | 13.52 ms | 12.79 ms | 796.68 us | 6.23% | 12.22 ms - 13.36 ms | 78.21 |
| sqlite | 10 | 7.68 ms | 8.04 ms | 7.68 ms | 378.30 us | 4.93% | 7.41 ms - 7.95 ms | 130.19 |

SQLite vs redb on the cold-start lane: `1.66x` median ops/s, `1.66x` median per-op latency

## durable journal bootstrap latency

async `export_durable_journal_bootstrap_async` on a seeded tenant

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 543.42 us | 570.42 us | 539.20 us | 29.54 us | 5.48% | 520.44 us - 557.97 us | 1840.21 |
| sqlite | 12 | 694.06 us | 715.25 us | 698.88 us | 25.37 us | 3.63% | 682.76 us - 715.00 us | 1440.79 |

SQLite vs redb on the steady-state lane: `0.78x` median ops/s, `0.78x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 15.12 ms | 16.51 ms | 14.86 ms | 1.78 ms | 11.98% | 13.58 ms - 16.13 ms | 66.14 |
| sqlite | 10 | 10.52 ms | 10.85 ms | 10.41 ms | 1.80 ms | 17.27% | 9.13 ms - 11.70 ms | 95.03 |

SQLite vs redb on the cold-start lane: `1.44x` median ops/s, `1.44x` median per-op latency

## subscription fan-out latency

time from one matching write to receipt of updates across all active subscriptions

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 385.58 us | 451.68 us | 396.19 us | 32.12 us | 8.11% | 375.79 us - 416.60 us | 2593.52 |
| sqlite | 12 | 68.70 us | 99.01 us | 71.09 us | 22.48 us | 31.61% | 56.81 us - 85.37 us | 14555.83 |

SQLite vs redb on the steady-state lane: `5.61x` median ops/s, `5.61x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 2.08 ms | 2.21 ms | 2.11 ms | 116.82 us | 5.53% | 2.03 ms - 2.19 ms | 481.24 |
| sqlite | 10 | 1.04 ms | 1.09 ms | 1.04 ms | 49.29 us | 4.74% | 1.00 ms - 1.07 ms | 960.12 |

SQLite vs redb on the cold-start lane: `2.00x` median ops/s, `2.00x` median per-op latency

## concurrent multi-tenant mixed read/write load

concurrent per-tenant mix of point reads, indexed queries, inserts, and updates

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 2.98 ms | 3.13 ms | 2.99 ms | 79.59 us | 2.66% | 2.94 ms - 3.04 ms | 336.11 |
| sqlite | 12 | 196.56 us | 215.26 us | 195.14 us | 18.16 us | 9.31% | 183.60 us - 206.68 us | 5087.38 |

SQLite vs redb on the steady-state lane: `15.14x` median ops/s, `15.14x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 2.96 ms | 3.07 ms | 3.26 ms | 959.47 us | 29.48% | 2.57 ms - 3.94 ms | 337.47 |
| sqlite | 10 | 225.61 us | 266.84 us | 230.56 us | 29.66 us | 12.86% | 209.35 us - 251.78 us | 4432.35 |

SQLite vs redb on the cold-start lane: `13.13x` median ops/s, `13.13x` median per-op latency
