# Layered SQLite write overhead

workload: 256 phased CRUD units / 768 logical mutations; batch distribution: `[5, 251, 90, 256, 20, 146]`; rounds: 12; repetitions/sample: 60; WAL + `synchronous=FULL`; bundled SQLCipher SQLite

I/O evidence reports the fieldwise maximum observed across every measured repetition and round.

SQLite runtime: `3.51.3`; SQLCipher: `4.14.0 community`; source id: `2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6alt1`

| lane | logical mut/s | 95% CI | median | CV% | SQL stmt/s | row changes/s | tx/s | sync commits/s | mean elapsed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| raw row mutation | 323803 | [313290, 334316] | 325282 | 5.1 | 328862 | 323803 | 2529.7 | 2529.7 | 2.377 ms |
| current-loop SQL, resident connection | 45851 | [45240, 46463] | 45929 | 2.1 | 385021 | 138271 | 716.4 | 716.4 | 16.757 ms |
| guarded prepared/hoisted SQL | 129579 | [121842, 137315] | 132465 | 9.4 | 573824 | 390760 | 2024.7 | 2024.7 | 5.981 ms |
| Nimbus-shaped SQL lower bound | 151286 | [146382, 156191] | 151358 | 5.1 | 463315 | 456223 | 2363.9 | 2363.9 | 5.089 ms |
| production storage append+apply | 99716 | [93506, 105925] | 102430 | 9.8 | 867318 | 300965 | 1558.1 | 1558.1 | 7.790 ms |
## Bytes and checkpoint state

| lane | DB bytes | WAL bytes | page size | WAL frames | checkpointed frames | autocheckpoint pages |
|---|---:|---:|---:|---:|---:|---:|
| raw row mutation | 4096 | 712792 | 4096 | 173 | 173 | 1000 |
| current-loop SQL, resident connection | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| guarded prepared/hoisted SQL | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| Nimbus-shaped SQL lower bound | 4096 | 1404952 | 4096 | 341 | 341 | 1000 |
| production storage append+apply | 4096 | 1396712 | 4096 | 339 | 339 | 1000 |

## Raw measured-round samples

- **raw row mutation:** 303711.369, 363458.294, 331172.493, 302518.941, 316612.804, 333535.192, 314498.195, 330585.530, 325689.483, 329659.811, 324873.864, 309319.357
- **current-loop SQL, resident connection:** 47225.068, 47091.957, 46114.709, 45542.164, 43998.478, 45271.461, 46067.930, 45806.245, 44701.557, 45393.113, 46954.032, 46051.226
- **guarded prepared/hoisted SQL:** 132838.489, 133446.019, 137736.641, 137460.668, 123897.181, 108838.567, 103491.490, 131876.701, 129394.448, 132090.620, 139066.997, 144804.417
- **Nimbus-shaped SQL lower bound:** 160183.912, 157827.045, 159094.119, 157369.255, 138288.885, 150758.495, 159176.461, 144743.847, 151957.922, 149551.415, 147017.731, 139468.419
- **production storage append+apply:** 97355.300, 71386.917, 107162.926, 100190.021, 103974.786, 100211.992, 108126.479, 105076.453, 94186.268, 100885.927, 103990.008, 104040.419

## CPU-only serialization

Production record MessagePack plus the current document JSON/typed-field encoding work: **772557 logical mutations/s** (0.994 ms for one 768-mutation fixture). This lane performs no SQLite I/O and is not a durability throughput result.

## Connection and initialization cost

| operation | mean µs/op |
|---|---:|
| `Connection::open` only | 42.2 |
| production-equivalent connection init on initialized DB | 547.1 |
| `SqliteTenantStore::open` + schema load | 538.6 |

