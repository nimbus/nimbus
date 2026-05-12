# Replica-Connected SQLite Provider Benchmark Report

Generated with:

```bash
NIMBUS_LIBSQL_URL='http://127.0.0.1:18080' \
NIMBUS_LIBSQL_ADMIN_URL='http://127.0.0.1:18081' \
make bench-libsql-replica-provider WORKLOADS='point-read indexed-query composite-indexed-query barrier-refresh peer-catch-up' ENCRYPTION=temp-master-key-file REPORT=/Users/jack/src/github.com/nimbus/nimbus/docs/plans/research/encryption-at-rest-libsql-replica-encrypted-cache-benchmark-report.md
```

## Methodology

- local cache encryption mode: `temp-master-key-file`
- steady-state lane compares embedded `sqlite` against `libsql replica` with alternating backend order
- cold-start lane compares fresh service open plus the first representative execution for embedded `sqlite` and `libsql replica`
- replica-operational lane measures the real freshness contract shipped today: same-service barrier refresh after a remote-primary write, plus peer catch-up / delegated-write visibility through the provider poll worker
- steady-state warmup rounds: `2`; steady-state measured rounds: `10`
- cold-start warmup rounds: `1`; cold-start measured rounds: `8`
- replica-operational warmup rounds: `1`; replica-operational measured rounds: `10`
- 95% confidence intervals use a two-sided Student-t interval on mean per-operation latency
- encryption-enabled runs use one benchmark-only 32-byte master key file per benchmark process so local redb control state and replica-cache SQLite files both reopen through the same manifest-backed path during the benchmark

## Configuration

- CRUD documents per sample: `24`
- point reads per sample: `100` over `500` seeded documents
- indexed queries per sample: `12` over `1000` seeded documents
- mixed-load tenants: `2` with `40` ops per tenant per sample
- peer catch-up timeout: `6` with `25.00 ms` polling interval
- local cache encryption posture: `manifest-backed encrypted local cache`
- local cache encryption notes: enables the real service startup path with a benchmark-only master key file so control-plane redb and replica cache SQLite files both reopen through manifest-backed DEKs
- report path: `/Users/jack/src/github.com/nimbus/nimbus/docs/plans/research/encryption-at-rest-libsql-replica-encrypted-cache-benchmark-report.md`
- workload filter: `point read latency, indexed query latency, composite indexed query latency, same-service barrier refresh latency, peer catch-up / delegated-write visibility latency`

## SQLite Contrast Scorecard

Winner is determined by higher median ops/s, which is equivalent here to lower median per-op latency.

### Steady-State summary

| Workload | libsql replica vs sqlite | Winner |
| --- | ---: | --- |
| point read latency | 0.98x | sqlite |
| indexed query latency | 1.01x | libsql replica |
| composite indexed query latency | 1.06x | libsql replica |
| Total lanes won | libsql replica 2, sqlite 1 | libsql replica |

### Cold-Start summary

| Workload | libsql replica vs sqlite | Winner |
| --- | ---: | --- |
| point read latency | 0.03x | sqlite |
| indexed query latency | 0.05x | sqlite |
| composite indexed query latency | 0.04x | sqlite |
| Total lanes won | libsql replica 0, sqlite 3 | sqlite |

### Overall total

| Scope | libsql replica lanes won | sqlite lanes won | Overall winner |
| --- | ---: | ---: | --- |
| All contrast lanes | 2 | 4 | sqlite |

## point read latency

batched async `get_document_async` over seeded documents

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 989.00 ns | 1.06 us | 1.01 us | 54.00 ns | 5.37% | 968.00 ns - 1.05 us | 1011122.35 |
| libsql replica | 10 | 1.01 us | 1.06 us | 1.03 us | 64.00 ns | 6.18% | 982.00 ns - 1.07 us | 990099.01 |

libsql replica vs sqlite on the steady-state lane: `0.98x` median ops/s, `0.98x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 23.59 us | 27.28 us | 24.35 us | 4.60 us | 18.90% | 20.51 us - 28.20 us | 42394.44 |
| libsql replica | 8 | 681.54 us | 788.82 us | 713.80 us | 60.02 us | 8.41% | 663.61 us - 763.98 us | 1467.27 |

libsql replica vs sqlite on the cold-start lane: `0.03x` median ops/s, `0.03x` median per-op latency

## indexed query latency

single-field `status` equality query through the planner-selected index path

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 304.94 us | 314.05 us | 304.81 us | 12.16 us | 3.99% | 296.11 us - 313.50 us | 3279.34 |
| libsql replica | 10 | 302.52 us | 330.10 us | 305.80 us | 18.11 us | 5.92% | 292.84 us - 318.75 us | 3305.58 |

libsql replica vs sqlite on the steady-state lane: `1.01x` median ops/s, `1.01x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 409.72 us | 503.78 us | 428.04 us | 67.39 us | 15.74% | 371.68 us - 484.39 us | 2440.71 |
| libsql replica | 8 | 8.59 ms | 8.79 ms | 8.58 ms | 243.13 us | 2.83% | 8.38 ms - 8.78 ms | 116.47 |

libsql replica vs sqlite on the cold-start lane: `0.05x` median ops/s, `0.05x` median per-op latency

## composite indexed query latency

three-field composite index query with exact-prefix + range filters

### Steady-State lane

reuses warmed services and alternates backend order every round

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 10 | 194.52 us | 201.79 us | 195.62 us | 9.40 us | 4.81% | 188.89 us - 202.34 us | 5140.78 |
| libsql replica | 10 | 183.02 us | 214.43 us | 190.46 us | 16.48 us | 8.65% | 178.67 us - 202.25 us | 5463.79 |

libsql replica vs sqlite on the steady-state lane: `1.06x` median ops/s, `1.06x` median per-op latency

### Cold-Start lane

times a fresh service/runtime open plus the first representative execution

| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |
| sqlite | 8 | 294.09 us | 320.34 us | 290.93 us | 34.47 us | 11.85% | 262.11 us - 319.75 us | 3400.32 |
| libsql replica | 8 | 8.19 ms | 8.39 ms | 8.18 ms | 208.35 us | 2.55% | 8.00 ms - 8.35 ms | 122.11 |

libsql replica vs sqlite on the cold-start lane: `0.04x` median ops/s, `0.04x` median per-op latency

## Replica Freshness Drills

These lanes are the operational readiness gate for the shipped replica contract. They are intentionally replica-only because embedded SQLite has no corresponding remote-primary barrier or peer catch-up path.

| Drill | Samples | Median latency | P95 latency | Mean latency | 95% CI of mean | Result |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| same-service barrier refresh latency | 10 | 2.91 ms | 3.53 ms | 2.91 ms | 2.54 ms - 3.28 ms | pass |
| peer catch-up / delegated-write visibility latency | 10 | 539.39 ms | 541.52 ms | 535.20 ms | 527.43 ms - 542.97 ms | pass |

## Operator Assumptions

- Replica-connected SQLite tenant persistence is benchmarked with the global usage/control path still local and redb-backed.
- The live freshness contract in this first slice is provider-owned cache refresh or poll-driven catch-up, not an ad hoc direct-primary query bypass from planner code.
- The peer catch-up drill is the delegated-write readiness check for this family: one authoritative remote primary accepts the write, and another service becomes fresh only after the provider poll worker re-establishes journal/cache proof.
