# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 309499 | [296059, 322939] | 310213 | 6.8 | 314335 | 309499 | 2407.3 | 2407.3 | 2.492 ms |
| current-loop SQL, resident connection | 46079 | [45348, 46811] | 46161 | 2.5 | 386934 | 138958 | 719.6 | 719.6 | 16.677 ms |
| guarded prepared/hoisted SQL | 99071 | [80493, 117649] | 104931 | 29.5 | 438724 | 298761 | 1280.9 | 1280.9 | 9.368 ms |
| Nimbus-shaped SQL lower bound | 106704 | [101792, 111617] | 108637 | 7.2 | 326781 | 321780 | 1659.0 | 1659.0 | 7.233 ms |
| production storage append+apply | 23707 | [22103, 25312] | 24198 | 10.6 | 206205 | 71554 | 366.2 | 366.2 | 32.768 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 704552 | 4096 | 171 | 171 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 326852.930, 305012.217, 277535.048, 313385.089, 319300.791, 272655.865, 339485.136, 335057.510, 322579.893, 290660.371, 304418.468, 307040.501
- **current-loop SQL, resident connection:** 43975.494, 45586.657, 46817.294, 47348.498, 48044.082, 44463.920, 46533.915, 46466.710, 45854.533, 45508.195, 46692.857, 45659.751
- **guarded prepared/hoisted SQL:** 109202.045, 122100.064, 120980.605, 132618.742, 89460.530, 25659.506, 120818.381, 86630.967, 100659.212, 94331.037, 115137.490, 71253.044
- **Nimbus-shaped SQL lower bound:** 96451.674, 92616.390, 101593.162, 113860.360, 105904.061, 117953.050, 114646.460, 108552.229, 108721.992, 99669.248, 108755.693, 111725.151
- **production storage append+apply:** 26465.700, 25988.387, 23619.480, 24370.296, 24658.612, 24025.451, 18616.103, 25815.174, 26340.177, 21009.657, 23192.244, 20387.809

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **472580 logical mutations/s** (1.625 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 143.1 |
| production-equivalent connection init on initialized DB | 1000.0 |
| `SqliteTenantStore::open` + schema load | 1049.8 |

