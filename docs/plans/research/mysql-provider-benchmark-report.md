# MySQL Provider Benchmark Report

Generated with:

```bash
NEOVEX_MYSQL_URL='<connection-string>' make bench-mysql-provider REPORT=docs/plans/research/mysql-provider-benchmark-report.md
```

## Methodology

- steady-state lane compares `sqlite` against `mysql (loopback)` with alternating backend order
- cold-start lane compares `sqlite` against `mysql (loopback)` and includes fresh service open plus the first representative execution
- RTT-sensitive lane compares `mysql (loopback)` against `mysql (injected RTT)` using a local TCP proxy that delays each forwarded chunk by `5.00 ms`
- RTT-sensitive lanes use reduced representative sample sizes documented below so network sensitivity stays measurable without turning the readiness gate into an hours-long run
- steady-state warmup rounds: `2`; steady-state measured rounds: `10`
- cold-start warmup rounds: `1`; cold-start measured rounds: `8`
- RTT warmup rounds: `1`; RTT measured rounds: `4`
- 95% confidence intervals use a two-sided Student-t interval on mean per-operation latency

## Configuration

- CRUD documents per steady/cold sample: `300`; RTT sample: `8`
- point reads per steady/cold sample: `200` over `2000` seeded documents; RTT sample: `8`
- indexed queries per steady/cold sample: `24` over `4000` seeded documents; RTT sample: `4`
- journal dataset size: `1000` writes with stream page limit `256`
- subscription fan-out count: `24`
- mixed-load steady/cold sample: `4` tenants with `120` ops per tenant; RTT sample: `1` tenants with `8` ops per tenant
- standard MySQL pool config for benchmark fixtures: `min_connections=1`, `max_connections=4`
- pool-pressure observation: `min_connections=1`, `max_connections=2`, `4` concurrent workers running pure point reads while sampling active MySQL threads attributable to the benchmark provider
- background poll model assumption: no dedicated listener connection is measured; MySQL catch-up uses the provider poll worker outside the measured workload
- control-plane assumption: tenant persistence may be MySQL-backed while the global usage/control path remains local redb
- tenant-lifecycle sqlite contrast uses same-service open verification because the embedded redb control plane is single-open within one process; the MySQL lane uses a distinct peer service
- report path: `docs/plans/research/mysql-provider-benchmark-report.md`

## SQLite Contrast Scorecard

Winner is determined by higher median ops/s, which is equivalent here to lower median per-op latency.

### Steady-State summary

| Workload | MySQL vs sqlite | Winner |
| --- | ---: | --- |
| document CRUD throughput | 0.03x | sqlite |
| point read latency | 0.98x | sqlite |
| indexed query latency | 0.12x | sqlite |
| composite indexed query latency | 0.06x | sqlite |
| durable journal stream latency | 0.20x | sqlite |
| durable journal bootstrap latency | 0.16x | sqlite |
| subscription bootstrap plus catch-up latency | 0.07x | sqlite |
| subscription fan-out latency | 0.09x | sqlite |
| concurrent multi-tenant mixed read/write load | 0.06x | sqlite |
| tenant create/open/delete latency | 0.04x | sqlite |
| Total lanes won | mysql 0, sqlite 10 | sqlite |

### Cold-Start summary

| Workload | MySQL vs sqlite | Winner |
| --- | ---: | --- |
| document CRUD throughput | 0.04x | sqlite |
| point read latency | 0.01x | sqlite |
| indexed query latency | 0.17x | sqlite |
| composite indexed query latency | 0.08x | sqlite |
| durable journal stream latency | 0.24x | sqlite |
| durable journal bootstrap latency | 0.42x | sqlite |
| subscription bootstrap plus catch-up latency | 0.07x | sqlite |
| subscription fan-out latency | 0.10x | sqlite |
| concurrent multi-tenant mixed read/write load | 0.07x | sqlite |
| tenant create/open/delete latency | 0.06x | sqlite |
| Total lanes won | mysql 0, sqlite 10 | sqlite |

### Overall total

| Scope | MySQL lanes won | sqlite lanes won | Overall winner |
| --- | ---: | ---: | --- |
| Loopback contrast lanes | 0 | 20 | sqlite |

## RTT Sensitivity Scorecard

| Workload | Injected RTT vs loopback latency | Interpretation |
| --- | ---: | --- |
| document CRUD throughput | 46.46x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| point read latency | 27.11x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| indexed query latency | 23.63x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| composite indexed query latency | 23.44x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| durable journal stream latency | 23.01x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| durable journal bootstrap latency | 22.48x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| subscription bootstrap plus catch-up latency | 23.21x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| subscription fan-out latency | 24.95x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| concurrent multi-tenant mixed read/write load | 42.34x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| tenant create/open/delete latency | 8.43x | higher is worse; this is the steady-state sensitivity to non-zero RTT |

## document CRUD throughput

async insert + update + delete through the canonical service mutation path

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 373.45 us | 393.61 us | 379.63 us | 11.79 us | 3.11% | 371.20 us - 388.06 us | 2677.73 |
| mysql (loopback) | 10 | 11.00 ms | 11.82 ms | 11.22 ms | 471.96 us | 4.21% | 10.88 ms - 11.56 ms | 90.87 |

mysql (loopback) vs sqlite on the steady-state lane: `0.03x` median ops/s, `0.03x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 401.09 us | 417.65 us | 404.52 us | 13.54 us | 3.35% | 393.19 us - 415.84 us | 2493.23 |
| mysql (loopback) | 8 | 11.23 ms | 11.46 ms | 11.23 ms | 210.74 us | 1.88% | 11.06 ms - 11.41 ms | 89.06 |

mysql (loopback) vs sqlite on the cold-start lane: `0.04x` median ops/s, `0.04x` median per-op latency

### RTT-Sensitive lane

compares MySQL loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| mysql (loopback) | 4 | 11.28 ms | 11.89 ms | 11.24 ms | 785.59 us | 6.99% | 9.99 ms - 12.49 ms | 88.69 |
| mysql (injected RTT) | 4 | 523.85 ms | 526.51 ms | 524.33 ms | 3.82 ms | 0.73% | 518.25 ms - 530.41 ms | 1.91 |

mysql (injected RTT) vs mysql (loopback) on the rtt-sensitive lane: `0.02x` median ops/s, `0.02x` median per-op latency

## point read latency

batched async `get_document_async` over seeded documents

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 806.00 ns | 850.00 ns | 801.00 ns | 38.00 ns | 4.77% | 773.00 ns - 828.00 ns | 1240694.79 |
| mysql (loopback) | 10 | 824.00 ns | 879.00 ns | 841.00 ns | 73.00 ns | 8.67% | 789.00 ns - 893.00 ns | 1213592.23 |

mysql (loopback) vs sqlite on the steady-state lane: `0.98x` median ops/s, `0.98x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 49.56 us | 51.32 us | 49.77 us | 1.86 us | 3.73% | 48.22 us - 51.32 us | 20178.38 |
| mysql (loopback) | 8 | 6.10 ms | 6.13 ms | 6.08 ms | 49.89 us | 0.82% | 6.04 ms - 6.12 ms | 163.97 |

mysql (loopback) vs sqlite on the cold-start lane: `0.01x` median ops/s, `0.01x` median per-op latency

### RTT-Sensitive lane

compares MySQL loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| mysql (loopback) | 4 | 9.02 ms | 9.24 ms | 9.48 ms | 1.27 ms | 13.40% | 7.46 ms - 11.50 ms | 110.89 |
| mysql (injected RTT) | 4 | 244.51 ms | 244.66 ms | 244.42 ms | 393.39 us | 0.16% | 243.80 ms - 245.05 ms | 4.09 |

mysql (injected RTT) vs mysql (loopback) on the rtt-sensitive lane: `0.04x` median ops/s, `0.04x` median per-op latency

## indexed query latency

single-field `status` equality query through the planner-selected index path

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 1.16 ms | 1.18 ms | 1.16 ms | 31.00 us | 2.68% | 1.14 ms - 1.18 ms | 862.38 |
| mysql (loopback) | 10 | 9.86 ms | 10.30 ms | 9.97 ms | 378.32 us | 3.79% | 9.70 ms - 10.24 ms | 101.41 |

mysql (loopback) vs sqlite on the steady-state lane: `0.12x` median ops/s, `0.12x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 1.70 ms | 1.75 ms | 1.70 ms | 38.83 us | 2.28% | 1.67 ms - 1.74 ms | 586.92 |
| mysql (loopback) | 8 | 10.06 ms | 10.21 ms | 10.11 ms | 192.27 us | 1.90% | 9.95 ms - 10.27 ms | 99.43 |

mysql (loopback) vs sqlite on the cold-start lane: `0.17x` median ops/s, `0.17x` median per-op latency

### RTT-Sensitive lane

compares MySQL loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| mysql (loopback) | 4 | 15.03 ms | 16.31 ms | 15.33 ms | 1.89 ms | 12.32% | 12.32 ms - 18.33 ms | 66.52 |
| mysql (injected RTT) | 4 | 355.22 ms | 355.79 ms | 358.02 ms | 8.39 ms | 2.34% | 344.67 ms - 371.37 ms | 2.82 |

mysql (injected RTT) vs mysql (loopback) on the rtt-sensitive lane: `0.04x` median ops/s, `0.04x` median per-op latency

## composite indexed query latency

three-field composite index query with exact-prefix + range filters

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 660.91 us | 674.44 us | 658.40 us | 18.03 us | 2.74% | 645.50 us - 671.30 us | 1513.06 |
| mysql (loopback) | 10 | 11.42 ms | 11.63 ms | 11.31 ms | 340.01 us | 3.01% | 11.07 ms - 11.55 ms | 87.59 |

mysql (loopback) vs sqlite on the steady-state lane: `0.06x` median ops/s, `0.06x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 975.34 us | 997.63 us | 980.48 us | 19.50 us | 1.99% | 964.17 us - 996.79 us | 1025.28 |
| mysql (loopback) | 8 | 11.63 ms | 12.00 ms | 11.89 ms | 703.91 us | 5.92% | 11.30 ms - 12.48 ms | 85.99 |

mysql (loopback) vs sqlite on the cold-start lane: `0.08x` median ops/s, `0.08x` median per-op latency

### RTT-Sensitive lane

compares MySQL loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| mysql (loopback) | 4 | 16.83 ms | 17.49 ms | 17.12 ms | 2.68 ms | 15.67% | 12.85 ms - 21.38 ms | 59.43 |
| mysql (injected RTT) | 4 | 394.39 ms | 398.96 ms | 392.21 ms | 10.22 ms | 2.61% | 375.94 ms - 408.47 ms | 2.54 |

mysql (injected RTT) vs mysql (loopback) on the rtt-sensitive lane: `0.04x` median ops/s, `0.04x` median per-op latency

## durable journal stream latency

async `stream_durable_journal_async` from cursor 0 with a fixed page limit

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 725.73 us | 757.50 us | 719.39 us | 36.86 us | 5.12% | 693.02 us - 745.76 us | 1377.93 |
| mysql (loopback) | 10 | 3.70 ms | 4.49 ms | 3.64 ms | 814.01 us | 22.35% | 3.06 ms - 4.22 ms | 270.55 |

mysql (loopback) vs sqlite on the steady-state lane: `0.20x` median ops/s, `0.20x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 2.33 ms | 3.00 ms | 2.46 ms | 551.05 us | 22.37% | 2.00 ms - 2.92 ms | 429.72 |
| mysql (loopback) | 8 | 9.67 ms | 10.91 ms | 9.90 ms | 1.20 ms | 12.13% | 8.89 ms - 10.90 ms | 103.40 |

mysql (loopback) vs sqlite on the cold-start lane: `0.24x` median ops/s, `0.24x` median per-op latency

### RTT-Sensitive lane

compares MySQL loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| mysql (loopback) | 4 | 13.77 ms | 14.37 ms | 13.78 ms | 739.04 us | 5.36% | 12.60 ms - 14.95 ms | 72.64 |
| mysql (injected RTT) | 4 | 316.73 ms | 318.34 ms | 315.77 ms | 4.58 ms | 1.45% | 308.48 ms - 323.06 ms | 3.16 |

mysql (injected RTT) vs mysql (loopback) on the rtt-sensitive lane: `0.04x` median ops/s, `0.04x` median per-op latency

## durable journal bootstrap latency

async `export_durable_journal_bootstrap_async` on a seeded tenant

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 796.44 us | 842.33 us | 803.52 us | 52.78 us | 6.57% | 765.77 us - 841.28 us | 1255.59 |
| mysql (loopback) | 10 | 5.14 ms | 6.00 ms | 5.02 ms | 894.78 us | 17.82% | 4.38 ms - 5.66 ms | 194.65 |

mysql (loopback) vs sqlite on the steady-state lane: `0.16x` median ops/s, `0.16x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 5.46 ms | 5.84 ms | 5.41 ms | 479.73 us | 8.86% | 5.01 ms - 5.82 ms | 183.07 |
| mysql (loopback) | 8 | 13.15 ms | 13.67 ms | 12.84 ms | 1.35 ms | 10.51% | 11.71 ms - 13.96 ms | 76.05 |

mysql (loopback) vs sqlite on the cold-start lane: `0.42x` median ops/s, `0.42x` median per-op latency

### RTT-Sensitive lane

compares MySQL loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| mysql (loopback) | 4 | 17.08 ms | 17.80 ms | 17.26 ms | 1.51 ms | 8.74% | 14.86 ms - 19.65 ms | 58.56 |
| mysql (injected RTT) | 4 | 383.82 ms | 391.14 ms | 388.10 ms | 16.88 ms | 4.35% | 361.25 ms - 414.95 ms | 2.61 |

mysql (injected RTT) vs mysql (loopback) on the rtt-sensitive lane: `0.04x` median ops/s, `0.04x` median per-op latency

## subscription bootstrap plus catch-up latency

single subscription bootstrap followed by one durable matching update

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 826.48 us | 1.03 ms | 849.48 us | 144.84 us | 17.05% | 745.87 us - 953.08 us | 1209.95 |
| mysql (loopback) | 10 | 11.53 ms | 13.04 ms | 11.28 ms | 1.57 ms | 13.91% | 10.16 ms - 12.41 ms | 86.72 |

mysql (loopback) vs sqlite on the steady-state lane: `0.07x` median ops/s, `0.07x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 1.17 ms | 1.27 ms | 1.14 ms | 149.32 us | 13.10% | 1.02 ms - 1.27 ms | 855.92 |
| mysql (loopback) | 8 | 15.76 ms | 16.53 ms | 15.43 ms | 1.13 ms | 7.33% | 14.49 ms - 16.38 ms | 63.45 |

mysql (loopback) vs sqlite on the cold-start lane: `0.07x` median ops/s, `0.07x` median per-op latency

### RTT-Sensitive lane

compares MySQL loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| mysql (loopback) | 4 | 18.89 ms | 22.27 ms | 18.88 ms | 3.93 ms | 20.82% | 12.62 ms - 25.13 ms | 52.94 |
| mysql (injected RTT) | 4 | 438.36 ms | 443.24 ms | 441.13 ms | 10.43 ms | 2.36% | 424.54 ms - 457.73 ms | 2.28 |

mysql (injected RTT) vs mysql (loopback) on the rtt-sensitive lane: `0.04x` median ops/s, `0.04x` median per-op latency

## subscription fan-out latency

time from one durable matching write to delivery across all active subscriptions

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 49.45 us | 63.07 us | 52.76 us | 14.89 us | 28.22% | 42.11 us - 63.41 us | 20221.63 |
| mysql (loopback) | 10 | 547.14 us | 789.74 us | 667.15 us | 388.15 us | 58.18% | 389.50 us - 944.80 us | 1827.70 |

mysql (loopback) vs sqlite on the steady-state lane: `0.09x` median ops/s, `0.09x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 46.84 us | 48.57 us | 47.00 us | 7.00 us | 14.89% | 41.15 us - 52.85 us | 21351.55 |
| mysql (loopback) | 8 | 483.85 us | 530.48 us | 470.32 us | 72.33 us | 15.38% | 409.84 us - 530.79 us | 2066.76 |

mysql (loopback) vs sqlite on the cold-start lane: `0.10x` median ops/s, `0.10x` median per-op latency

### RTT-Sensitive lane

compares MySQL loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| mysql (loopback) | 4 | 730.18 us | 830.53 us | 746.69 us | 149.29 us | 19.99% | 509.17 us - 984.22 us | 1369.52 |
| mysql (injected RTT) | 4 | 18.22 ms | 18.51 ms | 18.05 ms | 765.64 us | 4.24% | 16.84 ms - 19.27 ms | 54.90 |

mysql (injected RTT) vs mysql (loopback) on the rtt-sensitive lane: `0.04x` median ops/s, `0.04x` median per-op latency

## concurrent multi-tenant mixed read/write load

concurrent per-tenant mix of point reads, indexed queries, inserts, and updates

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 179.90 us | 194.15 us | 181.28 us | 9.07 us | 5.00% | 174.80 us - 187.77 us | 5558.80 |
| mysql (loopback) | 10 | 2.80 ms | 2.85 ms | 2.80 ms | 54.97 us | 1.96% | 2.76 ms - 2.84 ms | 357.34 |

mysql (loopback) vs sqlite on the steady-state lane: `0.06x` median ops/s, `0.06x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 210.09 us | 242.12 us | 216.39 us | 18.43 us | 8.52% | 200.98 us - 231.80 us | 4759.77 |
| mysql (loopback) | 8 | 3.09 ms | 3.15 ms | 3.09 ms | 70.42 us | 2.28% | 3.03 ms - 3.15 ms | 323.69 |

mysql (loopback) vs sqlite on the cold-start lane: `0.07x` median ops/s, `0.07x` median per-op latency

### RTT-Sensitive lane

compares MySQL loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| mysql (loopback) | 4 | 7.68 ms | 7.90 ms | 8.18 ms | 1.31 ms | 16.02% | 6.10 ms - 10.27 ms | 130.18 |
| mysql (injected RTT) | 4 | 325.27 ms | 325.59 ms | 324.65 ms | 3.10 ms | 0.95% | 319.72 ms - 329.58 ms | 3.07 |

mysql (injected RTT) vs mysql (loopback) on the rtt-sensitive lane: `0.02x` median ops/s, `0.02x` median per-op latency

## tenant create/open/delete latency

create a tenant, verify it opens from a peer service when the topology allows it, then delete it cleanly

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 899.77 us | 1.48 ms | 1.01 ms | 263.51 us | 26.18% | 817.91 us - 1.19 ms | 1111.39 |
| mysql (loopback) | 10 | 22.16 ms | 22.57 ms | 22.44 ms | 1.53 ms | 6.81% | 21.34 ms - 23.53 ms | 45.12 |

mysql (loopback) vs sqlite on the steady-state lane: `0.04x` median ops/s, `0.04x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 1.37 ms | 1.56 ms | 1.37 ms | 206.95 us | 15.08% | 1.20 ms - 1.55 ms | 727.56 |
| mysql (loopback) | 8 | 23.41 ms | 26.69 ms | 25.88 ms | 5.85 ms | 22.60% | 20.99 ms - 30.77 ms | 42.72 |

mysql (loopback) vs sqlite on the cold-start lane: `0.06x` median ops/s, `0.06x` median per-op latency

### RTT-Sensitive lane

compares MySQL loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| mysql (loopback) | 4 | 27.01 ms | 27.05 ms | 25.90 ms | 3.73 ms | 14.40% | 19.96 ms - 31.83 ms | 37.03 |
| mysql (injected RTT) | 4 | 227.80 ms | 230.81 ms | 225.78 ms | 9.82 ms | 4.35% | 210.16 ms - 241.40 ms | 4.39 |

mysql (injected RTT) vs mysql (loopback) on the rtt-sensitive lane: `0.12x` median ops/s, `0.12x` median per-op latency

## Pool Pressure Observation

This observation intentionally constrains the MySQL provider pool to expose head-of-line behavior and verify that active pooled backends remain bounded.

| Samples | Max active MySQL threads observed | Configured max connections | Concurrent workers | Median sample latency | P95 sample latency | Mean sample latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8 | 2 | 2 | 4 | 843.50 us | 1.51 ms | 333.14 ms |

Relative to the unconstrained steady-state MySQL mixed-load lane, the bounded-pool observation shows `0.30x` higher median end-to-end sample latency while active provider-attributed MySQL threads remained capped at `2`.

## Operator Assumptions

- MySQL tenant persistence is benchmarked with the global usage/control path still local and redb-backed.
- The service-path benchmark includes provider-owned pooling, typed construction, scheduler/journal semantics, and the provider background poll wake path; authoritative recovery still comes from durable journal progress rather than from wake signals.
- Companion operational drills for poll catch-up, restart recovery, transient backend failure, unloaded-tenant scheduler wake, and tenant cleanup are covered by focused storage/engine verification and recorded in `/Users/jack/src/github.com/agentstation/neovex/docs/plans/archive/mysql-storage-provider-plan.md`.
