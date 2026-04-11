# Postgres Provider Benchmark Report

Generated with:

```bash
NEOVEX_BENCH_POSTGRES_URL='<connection-string>' make bench-postgres-provider REPORT=docs/research/postgres-provider-benchmark-report.md
```

## Methodology

- steady-state lane compares `sqlite` against `postgres (loopback)` with alternating backend order
- cold-start lane compares `sqlite` against `postgres (loopback)` and includes fresh service open plus the first representative execution
- RTT-sensitive lane compares `postgres (loopback)` against `postgres (injected RTT)` using a local TCP proxy that delays each forwarded chunk by `5.00 ms`
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
- standard Postgres pool config for benchmark fixtures: `min_connections=1`, `max_connections=4`
- pool-pressure observation: `min_connections=1`, `max_connections=2`, `4` concurrent workers running pure point reads
- notification model assumption: one additional Postgres listener connection per live service process, outside the measured pool
- control-plane assumption: tenant persistence may be Postgres-backed while the global usage/control path remains local redb
- tenant-lifecycle sqlite contrast uses same-service open verification because the embedded redb control plane is single-open within one process; the Postgres lane uses a distinct peer service
- report path: `docs/research/postgres-provider-benchmark-report.md`

## SQLite Contrast Scorecard

Winner is determined by higher median ops/s, which is equivalent here to lower median per-op latency.

### Steady-State summary

| Workload | Postgres vs sqlite | Winner |
| --- | ---: | --- |
| document CRUD throughput | 0.12x | sqlite |
| point read latency | 0.97x | sqlite |
| indexed query latency | 0.07x | sqlite |
| composite indexed query latency | 0.03x | sqlite |
| durable journal stream latency | 0.64x | sqlite |
| durable journal bootstrap latency | 0.14x | sqlite |
| subscription bootstrap plus catch-up latency | 0.34x | sqlite |
| subscription fan-out latency | 0.45x | sqlite |
| concurrent multi-tenant mixed read/write load | 0.22x | sqlite |
| tenant create/open/delete latency | 0.25x | sqlite |
| Total lanes won | postgres 0, sqlite 10 | sqlite |

### Cold-Start summary

| Workload | Postgres vs sqlite | Winner |
| --- | ---: | --- |
| document CRUD throughput | 0.12x | sqlite |
| point read latency | 0.00x | sqlite |
| indexed query latency | 0.10x | sqlite |
| composite indexed query latency | 0.05x | sqlite |
| durable journal stream latency | 0.26x | sqlite |
| durable journal bootstrap latency | 0.41x | sqlite |
| subscription bootstrap plus catch-up latency | 0.11x | sqlite |
| subscription fan-out latency | 0.25x | sqlite |
| concurrent multi-tenant mixed read/write load | 0.24x | sqlite |
| tenant create/open/delete latency | 0.17x | sqlite |
| Total lanes won | postgres 0, sqlite 10 | sqlite |

### Overall total

| Scope | Postgres lanes won | sqlite lanes won | Overall winner |
| --- | ---: | ---: | --- |
| Loopback contrast lanes | 0 | 20 | sqlite |

## RTT Sensitivity Scorecard

| Workload | Injected RTT vs loopback latency | Interpretation |
| --- | ---: | --- |
| document CRUD throughput | 165.43x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| point read latency | 23.80x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| indexed query latency | 19.20x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| composite indexed query latency | 18.00x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| durable journal stream latency | 52.20x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| durable journal bootstrap latency | 26.09x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| subscription bootstrap plus catch-up latency | 17.81x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| subscription fan-out latency | 19.91x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| concurrent multi-tenant mixed read/write load | 58.08x | higher is worse; this is the steady-state sensitivity to non-zero RTT |
| tenant create/open/delete latency | 17.53x | higher is worse; this is the steady-state sensitivity to non-zero RTT |

## document CRUD throughput

async insert + update + delete through the canonical service mutation path

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 344.90 us | 366.46 us | 353.36 us | 19.98 us | 5.66% | 339.07 us - 367.65 us | 2899.40 |
| postgres (loopback) | 10 | 2.96 ms | 3.15 ms | 2.99 ms | 159.07 us | 5.31% | 2.88 ms - 3.11 ms | 337.46 |

postgres (loopback) vs sqlite on the steady-state lane: `0.12x` median ops/s, `0.12x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 371.75 us | 406.24 us | 380.15 us | 17.56 us | 4.62% | 365.47 us - 394.83 us | 2690.00 |
| postgres (loopback) | 8 | 3.23 ms | 3.31 ms | 3.19 ms | 132.16 us | 4.14% | 3.08 ms - 3.30 ms | 309.52 |

postgres (loopback) vs sqlite on the cold-start lane: `0.12x` median ops/s, `0.12x` median per-op latency

### RTT-Sensitive lane

compares Postgres loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| postgres (loopback) | 4 | 3.84 ms | 5.55 ms | 3.83 ms | 2.00 ms | 52.29% | 643.77 us - 7.02 ms | 260.62 |
| postgres (injected RTT) | 4 | 634.76 ms | 634.77 ms | 634.39 ms | 793.74 us | 0.13% | 633.13 ms - 635.65 ms | 1.58 |

postgres (injected RTT) vs postgres (loopback) on the rtt-sensitive lane: `0.01x` median ops/s, `0.01x` median per-op latency

## point read latency

batched async `get_document_async` over seeded documents

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 758.00 ns | 773.00 ns | 762.00 ns | 16.00 ns | 2.15% | 750.00 ns - 774.00 ns | 1319261.21 |
| postgres (loopback) | 10 | 783.00 ns | 804.00 ns | 785.00 ns | 14.00 ns | 1.72% | 775.00 ns - 795.00 ns | 1277139.21 |

postgres (loopback) vs sqlite on the steady-state lane: `0.97x` median ops/s, `0.97x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 50.76 us | 51.86 us | 50.75 us | 3.24 us | 6.38% | 48.05 us - 53.46 us | 19699.39 |
| postgres (loopback) | 8 | 10.22 ms | 10.32 ms | 10.24 ms | 86.47 us | 0.84% | 10.17 ms - 10.31 ms | 97.89 |

postgres (loopback) vs sqlite on the cold-start lane: `0.00x` median ops/s, `0.00x` median per-op latency

### RTT-Sensitive lane

compares Postgres loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| postgres (loopback) | 4 | 14.35 ms | 16.07 ms | 14.17 ms | 2.71 ms | 19.10% | 9.87 ms - 18.48 ms | 69.69 |
| postgres (injected RTT) | 4 | 341.51 ms | 343.02 ms | 341.33 ms | 2.14 ms | 0.63% | 337.93 ms - 344.73 ms | 2.93 |

postgres (injected RTT) vs postgres (loopback) on the rtt-sensitive lane: `0.04x` median ops/s, `0.04x` median per-op latency

## indexed query latency

single-field `status` equality query through the planner-selected index path

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 1.23 ms | 1.37 ms | 1.26 ms | 85.07 us | 6.74% | 1.20 ms - 1.32 ms | 811.54 |
| postgres (loopback) | 10 | 18.10 ms | 18.57 ms | 18.21 ms | 298.71 us | 1.64% | 17.99 ms - 18.42 ms | 55.25 |

postgres (loopback) vs sqlite on the steady-state lane: `0.07x` median ops/s, `0.07x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 1.65 ms | 1.77 ms | 1.66 ms | 101.58 us | 6.10% | 1.58 ms - 1.75 ms | 604.68 |
| postgres (loopback) | 8 | 16.92 ms | 17.24 ms | 16.75 ms | 510.24 us | 3.05% | 16.32 ms - 17.17 ms | 59.10 |

postgres (loopback) vs sqlite on the cold-start lane: `0.10x` median ops/s, `0.10x` median per-op latency

### RTT-Sensitive lane

compares Postgres loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| postgres (loopback) | 4 | 25.97 ms | 28.78 ms | 27.74 ms | 6.10 ms | 21.98% | 18.04 ms - 37.44 ms | 38.51 |
| postgres (injected RTT) | 4 | 498.66 ms | 498.86 ms | 490.86 ms | 16.38 ms | 3.34% | 464.80 ms - 516.91 ms | 2.01 |

postgres (injected RTT) vs postgres (loopback) on the rtt-sensitive lane: `0.05x` median ops/s, `0.05x` median per-op latency

## composite indexed query latency

three-field composite index query with exact-prefix + range filters

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 730.70 us | 794.50 us | 729.70 us | 59.86 us | 8.20% | 686.88 us - 772.52 us | 1368.55 |
| postgres (loopback) | 10 | 23.70 ms | 24.30 ms | 23.80 ms | 393.08 us | 1.65% | 23.52 ms - 24.08 ms | 42.20 |

postgres (loopback) vs sqlite on the steady-state lane: `0.03x` median ops/s, `0.03x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 957.94 us | 1.01 ms | 958.19 us | 76.05 us | 7.94% | 894.60 us - 1.02 ms | 1043.91 |
| postgres (loopback) | 8 | 20.81 ms | 21.13 ms | 20.73 ms | 637.41 us | 3.08% | 20.19 ms - 21.26 ms | 48.06 |

postgres (loopback) vs sqlite on the cold-start lane: `0.05x` median ops/s, `0.05x` median per-op latency

### RTT-Sensitive lane

compares Postgres loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| postgres (loopback) | 4 | 29.13 ms | 34.95 ms | 29.40 ms | 7.10 ms | 24.14% | 18.10 ms - 40.69 ms | 34.32 |
| postgres (injected RTT) | 4 | 524.50 ms | 526.13 ms | 526.47 ms | 5.36 ms | 1.02% | 517.95 ms - 534.99 ms | 1.91 |

postgres (injected RTT) vs postgres (loopback) on the rtt-sensitive lane: `0.06x` median ops/s, `0.06x` median per-op latency

## durable journal stream latency

async `stream_durable_journal_async` from cursor 0 with a fixed page limit

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 683.38 us | 712.46 us | 689.98 us | 36.23 us | 5.25% | 664.07 us - 715.90 us | 1463.33 |
| postgres (loopback) | 10 | 1.06 ms | 1.11 ms | 1.07 ms | 28.86 us | 2.71% | 1.04 ms - 1.09 ms | 942.16 |

postgres (loopback) vs sqlite on the steady-state lane: `0.64x` median ops/s, `0.64x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 2.16 ms | 2.73 ms | 2.24 ms | 388.56 us | 17.38% | 1.91 ms - 2.56 ms | 462.56 |
| postgres (loopback) | 8 | 8.36 ms | 9.65 ms | 7.36 ms | 2.53 ms | 34.46% | 5.24 ms - 9.48 ms | 119.66 |

postgres (loopback) vs sqlite on the cold-start lane: `0.26x` median ops/s, `0.26x` median per-op latency

### RTT-Sensitive lane

compares Postgres loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| postgres (loopback) | 4 | 8.15 ms | 9.10 ms | 11.18 ms | 6.81 ms | 60.88% | 350.46 us - 22.01 ms | 122.64 |
| postgres (injected RTT) | 4 | 425.61 ms | 426.56 ms | 423.61 ms | 5.34 ms | 1.26% | 415.12 ms - 432.10 ms | 2.35 |

postgres (injected RTT) vs postgres (loopback) on the rtt-sensitive lane: `0.02x` median ops/s, `0.02x` median per-op latency

## durable journal bootstrap latency

async `export_durable_journal_bootstrap_async` on a seeded tenant

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 716.12 us | 786.29 us | 731.62 us | 45.63 us | 6.24% | 698.98 us - 764.25 us | 1396.40 |
| postgres (loopback) | 10 | 5.06 ms | 5.12 ms | 5.03 ms | 96.18 us | 1.91% | 4.96 ms - 5.10 ms | 197.69 |

postgres (loopback) vs sqlite on the steady-state lane: `0.14x` median ops/s, `0.14x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 4.46 ms | 4.94 ms | 4.56 ms | 359.77 us | 7.89% | 4.26 ms - 4.86 ms | 224.03 |
| postgres (loopback) | 8 | 11.00 ms | 14.40 ms | 10.59 ms | 5.29 ms | 49.95% | 6.17 ms - 15.02 ms | 90.95 |

postgres (loopback) vs sqlite on the cold-start lane: `0.41x` median ops/s, `0.41x` median per-op latency

### RTT-Sensitive lane

compares Postgres loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| postgres (loopback) | 4 | 21.53 ms | 24.57 ms | 21.37 ms | 3.72 ms | 17.43% | 15.44 ms - 27.29 ms | 46.45 |
| postgres (injected RTT) | 4 | 561.58 ms | 563.07 ms | 594.67 ms | 75.14 ms | 12.64% | 475.11 ms - 714.22 ms | 1.78 |

postgres (injected RTT) vs postgres (loopback) on the rtt-sensitive lane: `0.04x` median ops/s, `0.04x` median per-op latency

## subscription bootstrap plus catch-up latency

single subscription bootstrap followed by one durable matching update

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 812.69 us | 919.96 us | 820.70 us | 81.44 us | 9.92% | 762.44 us - 878.95 us | 1230.49 |
| postgres (loopback) | 10 | 2.39 ms | 2.72 ms | 2.44 ms | 225.35 us | 9.24% | 2.28 ms - 2.60 ms | 419.20 |

postgres (loopback) vs sqlite on the steady-state lane: `0.34x` median ops/s, `0.34x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 1.11 ms | 1.44 ms | 1.16 ms | 231.06 us | 19.91% | 967.23 us - 1.35 ms | 902.95 |
| postgres (loopback) | 8 | 9.99 ms | 10.18 ms | 9.40 ms | 1.11 ms | 11.78% | 8.47 ms - 10.32 ms | 100.07 |

postgres (loopback) vs sqlite on the cold-start lane: `0.11x` median ops/s, `0.11x` median per-op latency

### RTT-Sensitive lane

compares Postgres loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| postgres (loopback) | 4 | 27.89 ms | 48.88 ms | 28.23 ms | 26.25 ms | 93.00% | 0.00 ns - 69.99 ms | 35.86 |
| postgres (injected RTT) | 4 | 496.61 ms | 501.32 ms | 495.94 ms | 6.47 ms | 1.31% | 485.64 ms - 506.24 ms | 2.01 |

postgres (injected RTT) vs postgres (loopback) on the rtt-sensitive lane: `0.06x` median ops/s, `0.06x` median per-op latency

## subscription fan-out latency

time from one durable matching write to delivery across all active subscriptions

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 46.10 us | 50.89 us | 45.45 us | 3.91 us | 8.60% | 42.65 us - 48.24 us | 21692.92 |
| postgres (loopback) | 10 | 103.30 us | 116.39 us | 105.58 us | 6.49 us | 6.14% | 100.94 us - 110.22 us | 9680.92 |

postgres (loopback) vs sqlite on the steady-state lane: `0.45x` median ops/s, `0.45x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 50.65 us | 64.59 us | 52.27 us | 10.35 us | 19.80% | 43.61 us - 60.92 us | 19743.34 |
| postgres (loopback) | 8 | 201.87 us | 391.12 us | 252.28 us | 107.31 us | 42.54% | 162.55 us - 342.00 us | 4953.61 |

postgres (loopback) vs sqlite on the cold-start lane: `0.25x` median ops/s, `0.25x` median per-op latency

### RTT-Sensitive lane

compares Postgres loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| postgres (loopback) | 4 | 1.04 ms | 1.86 ms | 1.08 ms | 1.01 ms | 93.43% | 0.00 ns - 2.69 ms | 958.79 |
| postgres (injected RTT) | 4 | 20.77 ms | 20.78 ms | 20.74 ms | 407.56 us | 1.96% | 20.09 ms - 21.39 ms | 48.15 |

postgres (injected RTT) vs postgres (loopback) on the rtt-sensitive lane: `0.05x` median ops/s, `0.05x` median per-op latency

## concurrent multi-tenant mixed read/write load

concurrent per-tenant mix of point reads, indexed queries, inserts, and updates

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 167.07 us | 192.06 us | 176.57 us | 24.99 us | 14.15% | 158.69 us - 194.44 us | 5985.37 |
| postgres (loopback) | 10 | 768.44 us | 804.96 us | 743.86 us | 63.25 us | 8.50% | 698.62 us - 789.10 us | 1301.34 |

postgres (loopback) vs sqlite on the steady-state lane: `0.22x` median ops/s, `0.22x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 188.76 us | 221.26 us | 196.44 us | 20.05 us | 10.21% | 179.67 us - 213.21 us | 5297.84 |
| postgres (loopback) | 8 | 782.72 us | 880.04 us | 797.56 us | 60.55 us | 7.59% | 746.93 us - 848.18 us | 1277.60 |

postgres (loopback) vs sqlite on the cold-start lane: `0.24x` median ops/s, `0.24x` median per-op latency

### RTT-Sensitive lane

compares Postgres loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| postgres (loopback) | 4 | 7.00 ms | 9.60 ms | 6.80 ms | 3.55 ms | 52.17% | 1.16 ms - 12.45 ms | 142.87 |
| postgres (injected RTT) | 4 | 406.50 ms | 406.72 ms | 405.85 ms | 2.07 ms | 0.51% | 402.56 ms - 409.14 ms | 2.46 |

postgres (injected RTT) vs postgres (loopback) on the rtt-sensitive lane: `0.02x` median ops/s, `0.02x` median per-op latency

## tenant create/open/delete latency

create a tenant, verify it opens from a peer service when the topology allows it, then delete it cleanly

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 1.01 ms | 1.08 ms | 1.02 ms | 75.33 us | 7.42% | 961.76 us - 1.07 ms | 991.05 |
| postgres (loopback) | 10 | 4.03 ms | 4.60 ms | 4.09 ms | 381.30 us | 9.32% | 3.82 ms - 4.36 ms | 248.32 |

postgres (loopback) vs sqlite on the steady-state lane: `0.25x` median ops/s, `0.25x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 1.74 ms | 2.20 ms | 1.72 ms | 521.34 us | 30.26% | 1.29 ms - 2.16 ms | 573.98 |
| postgres (loopback) | 8 | 10.34 ms | 13.26 ms | 11.35 ms | 2.34 ms | 20.61% | 9.39 ms - 13.30 ms | 96.68 |

postgres (loopback) vs sqlite on the cold-start lane: `0.17x` median ops/s, `0.17x` median per-op latency

### RTT-Sensitive lane

compares Postgres loopback against the same service path through a local injected-latency TCP proxy

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| postgres (loopback) | 4 | 9.54 ms | 12.19 ms | 9.77 ms | 3.93 ms | 40.20% | 3.52 ms - 16.02 ms | 104.78 |
| postgres (injected RTT) | 4 | 167.31 ms | 167.33 ms | 167.59 ms | 1.43 ms | 0.85% | 165.32 ms - 169.86 ms | 5.98 |

postgres (injected RTT) vs postgres (loopback) on the rtt-sensitive lane: `0.06x` median ops/s, `0.06x` median per-op latency

## Pool Pressure Observation

This observation intentionally constrains the Postgres provider pool to expose head-of-line behavior and verify that active pooled backends remain bounded.

| Samples | Max pooled backends observed | Configured max connections | Concurrent workers | Median sample latency | P95 sample latency | Mean sample latency |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 8 | 2 | 2 | 4 | 791.54 us | 1.11 ms | 461.86 ms |

Relative to the unconstrained steady-state Postgres mixed-load lane, the bounded-pool observation shows `1.03x` higher median end-to-end sample latency while pooled backend count remained capped at `2`.

## Operator Assumptions

- Postgres tenant persistence is benchmarked with the global usage/control path still local and redb-backed.
- The service-path benchmark includes provider-owned pooling, typed construction, scheduler/journal semantics, and the provider hint-listener wake path, but notifications remain wake hints rather than the authoritative journal contract.
- Companion operational drills for reconnect recovery, restart recovery, transient backend termination, unloaded-tenant scheduler wake, and tenant cleanup are covered by focused storage/engine verification and recorded in `/Users/jack/src/github.com/agentstation/neovex/docs/plans/archive/postgres-storage-provider-plan.md`.
