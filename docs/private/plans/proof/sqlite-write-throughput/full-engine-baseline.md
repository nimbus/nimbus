# Full Engine SQLite Baseline

Base: `e47b64eacc3d54dc5bfe7d51727306a81cfacb28`

Date: 2026-07-27

Workload: closed-loop, one tenant, phased disjoint CRUD, SQLite, release,
N=1/32/256, 100 units/worker bounded to 9,000 mutations/round, three warmups,
15 measured rounds, commit-phase split enabled.

## Result

| N | Mean mut/s | 95% CI | Median | CV | Speedup | p50/p95/p99 µs | Avg batch |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 1,711 | 1,682–1,740 | 1,727 | 3.1% | 1.00× | 567.2 / 705.6 / 867.2 | 1.00 |
| 32 | 13,510 | 13,273–13,748 | 13,652 | 3.2% | 7.90× | 2,289.5 / 3,334.5 / 4,929.2 | 16.34 |
| 256 | 21,433 | 20,753–22,112 | 21,920 | 5.7% | 12.53× | 11,085.2 / 19,150.1 / 23,993.2 | 142.22 |

## Raw measured-round samples

- **N=1:** 1726.540, 1650.781, 1630.867, 1609.096, 1655.557,
  1676.889, 1720.490, 1758.212, 1747.740, 1747.634, 1730.282,
  1718.690, 1748.603, 1780.176, 1759.533
- **N=32:** 12795.974, 14057.922, 13541.758, 13109.657, 13593.973,
  13798.431, 13822.016, 13968.599, 13808.166, 13855.772, 13651.562,
  13656.624, 12836.317, 12820.995, 13336.562
- **N=256:** 21920.175, 22041.603, 20308.319, 21975.278, 22005.236,
  21198.953, 17524.176, 21168.493, 22108.957, 22373.832, 22019.709,
  21311.367, 21256.179, 22607.385, 21670.130

## Phase split

| N | Window/storage prepare | Plan CPU | Conflict | Apply + publish | First append | Measured phase time |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 4500 / 0 | 1.8% | 0.2% | 53.0% | 45.0% | 2,450.485 ms |
| 32 | 133920 / 0 | 13.9% | 1.8% | 54.5% | 29.7% | 10,350.653 ms |
| 256 | 126720 / 0 | 26.0% | 4.6% | 51.8% | 17.6% | 6,062.050 ms |

`First append` covers `append_durable_records_batch` only. The second FULL
SQLite transaction and its sync are inside `Apply + publish`.

## Command

```bash
timeout 600 env \
  NIMBUS_CWB_WORKLOAD=crud \
  NIMBUS_CWB_LADDER=1,32,256 \
  NIMBUS_CWB_OPS_PER_WORKER=100 \
  NIMBUS_CWB_MAX_MUTATIONS_PER_ROUND=9000 \
  NIMBUS_CWB_MEASURE_ROUNDS=15 \
  NIMBUS_CWB_WARMUP_ROUNDS=3 \
  NIMBUS_CWB_SPLIT_PHASES=1 \
  NIMBUS_CWB_OUT=/tmp/sqlite-write-overhead-cwb-stable.md \
  /Users/jack/src/github.com/nimbus/nimbus/target/release/deps/concurrent_write_throughput-d0d97d4acf36a759
```
