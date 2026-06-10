# SQLite Storage Backend Benchmark Report

Generated with:

```bash
make bench-embedded-providers REPORT=docs/plans/research/sqlite-storage-benchmark-report.md
```

## Methodology

- provider order alternates every round inside each workload and lane: round 1 runs `redb -> sqlite`, round 2 runs `sqlite -> redb`, then repeats
- steady-state warmup rounds: `2`; steady-state measured rounds: `12`
- cold-start warmup rounds: `1`; cold-start measured rounds: `10`
- cold-start read/query/journal lanes seed one canonical on-disk dataset per provider, clone that dataset before each sample, and then time only the fresh open plus first representative execution
- 95% confidence intervals use a two-sided Student-t interval on mean per-operation latency
- subscription cold-start includes fresh subscription registration/bootstrap because subscriptions are in-memory and do not survive reopen

## Configuration

- CRUD documents per sample: `300`
- point reads per sample: `200` over `2000` seeded documents
- indexed queries per sample: `24` over `4000` seeded documents
- journal dataset size: `1000` writes with stream page limit `256`
- subscription fan-out count: `24`
- mixed-load tenants: `4` with `120` ops per tenant per sample
- report path: `docs/plans/research/sqlite-storage-benchmark-report.md`

## Winner Scorecard

Winner is determined by higher median ops/s, which is equivalent here to lower
median per-op latency.

### Steady-State summary

| Workload | SQLite vs redb | Winner |
| --- | ---: | --- |
| document CRUD throughput | 24.50x | sqlite |
| point read latency | 1.00x | sqlite |
| indexed query latency | 1.12x | sqlite |
| composite indexed query latency | 1.09x | sqlite |
| durable journal stream latency | 0.98x | redb |
| durable journal bootstrap latency | 0.74x | redb |
| subscription fan-out latency | 6.64x | sqlite |
| concurrent multi-tenant mixed read/write load | 16.53x | sqlite |
| Total lanes won | sqlite 6, redb 2 | sqlite |

### Cold-Start summary

| Workload | SQLite vs redb | Winner |
| --- | ---: | --- |
| document CRUD throughput | 21.44x | sqlite |
| point read latency | 1.41x | sqlite |
| indexed query latency | 1.41x | sqlite |
| composite indexed query latency | 1.59x | sqlite |
| durable journal stream latency | 1.60x | sqlite |
| durable journal bootstrap latency | 1.52x | sqlite |
| subscription fan-out latency | 2.17x | sqlite |
| concurrent multi-tenant mixed read/write load | 14.83x | sqlite |
| Total lanes won | sqlite 8, redb 0 | sqlite |

### Overall total

| Scope | SQLite lanes won | redb lanes won | Overall winner |
| --- | ---: | ---: | --- |
| All measured lanes | 14 | 2 | sqlite |

## document CRUD throughput

async insert + update + delete through the Service mutation path

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 9.20 ms | 9.68 ms | 9.25 ms | 310.06 us | 3.35% | 9.05 ms - 9.45 ms | 108.64 |
| sqlite | 12 | 375.76 us | 408.08 us | 385.66 us | 32.71 us | 8.48% | 364.88 us - 406.45 us | 2661.28 |

SQLite vs redb on the steady-state lane: `24.50x` median ops/s, `24.50x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 9.07 ms | 9.15 ms | 9.04 ms | 111.49 us | 1.23% | 8.96 ms - 9.12 ms | 110.26 |
| sqlite | 10 | 423.00 us | 440.03 us | 422.76 us | 24.65 us | 5.83% | 405.13 us - 440.39 us | 2364.09 |

SQLite vs redb on the cold-start lane: `21.44x` median ops/s, `21.44x` median per-op latency

## point read latency

batched async `get_document_async` over preseeded documents

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 795.00 ns | 814.00 ns | 795.00 ns | 14.00 ns | 1.81% | 786.00 ns - 804.00 ns | 1257861.64 |
| sqlite | 12 | 794.00 ns | 843.00 ns | 809.00 ns | 38.00 ns | 4.73% | 785.00 ns - 834.00 ns | 1259445.84 |

SQLite vs redb on the steady-state lane: `1.00x` median ops/s, `1.00x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 107.26 us | 115.86 us | 107.14 us | 6.50 us | 6.07% | 102.48 us - 111.79 us | 9323.14 |
| sqlite | 10 | 75.84 us | 80.98 us | 78.15 us | 7.44 us | 9.52% | 72.83 us - 83.47 us | 13185.83 |

SQLite vs redb on the cold-start lane: `1.41x` median ops/s, `1.41x` median per-op latency

## indexed query latency

single-field `status` equality query through planner-selected index path

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 1.25 ms | 1.27 ms | 1.24 ms | 18.46 us | 1.48% | 1.23 ms - 1.26 ms | 802.69 |
| sqlite | 12 | 1.11 ms | 1.15 ms | 1.12 ms | 19.87 us | 1.78% | 1.11 ms - 1.13 ms | 899.15 |

SQLite vs redb on the steady-state lane: `1.12x` median ops/s, `1.12x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 2.49 ms | 2.53 ms | 2.48 ms | 58.85 us | 2.38% | 2.43 ms - 2.52 ms | 401.53 |
| sqlite | 10 | 1.76 ms | 1.83 ms | 1.77 ms | 45.37 us | 2.57% | 1.73 ms - 1.80 ms | 567.30 |

SQLite vs redb on the cold-start lane: `1.41x` median ops/s, `1.41x` median per-op latency

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
| redb | 12 | 682.02 us | 694.21 us | 720.89 us | 155.70 us | 21.60% | 621.97 us - 819.82 us | 1466.23 |
| sqlite | 12 | 625.72 us | 640.50 us | 628.25 us | 9.12 us | 1.45% | 622.46 us - 634.04 us | 1598.16 |

SQLite vs redb on the steady-state lane: `1.09x` median ops/s, `1.09x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 1.75 ms | 1.80 ms | 1.75 ms | 34.70 us | 1.98% | 1.73 ms - 1.78 ms | 573.03 |
| sqlite | 10 | 1.10 ms | 1.20 ms | 1.10 ms | 90.09 us | 8.22% | 1.03 ms - 1.16 ms | 912.70 |

SQLite vs redb on the cold-start lane: `1.59x` median ops/s, `1.59x` median per-op latency

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
| redb | 12 | 619.62 us | 662.42 us | 630.01 us | 24.04 us | 3.82% | 614.74 us - 645.29 us | 1613.88 |
| sqlite | 12 | 634.81 us | 652.08 us | 645.27 us | 29.54 us | 4.58% | 626.50 us - 664.04 us | 1575.27 |

SQLite vs redb on the steady-state lane: `0.98x` median ops/s, `0.98x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 15.24 ms | 16.81 ms | 15.07 ms | 1.49 ms | 9.90% | 14.00 ms - 16.14 ms | 65.60 |
| sqlite | 10 | 9.54 ms | 10.66 ms | 9.70 ms | 1.12 ms | 11.51% | 8.90 ms - 10.50 ms | 104.82 |

SQLite vs redb on the cold-start lane: `1.60x` median ops/s, `1.60x` median per-op latency

## durable journal bootstrap latency

async `export_durable_journal_bootstrap_async` on a seeded tenant

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 525.54 us | 544.75 us | 527.43 us | 12.75 us | 2.42% | 519.33 us - 535.53 us | 1902.80 |
| sqlite | 12 | 710.38 us | 773.67 us | 719.62 us | 38.63 us | 5.37% | 695.08 us - 744.17 us | 1407.71 |

SQLite vs redb on the steady-state lane: `0.74x` median ops/s, `0.74x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 14.20 ms | 15.54 ms | 14.29 ms | 1.29 ms | 9.00% | 13.37 ms - 15.21 ms | 70.42 |
| sqlite | 10 | 9.35 ms | 10.49 ms | 9.51 ms | 1.09 ms | 11.48% | 8.73 ms - 10.29 ms | 106.94 |

SQLite vs redb on the cold-start lane: `1.52x` median ops/s, `1.52x` median per-op latency

## subscription fan-out latency

time from one matching write to receipt of updates across all active subscriptions

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 410.31 us | 434.25 us | 402.75 us | 27.83 us | 6.91% | 385.07 us - 420.44 us | 2437.21 |
| sqlite | 12 | 61.75 us | 73.31 us | 61.20 us | 11.40 us | 18.63% | 53.95 us - 68.44 us | 16195.12 |

SQLite vs redb on the steady-state lane: `6.64x` median ops/s, `6.64x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 2.09 ms | 2.21 ms | 2.07 ms | 136.81 us | 6.61% | 1.97 ms - 2.17 ms | 478.52 |
| sqlite | 10 | 964.90 us | 1.03 ms | 935.53 us | 116.46 us | 12.45% | 852.22 us - 1.02 ms | 1036.38 |

SQLite vs redb on the cold-start lane: `2.17x` median ops/s, `2.17x` median per-op latency

## concurrent multi-tenant mixed read/write load

concurrent per-tenant mix of point reads, indexed queries, inserts, and updates

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 12 | 3.13 ms | 3.28 ms | 3.15 ms | 111.70 us | 3.55% | 3.08 ms - 3.22 ms | 319.82 |
| sqlite | 12 | 189.13 us | 213.58 us | 191.78 us | 16.82 us | 8.77% | 181.10 us - 202.47 us | 5287.34 |

SQLite vs redb on the steady-state lane: `16.53x` median ops/s, `16.53x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 10 | 3.36 ms | 3.62 ms | 3.39 ms | 148.38 us | 4.37% | 3.29 ms - 3.50 ms | 297.78 |
| sqlite | 10 | 226.52 us | 243.47 us | 225.91 us | 18.33 us | 8.11% | 212.80 us - 239.03 us | 4414.66 |

SQLite vs redb on the cold-start lane: `14.83x` median ops/s, `14.83x` median per-op latency
