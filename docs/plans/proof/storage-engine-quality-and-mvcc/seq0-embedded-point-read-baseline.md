# Embedded Storage Benchmark Report

Generated with:

```bash
make bench-embedded-providers REPORT=docs/research/sqlite-storage-benchmark-report.md
```

## Methodology

- local encryption mode: `disabled`
- backend order alternates every round inside each workload and lane: round 1 runs `redb -> sqlite`, round 2 runs `sqlite -> redb`, then repeats
- steady-state warmup rounds: `1`; steady-state measured rounds: `3`
- cold-start warmup rounds: `1`; cold-start measured rounds: `3`
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
- local encryption posture: `plaintext local files`
- local encryption notes: uses the current plaintext local-file path with no manifest or DEK unwrap work
- report path: `docs/plans/proof/storage-engine-quality-and-mvcc/seq0-embedded-point-read-baseline.md`
- workload filter: `point read latency`

## Winner Scorecard

Winner is determined by higher median ops/s, which is equivalent here to lower
median per-op latency.

### Steady-State summary

| Workload | SQLite vs redb | Winner |
| --- | ---: | --- |
| point read latency | 1.01x | sqlite |
| Total lanes won | sqlite 1, redb 0 | sqlite |

### Cold-Start summary

| Workload | SQLite vs redb | Winner |
| --- | ---: | --- |
| point read latency | 1.59x | sqlite |
| Total lanes won | sqlite 1, redb 0 | sqlite |

### Overall total

| Scope | SQLite lanes won | redb lanes won | Overall winner |
| --- | ---: | ---: | --- |
| All measured lanes | 2 | 0 | sqlite |

## point read latency

batched async `get_document_async` over preseeded documents

### Steady-State lane

reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 3 | 1.16 us | 1.16 us | 1.16 us | 19.00 ns | 1.66% | 1.11 us - 1.21 us | 860585.20 |
| sqlite | 3 | 1.15 us | 1.15 us | 1.20 us | 87.00 ns | 7.27% | 983.00 ns - 1.42 us | 868055.56 |

SQLite vs redb on the steady-state lane: `1.01x` median ops/s, `1.01x` median per-op latency

### Cold-Start lane

measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| redb | 3 | 199.32 us | 199.32 us | 194.59 us | 8.72 us | 4.48% | 172.93 us - 216.25 us | 5017.16 |
| sqlite | 3 | 125.63 us | 125.63 us | 126.48 us | 2.07 us | 1.63% | 121.35 us - 131.61 us | 7959.63 |

SQLite vs redb on the cold-start lane: `1.59x` median ops/s, `1.59x` median per-op latency

