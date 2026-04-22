# Encryption-at-Rest Embedded Storage Benchmark Report (Manifest-Backed Encryption)

Captured via the repo-owned embedded-provider benchmark harness for the
2026-04-21 evidence bundle summarized in
[Encryption-at-Rest Benchmark Report](./encryption-at-rest-benchmark-report.md).

Generated with:

```bash
make bench-embedded-providers \
  ENCRYPTION=temp-master-key-file \
  REPORT=docs/research/encryption-at-rest-embedded-encrypted-benchmark-report.md
```

## Methodology

- local encryption mode: `temp-master-key-file`
- backend order alternates every round inside each workload and lane: round 1 runs `redb -> sqlite`, round 2 runs `sqlite -> redb`, then repeats
- steady-state warmup rounds: `2`; steady-state measured rounds: `12`
- cold-start warmup rounds: `1`; cold-start measured rounds: `10`
- cold-start read/query/journal lanes seed one canonical on-disk dataset per backend, clone that dataset before each sample, and then time only the fresh open plus first representative execution
- 95% confidence intervals use a two-sided Student-t interval on mean per-operation latency
- subscription cold-start includes fresh subscription registration/bootstrap because subscriptions are in-memory and do not survive reopen
- encryption-enabled runs use one benchmark-only 32-byte master key file per benchmark process so cloned cold-start samples reopen through the same manifest-backed key path

## Configuration

- CRUD documents per sample: `300`
- point reads per sample: `200` over `2000` seeded documents
- indexed queries per sample: `24` over `4000` seeded documents
- journal dataset size: `1000` writes with stream page limit `256`
- subscription fan-out count: `24`
- mixed-load tenants: `4` with `120` ops per tenant per sample
- local encryption posture: `manifest-backed local encryption`
- local encryption notes: enables the real startup path with a benchmark-only master key file so every local database still uses a manifest-backed random DEK
- report path: `docs/research/encryption-at-rest-embedded-encrypted-benchmark-report.md`

## Winner Scorecard

Winner is determined by higher median ops/s, which is equivalent here to lower
median per-op latency.

### Steady-State summary

| Workload | SQLite vs redb | Winner |
| --- | ---: | --- |
| document CRUD throughput | 16.69x | sqlite |
| point read latency | 0.99x | redb |
| indexed query latency | 1.09x | sqlite |
| composite indexed query latency | 1.08x | sqlite |
| durable journal stream latency | 0.97x | redb |
| durable journal bootstrap latency | 0.79x | redb |
| subscription fan-out latency | 6.40x | sqlite |
| concurrent multi-tenant mixed read/write load | 18.70x | sqlite |
| Total lanes won | sqlite 5, redb 3 | sqlite |

### Cold-Start summary

| Workload | SQLite vs redb | Winner |
| --- | ---: | --- |
| document CRUD throughput | 15.14x | sqlite |
| point read latency | 1.79x | sqlite |
| indexed query latency | 1.54x | sqlite |
| composite indexed query latency | 1.64x | sqlite |
| durable journal stream latency | 1.80x | sqlite |
| durable journal bootstrap latency | 1.73x | sqlite |
| subscription fan-out latency | 1.85x | sqlite |
| concurrent multi-tenant mixed read/write load | 18.23x | sqlite |
| Total lanes won | sqlite 8, redb 0 | sqlite |

### Overall total

| Scope | SQLite lanes won | redb lanes won | Overall winner |
| --- | ---: | ---: | --- |
| All measured lanes | 13 | 3 | sqlite |

## document CRUD throughput

async insert + update + delete through the Service mutation path

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 10.23 ms | 10.30 ms | 10.41 ms | 757.68 us | 7.28% | 9.93 ms - 10.90 ms | 97.78 |
| sqlite | 12 | 612.81 us | 628.71 us | 609.46 us | 16.98 us | 2.79% | 598.67 us - 620.25 us | 1631.83 |

SQLite vs redb on the steady-state lane: `16.69x` median ops/s, `16.69x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 10.17 ms | 10.23 ms | 10.16 ms | 54.39 us | 0.54% | 10.12 ms - 10.20 ms | 98.32 |
| sqlite | 10 | 671.58 us | 691.52 us | 674.84 us | 9.90 us | 1.47% | 667.75 us - 681.92 us | 1489.02 |

SQLite vs redb on the cold-start lane: `15.14x` median ops/s, `15.14x` median per-op latency

## point read latency

batched async `get_document_async` over preseeded documents

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 778.00 ns | 785.00 ns | 778.00 ns | 10.00 ns | 1.23% | 772.00 ns - 784.00 ns | 1285347.04 |
| sqlite | 12 | 785.00 ns | 812.00 ns | 788.00 ns | 18.00 ns | 2.30% | 777.00 ns - 800.00 ns | 1273885.35 |

SQLite vs redb on the steady-state lane: `0.99x` median ops/s, `0.99x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 161.27 us | 165.30 us | 161.44 us | 3.31 us | 2.05% | 159.08 us - 163.81 us | 6200.97 |
| sqlite | 10 | 90.32 us | 93.94 us | 94.68 us | 13.93 us | 14.72% | 84.72 us - 104.65 us | 11071.62 |

SQLite vs redb on the cold-start lane: `1.79x` median ops/s, `1.79x` median per-op latency

## indexed query latency

single-field `status` equality query through planner-selected index path

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 1.24 ms | 1.27 ms | 1.25 ms | 18.12 us | 1.46% | 1.23 ms - 1.26 ms | 804.73 |
| sqlite | 12 | 1.14 ms | 1.15 ms | 1.14 ms | 8.13 us | 0.72% | 1.13 ms - 1.14 ms | 877.52 |

SQLite vs redb on the steady-state lane: `1.09x` median ops/s, `1.09x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 7.44 ms | 7.79 ms | 7.50 ms | 297.64 us | 3.97% | 7.29 ms - 7.71 ms | 134.46 |
| sqlite | 10 | 4.82 ms | 5.10 ms | 4.88 ms | 220.11 us | 4.51% | 4.72 ms - 5.04 ms | 207.26 |

SQLite vs redb on the cold-start lane: `1.54x` median ops/s, `1.54x` median per-op latency

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
| redb | 12 | 689.38 us | 709.32 us | 745.03 us | 195.80 us | 26.28% | 620.62 us - 869.43 us | 1450.59 |
| sqlite | 12 | 636.71 us | 656.46 us | 645.74 us | 35.73 us | 5.53% | 623.04 us - 668.44 us | 1570.58 |

SQLite vs redb on the steady-state lane: `1.08x` median ops/s, `1.08x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 2.20 ms | 2.30 ms | 2.22 ms | 64.33 us | 2.89% | 2.18 ms - 2.27 ms | 454.68 |
| sqlite | 10 | 1.34 ms | 1.37 ms | 1.35 ms | 22.20 us | 1.64% | 1.34 ms - 1.37 ms | 743.51 |

SQLite vs redb on the cold-start lane: `1.64x` median ops/s, `1.64x` median per-op latency

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
| redb | 12 | 651.17 us | 680.33 us | 659.65 us | 23.40 us | 3.55% | 644.79 us - 674.52 us | 1535.71 |
| sqlite | 12 | 670.08 us | 720.88 us | 699.05 us | 85.06 us | 12.17% | 645.00 us - 753.09 us | 1492.35 |

SQLite vs redb on the steady-state lane: `0.97x` median ops/s, `0.97x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 23.45 ms | 24.97 ms | 23.66 ms | 1.15 ms | 4.86% | 22.84 ms - 24.49 ms | 42.64 |
| sqlite | 10 | 13.02 ms | 14.09 ms | 13.18 ms | 812.79 us | 6.17% | 12.60 ms - 13.77 ms | 76.82 |

SQLite vs redb on the cold-start lane: `1.80x` median ops/s, `1.80x` median per-op latency

## durable journal bootstrap latency

async `export_durable_journal_bootstrap_async` on a seeded tenant

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 542.48 us | 590.17 us | 549.67 us | 39.06 us | 7.11% | 524.85 us - 574.49 us | 1843.39 |
| sqlite | 12 | 686.29 us | 744.08 us | 694.92 us | 31.45 us | 4.53% | 674.94 us - 714.90 us | 1457.11 |

SQLite vs redb on the steady-state lane: `0.79x` median ops/s, `0.79x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 26.09 ms | 28.38 ms | 26.39 ms | 2.39 ms | 9.05% | 24.69 ms - 28.10 ms | 38.33 |
| sqlite | 10 | 15.12 ms | 15.56 ms | 14.60 ms | 1.23 ms | 8.41% | 13.73 ms - 15.48 ms | 66.15 |

SQLite vs redb on the cold-start lane: `1.73x` median ops/s, `1.73x` median per-op latency

## subscription fan-out latency

time from one matching write to receipt of updates across all active subscriptions

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 424.96 us | 505.77 us | 441.82 us | 41.06 us | 9.29% | 415.73 us - 467.91 us | 2353.17 |
| sqlite | 12 | 66.41 us | 75.10 us | 63.31 us | 9.06 us | 14.31% | 57.55 us - 69.07 us | 15058.88 |

SQLite vs redb on the steady-state lane: `6.40x` median ops/s, `6.40x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 5.15 ms | 5.25 ms | 5.17 ms | 79.29 us | 1.53% | 5.11 ms - 5.23 ms | 194.03 |
| sqlite | 10 | 2.79 ms | 2.90 ms | 2.80 ms | 74.03 us | 2.64% | 2.75 ms - 2.85 ms | 358.00 |

SQLite vs redb on the cold-start lane: `1.85x` median ops/s, `1.85x` median per-op latency

## concurrent multi-tenant mixed read/write load

concurrent per-tenant mix of point reads, indexed queries, inserts, and updates

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 4.14 ms | 4.31 ms | 4.16 ms | 122.47 us | 2.95% | 4.08 ms - 4.23 ms | 241.47 |
| sqlite | 12 | 221.44 us | 243.35 us | 221.22 us | 25.29 us | 11.43% | 205.15 us - 237.29 us | 4515.98 |

SQLite vs redb on the steady-state lane: `18.70x` median ops/s, `18.70x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 4.40 ms | 4.54 ms | 4.39 ms | 145.53 us | 3.32% | 4.28 ms - 4.49 ms | 227.11 |
| sqlite | 10 | 241.50 us | 244.69 us | 251.39 us | 31.73 us | 12.62% | 228.69 us - 274.08 us | 4140.77 |

SQLite vs redb on the cold-start lane: `18.23x` median ops/s, `18.23x` median per-op latency
