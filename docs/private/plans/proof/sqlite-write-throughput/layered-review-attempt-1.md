# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 294214 | [284254, 304175] | 293721 | 5.3 | 298811 | 294214 | 2292.6 | 2292.6 | 2.617 ms |
| current-loop SQL, resident connection | 48682 | [46495, 50868] | 50164 | 7.1 | 408788 | 146806 | 756.8 | 756.8 | 15.856 ms |
| guarded prepared/hoisted SQL | 147646 | [143131, 152161] | 150375 | 4.8 | 653835 | 445246 | 2301.8 | 2301.8 | 5.213 ms |
| Nimbus-shaped SQL lower bound | 159503 | [142992, 176013] | 167200 | 16.3 | 488477 | 481000 | 2384.9 | 2384.9 | 5.032 ms |
| production storage append+apply | 36678 | [33356, 40000] | 38394 | 14.3 | 319021 | 110702 | 557.0 | 557.0 | 21.546 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 708672 | 4096 | 172 | 172 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 298568.794, 296443.702, 286974.593, 272758.757, 267965.329, 293003.886, 294439.015, 324549.714, 286736.565, 292440.838, 301369.676, 315321.733
- **current-loop SQL, resident connection:** 47720.360, 45422.897, 46252.864, 40171.607, 49856.859, 50471.198, 51038.322, 50567.934, 47914.262, 51985.772, 51900.342, 50880.157
- **guarded prepared/hoisted SQL:** 151550.095, 150624.270, 138820.216, 153894.196, 152451.637, 131059.650, 142203.458, 148466.542, 150126.267, 145201.943, 152963.079, 154393.927
- **Nimbus-shaped SQL lower bound:** 158733.258, 174296.034, 168658.847, 164381.972, 169588.053, 158704.348, 165741.557, 79645.019, 155896.001, 169018.448, 171710.709, 177657.404
- **production storage append+apply:** 37904.588, 38696.745, 21350.762, 32796.194, 36098.206, 38673.490, 37569.556, 40393.836, 38114.949, 39317.131, 39489.079, 39729.599

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **643885 logical mutations/s** (1.193 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 45.1 |
| production-equivalent connection init on initialized DB | 515.1 |
| `SqliteTenantStore::open` + schema load | 478.8 |

