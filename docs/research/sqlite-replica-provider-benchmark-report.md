# Replica-Connected SQLite Provider Benchmark Report

Generated with:

```bash
NEOVEX_SQLITE_URL='http://127.0.0.1:18080' \
NEOVEX_SQLITE_ADMIN_URL='http://127.0.0.1:18081' \
make bench-sqlite-replica-provider REPORT=docs/research/sqlite-replica-provider-benchmark-report.md
```

## Methodology

- steady-state lane compares embedded `sqlite` against `sqlite replica` with alternating backend order
- cold-start lane compares fresh service open plus the first representative execution for embedded `sqlite` and `sqlite replica`
- replica-operational lane measures the real freshness contract shipped today: same-service barrier refresh after a remote-primary write, plus peer catch-up / delegated-write visibility through the provider poll worker
- steady-state warmup rounds: `2`; steady-state measured rounds: `10`
- cold-start warmup rounds: `1`; cold-start measured rounds: `8`
- replica-operational warmup rounds: `1`; replica-operational measured rounds: `10`
- 95% confidence intervals use a two-sided Student-t interval on mean per-operation latency

## Configuration

- CRUD documents per sample: `24`
- point reads per sample: `100` over `500` seeded documents
- indexed queries per sample: `12` over `1000` seeded documents
- mixed-load tenants: `2` with `40` ops per tenant per sample
- peer catch-up timeout: `6` with `25.00 ms` polling interval
- report path: `docs/research/sqlite-replica-provider-benchmark-report.md`

## SQLite Contrast Scorecard

Winner is determined by higher median ops/s, which is equivalent here to lower median per-op latency.

### Steady-State summary

| Workload | sqlite replica vs sqlite | Winner |
| --- | ---: | --- |
| document CRUD throughput | 0.01x | sqlite |
| point read latency | 0.97x | sqlite |
| indexed query latency | 0.98x | sqlite |
| composite indexed query latency | 1.03x | sqlite replica |
| concurrent multi-tenant mixed read/write load | 0.01x | sqlite |
| Total lanes won | sqlite replica 1, sqlite 4 | sqlite |

### Cold-Start summary

| Workload | sqlite replica vs sqlite | Winner |
| --- | ---: | --- |
| document CRUD throughput | 0.02x | sqlite |
| point read latency | 0.03x | sqlite |
| indexed query latency | 0.05x | sqlite |
| composite indexed query latency | 0.03x | sqlite |
| concurrent multi-tenant mixed read/write load | 0.02x | sqlite |
| Total lanes won | sqlite replica 0, sqlite 5 | sqlite |

### Overall total

| Scope | sqlite replica lanes won | sqlite lanes won | Overall winner |
| --- | ---: | ---: | --- |
| All contrast lanes | 1 | 9 | sqlite |

## document CRUD throughput

async insert + update + delete through the canonical service mutation path

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 388.00 us | 401.11 us | 390.36 us | 15.29 us | 3.92% | 379.42 us - 401.30 us | 2577.35 |
| sqlite replica | 10 | 38.95 ms | 48.29 ms | 39.57 ms | 8.53 ms | 21.56% | 33.47 ms - 45.68 ms | 25.67 |

sqlite replica vs sqlite on the steady-state lane: `0.01x` median ops/s, `0.01x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 555.87 us | 684.21 us | 598.21 us | 98.51 us | 16.47% | 515.84 us - 680.57 us | 1798.97 |
| sqlite replica | 8 | 22.89 ms | 23.24 ms | 23.03 ms | 518.92 us | 2.25% | 22.60 ms - 23.46 ms | 43.69 |

sqlite replica vs sqlite on the cold-start lane: `0.02x` median ops/s, `0.02x` median per-op latency

## point read latency

batched async `get_document_async` over seeded documents

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 872.00 ns | 924.00 ns | 858.00 ns | 59.00 ns | 6.90% | 816.00 ns - 900.00 ns | 1146788.99 |
| sqlite replica | 10 | 901.00 ns | 953.00 ns | 892.00 ns | 65.00 ns | 7.34% | 845.00 ns - 938.00 ns | 1109877.91 |

sqlite replica vs sqlite on the steady-state lane: `0.97x` median ops/s, `0.97x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 26.15 us | 33.99 us | 28.54 us | 4.63 us | 16.22% | 24.67 us - 32.42 us | 38246.77 |
| sqlite replica | 8 | 836.83 us | 909.54 us | 915.38 us | 228.53 us | 24.96% | 724.30 us - 1.11 ms | 1194.99 |

sqlite replica vs sqlite on the cold-start lane: `0.03x` median ops/s, `0.03x` median per-op latency

## indexed query latency

single-field `status` equality query through the planner-selected index path

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 302.70 us | 326.91 us | 306.38 us | 16.85 us | 5.50% | 294.33 us - 318.44 us | 3303.60 |
| sqlite replica | 10 | 309.88 us | 321.76 us | 307.77 us | 16.39 us | 5.32% | 296.05 us - 319.50 us | 3227.03 |

sqlite replica vs sqlite on the steady-state lane: `0.98x` median ops/s, `0.98x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 592.54 us | 630.27 us | 584.33 us | 53.38 us | 9.13% | 539.70 us - 628.96 us | 1687.66 |
| sqlite replica | 8 | 11.51 ms | 12.11 ms | 12.11 ms | 1.76 ms | 14.54% | 10.64 ms - 13.58 ms | 86.84 |

sqlite replica vs sqlite on the cold-start lane: `0.05x` median ops/s, `0.05x` median per-op latency

## composite indexed query latency

three-field composite index query with exact-prefix + range filters

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 203.25 us | 208.77 us | 198.33 us | 13.02 us | 6.56% | 189.02 us - 207.65 us | 4919.95 |
| sqlite replica | 10 | 197.20 us | 230.73 us | 210.87 us | 39.00 us | 18.50% | 182.97 us - 238.77 us | 5070.87 |

sqlite replica vs sqlite on the steady-state lane: `1.03x` median ops/s, `1.03x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 316.37 us | 333.35 us | 324.68 us | 46.95 us | 14.46% | 285.42 us - 363.93 us | 3160.83 |
| sqlite replica | 8 | 11.39 ms | 11.67 ms | 11.32 ms | 349.32 us | 3.09% | 11.03 ms - 11.61 ms | 87.83 |

sqlite replica vs sqlite on the cold-start lane: `0.03x` median ops/s, `0.03x` median per-op latency

## concurrent multi-tenant mixed read/write load

concurrent per-tenant mix of point reads, indexed queries, inserts, and updates

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 168.72 us | 176.64 us | 167.48 us | 7.55 us | 4.51% | 162.08 us - 172.88 us | 5927.01 |
| sqlite replica | 10 | 12.66 ms | 13.76 ms | 12.33 ms | 1.26 ms | 10.19% | 11.43 ms - 13.23 ms | 78.99 |

sqlite replica vs sqlite on the steady-state lane: `0.01x` median ops/s, `0.01x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 214.23 us | 224.38 us | 219.54 us | 13.56 us | 6.18% | 208.20 us - 230.89 us | 4667.79 |
| sqlite replica | 8 | 11.97 ms | 12.84 ms | 11.91 ms | 1.04 ms | 8.77% | 11.04 ms - 12.78 ms | 83.55 |

sqlite replica vs sqlite on the cold-start lane: `0.02x` median ops/s, `0.02x` median per-op latency

## Replica Freshness Drills

These lanes are the operational readiness gate for the shipped replica contract. They are intentionally replica-only because embedded SQLite has no corresponding remote-primary barrier or peer catch-up path.

| Drill | Samples | Median latency | P95 latency | Mean latency | 95% CI of mean | Result |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| same-service barrier refresh latency | 10 | 16.21 ms | 17.83 ms | 16.78 ms | 16.07 ms - 17.49 ms | pass |
| peer catch-up / delegated-write visibility latency | 10 | 537.88 ms | 563.94 ms | 543.48 ms | 532.33 ms - 554.62 ms | pass |

## Operator Assumptions

- Replica-connected SQLite tenant persistence is benchmarked with the global usage/control path still local and redb-backed.
- The live freshness contract in this first slice is provider-owned cache refresh or poll-driven catch-up, not an ad hoc direct-primary query bypass from planner code.
- The peer catch-up drill is the delegated-write readiness check for this family: one authoritative remote primary accepts the write, and another service becomes fresh only after the provider poll worker re-establishes journal/cache proof.
